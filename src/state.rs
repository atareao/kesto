//! Estado compartido de la aplicación.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use tokio::sync::{Mutex, RwLock};

use crate::auth::oidc::Discovery;
use crate::config::{AuthMode, Config, ConfigError, OidcSettings};
use crate::db::crypto::Clave;
use crate::db::entities::{Ejecuciones, Repositorios, Tareas, Targets};
use crate::engine::Motor;
use crate::secret::SecretString;

/// Caducidad de un `state` emitido en el login: 5 minutos.
///
/// Un login que no se completa en ese plazo es un intento abandonado. La
/// comprobación de verdad se hace al consumir, en `AuthState::consumir_login`
/// —es el defecto de la plantilla que este change corrige: guardaba la marca de
/// tiempo y nunca la miraba—.
pub const STATE_TTL: Duration = Duration::from_secs(5 * 60);

/// Un login en curso: el `nonce` emitido y el instante en que se emitió.
///
/// El `nonce` se guarda **junto** al `state` y no en un mapa aparte: el callback
/// lo compara con el del ID token al verificar, y con la asociación hecha
/// —`state → (nonce, instante)`— esa comparación no depende de nada más.
#[derive(Debug, Clone)]
pub struct PendingLogin {
    /// `nonce` que viajó en la petición de autorización.
    pub nonce: String,
    /// Instante de emisión, para aplicar el TTL.
    pub issued_at: Instant,
}

impl PendingLogin {
    /// ¿Ha caducado esta entrada en `ahora`?
    ///
    /// `saturating_duration_since` y no `duration_since`: un instante anterior
    /// al de emisión —imposible con un reloj monótono, pero gratis de cubrir—
    /// no puede entrar en pánico.
    pub fn expirado_en(&self, ahora: Instant) -> bool {
        ahora.saturating_duration_since(self.issued_at) >= STATE_TTL
    }
}

/// Por qué se rechaza un `state` al consumirlo en el callback.
///
/// Las tres condiciones son distintas y se distinguen en el código y en el
/// registro, aunque el mensaje que ve el cliente sea el mismo: quien no ha
/// entrado no aprende si su `state` era inventado, caducado o ya gastado. El
/// defecto de la plantilla que este change corrige es, precisamente, que la
/// caducidad no se comprobaba; separar los tres casos es lo que hace que la
/// comprobación no se pueda perder sin que un test lo note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RechazoState {
    /// El `state` no se emitió nunca: no está en el mapa ni entre los gastados.
    Desconocido,
    /// El `state` se emitió, pero hace más de [`STATE_TTL`].
    Caducado,
    /// El `state` ya lo gastó un callback anterior.
    Reutilizado,
}

/// Estado de la autenticación.
///
/// Existe **sólo** cuando el modo es `Oidc`: en `Disabled` no hay capa ninguna
/// que montar, y guardar un secreto que nadie va a usar sería guardar un secreto
/// de más. Por eso `AppState::auth` es un `Option` y no un campo vacío: el modo
/// se lee en el tipo, y quien tenga un `AuthState` en la mano sabe que hay
/// autenticación sin consultar la configuración.
#[derive(Debug, Clone)]
pub struct AuthState {
    /// Clave de firma de la sesión (`KESTO_SECRET`), ya resuelta por `Config`.
    ///
    /// Es un [`SecretString`] y no un `String` para que no pueda salir por
    /// `Debug` ni acabar en una línea de registro por descuido.
    pub secret: SecretString,
    /// Configuración del proveedor: emisor, `client_id`, secreto y retorno.
    ///
    /// Va tras un [`Arc`] porque el estado se clona en cada petición: clonar el
    /// `Arc` no vuelve a copiar las cadenas.
    pub(crate) settings: Arc<OidcSettings>,
    /// Descubrimiento cacheado del proveedor: `None` hasta que conteste.
    ///
    /// Va tras un [`RwLock`] y no un `Mutex`: el servidor es concurrente, y
    /// varios logins pueden leer la caché a la vez sin estorbarse. Se escribe
    /// solo cuando llega el primer resultado.
    pub(crate) discovery: Arc<RwLock<Option<Discovery>>>,
    /// Logins en curso: `state → (nonce, instante de emisión)`.
    ///
    /// Tras un [`Mutex`] porque insertar y purgar son mutaciones: el mapa no
    /// puede crecer sin límite con cada intento de entrada que nadie completa.
    pub(crate) logins: Arc<Mutex<HashMap<String, PendingLogin>>>,
    /// `state` ya consumidos, con el instante en que se gastaron.
    ///
    /// Se recuerdan para poder distinguir «reutilizado» de «desconocido» sin
    /// confundirlos: si un `state` consumido se descartara sin dejar rastro, un
    /// segundo callback con el mismo `state` parecería uno inventado. La purga
    /// por TTL acota su crecimiento igual que la de los pendientes.
    pub(crate) consumidos: Arc<Mutex<HashMap<String, Instant>>>,
}

impl AuthState {
    /// Construye el estado de autenticación en modo `Oidc`.
    pub(crate) fn new(settings: OidcSettings, secret: SecretString) -> Self {
        Self {
            secret,
            settings: Arc::new(settings),
            discovery: Arc::new(RwLock::new(None)),
            logins: Arc::new(Mutex::new(HashMap::new())),
            consumidos: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Registra un `state` emitido junto a su `nonce` y su marca de tiempo.
    ///
    /// Purga las entradas caducadas **al insertar**: sin esto el mapa crecería
    /// con cada login que nadie termina —un `state` que caduca sin consumirse— y
    /// el proceso acabaría guardando la memoria de intentos abandonados.
    ///
    /// AVISO — esto acota el mapa a la ventana del TTL, pero **no le pone un
    /// tope duro**: un caudal alto de logins dentro de esos cinco minutos lo hace
    /// crecer sin límite. La spec solo exige purgar por caducidad, así que el
    /// tope no se implementa aquí; el consumo del `state` en el callback añade la
    /// purga al consumir, pero tampoco un tope.
    pub async fn registrar_login(&self, state: &str, nonce: &str) {
        let ahora = Instant::now();
        let mut logins = self.logins.lock().await;

        logins.retain(|_, pendiente| !pendiente.expirado_en(ahora));
        logins.insert(
            state.to_owned(),
            PendingLogin {
                nonce: nonce.to_owned(),
                issued_at: ahora,
            },
        );
    }

    /// Consume un `state`: lo extrae del mapa y lo descarta una sola vez.
    ///
    /// Distingue los tres rechazos —desconocido, caducado y reutilizado— y
    /// comprueba la caducidad **de verdad** al consumir, no solo la registra:
    /// es el defecto de la plantilla que este change corrige. La purga de
    /// entradas caducadas y de marcas de consumo viejas ocurre aquí, de modo
    /// que el mapa y su rastro de gastados no crecen sin límite.
    pub(crate) async fn consumir_login(&self, state: &str) -> Result<PendingLogin, RechazoState> {
        let ahora = Instant::now();
        let mut logins = self.logins.lock().await;
        let mut consumidos = self.consumidos.lock().await;

        // La entrada objetivo se saca **antes** de purgar: si la purga la
        // borrara primero, una entrada caducada se volvería indistinguible de una
        // desconocida y las dos condiciones dejarían de separarse.
        let entrada = logins.remove(state);

        // Purga al consumir: pendientes caducados y marcas de consumo viejas. El
        // mapa no crece con intentos abandonados, ni el rastro de gastados con el
        // tiempo.
        logins.retain(|_, pendiente| !pendiente.expirado_en(ahora));
        consumidos.retain(|_, gastado_en| ahora.saturating_duration_since(*gastado_en) < STATE_TTL);

        match entrada {
            None => {
                if consumidos.contains_key(state) {
                    Err(RechazoState::Reutilizado)
                } else {
                    Err(RechazoState::Desconocido)
                }
            }
            Some(pendiente) if pendiente.expirado_en(ahora) => Err(RechazoState::Caducado),
            Some(pendiente) => {
                consumidos.insert(state.to_owned(), ahora);
                Ok(pendiente)
            }
        }
    }
}

/// El estado de base: el pool, la clave de cifrado y las **cuatro** capas de acceso.
///
/// Se construye **una sola vez**, al arrancar, y viaja dentro de [`AppState`] como
/// campo **no opcional**: un Kesto sin base de datos no es un estado válido —sin base
/// no hay panel ni una fila que servir— y un campo `Option` lo declararía posible. El
/// pool llega **ya abierto y migrado** desde fuera, así que [`AppState::new`] no
/// necesita ser asíncrona: quien abre la base y aplica el esquema es `src/main.rs` (y
/// el arnés de pruebas), no el constructor del estado.
///
/// Las cuatro capas se construyen sobre **este** pool y **esta** clave, y no sobre
/// otras: abrir un segundo pool sobre el mismo fichero no falla al abrirse —SQLite
/// admite varias conexiones y WAL tolera que las dos lean y escriban— y el daño
/// aparece más tarde y de forma intermitente, como una fila que una petición acaba de
/// escribir y otra no ve. Y hay un motivo de configuración: los pragmas que la base
/// necesita —`foreign_keys`, que es **por conexión**, WAL y el `busy_timeout`— se
/// fijan en las opciones del pool que se abre aquí y en ningún otro sitio; un pool
/// abierto por otro camino tendría que acordarse de fijarlos.
///
/// Las cuatro capas van tras un [`Arc`] porque el estado **se clona en cada petición**
/// y las capas no son clonables: clonar el estado no vuelve a construir ninguna, solo
/// copia el puntero. La clave también va en un `Arc` porque es el material de cifrado
/// del proceso, y cada capa que lo necesita recibe **el mismo**.
#[derive(Clone)]
pub struct EstadoDeBase {
    /// El pool abierto y migrado al arrancar: **el único** de la instancia.
    pub pool: SqlitePool,
    /// La clave con la que se cifran y descifran los tres secretos del §3.
    pub clave: Arc<Clave>,
    /// La capa de acceso de `repositories`.
    pub repositorios: Arc<Repositorios>,
    /// La capa de acceso de `backup_targets`.
    pub targets: Arc<Targets>,
    /// La capa de acceso de `backup_jobs`.
    pub tareas: Arc<Tareas>,
    /// La capa de acceso de `job_executions`.
    pub ejecuciones: Arc<Ejecuciones>,
}

impl EstadoDeBase {
    /// Construye las cuatro capas sobre el pool y la clave **dados**.
    ///
    /// `Tareas` y `Ejecuciones` no reciben la clave: `backup_jobs` y `job_executions`
    /// no tienen ninguna columna de secreto, y pasarle una clave que no se usa
    /// sugeriría que ahí se cifra algo.
    pub fn nuevo(pool: SqlitePool, clave: Arc<Clave>) -> Self {
        Self {
            repositorios: Arc::new(Repositorios::nuevo(pool.clone(), clave.clone())),
            targets: Arc::new(Targets::nuevo(pool.clone(), clave.clone())),
            tareas: Arc::new(Tareas::nuevo(pool.clone())),
            ejecuciones: Arc::new(Ejecuciones::nuevo(pool.clone())),
            pool,
            clave,
        }
    }
}

/// Estado que comparten los manejadores de la API.
#[derive(Clone)]
pub struct AppState {
    /// Configuración efectiva, cargada una sola vez al arrancar.
    pub config: Arc<Config>,
    /// Estado de la autenticación: `None` cuando no se monta ninguna capa.
    pub auth: Option<AuthState>,
    /// El estado de base: el pool, la clave y las cuatro capas de acceso.
    ///
    /// **No** es un `Option`, y no es un detalle de estilo: un estado sin base
    /// declararía válido un Kesto sin base de datos, y no lo es. El tipo lo dice, así
    /// que no hay ninguna forma de construir un estado servible sin haber abierto la
    /// base antes.
    pub db: EstadoDeBase,
    /// El motor de copias: la cola, los trabajadores y el hub de eventos.
    ///
    /// **Tampoco** es un `Option`, y por el mismo razonamiento que la base: un estado
    /// sin motor declararía válido un Kesto que no puede copiar. Y ahí el daño sería
    /// peor que un `404`, porque el panel estaría en pie y contestaría `200` a las
    /// rutas del §5 mientras la primera copia descubriera que no hay quien la
    /// ejecute. El tipo lo dice: no hay estado servible sin motor, y montarlo es cosa
    /// del arranque, no del constructor —el motor captura el `Handle` del runtime, y
    /// eso solo se puede hacer desde dentro de uno—.
    ///
    /// Se clona como el resto del estado, y clonarlo no cuesta nada: el motor es un
    /// `Arc` por dentro, así que un clon no copia ni la cola ni un solo evento.
    pub motor: Motor,
}

impl AppState {
    /// Construye el estado a partir de la configuración efectiva, la base **ya
    /// abierta** y el motor **ya arrancado**.
    ///
    /// Falla —en vez de degradar a superficie pública en silencio— si el modo
    /// es `Oidc` y no hay clave de firma. `Config` ya garantiza que en `Oidc` la
    /// clave está, así que ese camino es inalcanzable por construcción; y
    /// precisamente por eso no puede quedarse en un `None`, que aquí significaría
    /// «panel sin *gate*»: un fallo de invariante no puede abrir la superficie
    /// sin que nadie se entere.
    ///
    /// La base y el motor entran por parámetro **ya construidos** en vez de abrirse
    /// aquí: abrir la base y arrancar el motor exigen, respectivamente, un `await` y
    /// estar dentro de un runtime, y el orden —abrir, migrar, montar el motor y solo
    /// entonces escuchar— es una decisión del arranque: un fallo de migración es un
    /// arranque fallido, no un modo degradado. Con las dos piezas fuera, esta función
    /// sigue siendo síncrona y no puede quedarse a medias.
    pub fn new(config: Config, db: EstadoDeBase, motor: Motor) -> Result<Self, ConfigError> {
        let auth = match &config.auth {
            AuthMode::Oidc(settings) => match &config.secret {
                Some(secret) => Some(AuthState::new(settings.clone(), secret.clone())),
                None => {
                    return Err(ConfigError::MissingVariable {
                        name: "KESTO_SECRET",
                    });
                }
            },
            AuthMode::Disabled => None,
        };

        Ok(Self {
            config: Arc::new(config),
            auth,
            db,
            motor,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use time::OffsetDateTime;

    use super::{EstadoDeBase, PendingLogin, STATE_TTL};
    use crate::config::Config;
    use crate::db::entities::{
        AltaDeEjecucion, AltaDeRepositorio, AltaDeTarea, AltaDeTarget, Estado, TipoDeTarget,
    };
    use crate::secret::SecretString;
    use crate::test_support::{
        MASTER_KEY, OIDC_CLIENT_ID, OIDC_CLIENT_SECRET, OIDC_ISSUER_URL, SESSION_SECRET, auth_oidc,
        base_con_clave, config_con_dsn, estado_de_prueba, lookup_from,
    };

    #[tokio::test]
    async fn la_purga_descarta_las_entradas_caducadas() {
        // Tarea 3.5: el mapa de `state`/`nonce` no crece sin límite. Borrar el
        // `retain` de `registrar_login` deja este test en rojo: la entrada
        // caducada sobreviviría y el mapa tendría dos entradas en vez de una.
        let auth = auth_oidc().await;

        // La entrada se inserta con su marca de tiempo **ya retrasada** más allá
        // del TTL: determinista, sin dormir y sin inyectar un reloj en el estado
        // de producción.
        let hace = Instant::now()
            .checked_sub(STATE_TTL + Duration::from_secs(1))
            .expect("la purga necesita un uptime mayor que STATE_TTL");
        {
            let mut logins = auth.logins.lock().await;
            logins.insert(
                "state-caducado".to_owned(),
                PendingLogin {
                    nonce: "nonce-caducado".to_owned(),
                    issued_at: hace,
                },
            );
            assert_eq!(logins.len(), 1, "el mapa arranca con la entrada caducada");
        }

        auth.registrar_login("state-nuevo", "nonce-nuevo").await;

        let logins = auth.logins.lock().await;

        assert_eq!(
            logins.len(),
            1,
            "la entrada caducada debe purgarse al insertar: el mapa no puede crecer: {logins:?}"
        );
        assert!(
            !logins.contains_key("state-caducado"),
            "la entrada caducada sigue en el mapa: {logins:?}"
        );
        assert!(
            logins.contains_key("state-nuevo"),
            "la entrada nueva debe quedar registrada: {logins:?}"
        );
    }

    #[test]
    fn el_ttl_del_state_es_de_cinco_minutos() {
        // Fija el valor de la constante: la spec dice «más de 5 minutos» y el
        // callback rechaza por `expirado_en`, que compara contra `STATE_TTL`.
        assert_eq!(
            STATE_TTL,
            Duration::from_secs(300),
            "el TTL del state debe ser de 5 minutos"
        );
    }

    #[tokio::test]
    async fn en_desarrollo_no_hay_estado_de_autenticacion() {
        let config = Config::from_lookup(&lookup_from(&[("KESTO_MASTER_KEY", MASTER_KEY)]))
            .expect("la configuración de desarrollo es válida");

        // La guarda del directorio se mantiene viva en la prueba: la base que el estado
        // lleva dentro tiene que existir mientras el pool esté abierto.
        let (_directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(config, EstadoDeBase::nuevo(pool, clave));

        assert!(
            estado.auth.is_none(),
            "sin proveedor no hay capa que montar ni secreto que guardar"
        );
    }

    #[tokio::test]
    async fn con_oidc_el_estado_lleva_la_clave_y_no_la_publica() {
        let config = Config::from_lookup(&lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER_URL),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]))
        .expect("la configuración OIDC es válida");

        let (_directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(config, EstadoDeBase::nuevo(pool, clave));
        let auth = estado
            .auth
            .as_ref()
            .expect("con Oidc hay capa de autenticación");

        assert_eq!(auth.secret.expose(), SESSION_SECRET);

        // El estado acaba en un `Debug` en cuanto alguien registre `?state`, y
        // la clave de firma no puede salir por ahí.
        let en_debug = format!("{auth:?}");

        assert!(
            !en_debug.contains(SESSION_SECRET),
            "el Debug del estado publica la clave de firma: {en_debug}"
        );
    }

    /// 1.1 — El estado lleva el pool y las cuatro capas de acceso.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «El estado lleva el pool y las
    /// cuatro capas»): el estado que comparten los manejadores lleva el pool migrado y
    /// las cuatro capas del §3, y las cuatro comparten **ese** pool y **esa** clave.
    ///
    /// Se comprueba por el camino que importa y no por la presencia de los campos: lo
    /// que escribe cada capa se relee con **SQL directo** sobre el fichero de la base
    /// —sin pasar por la API ni por el estado—, y el sobre que queda en la columna del
    /// secreto se descifra con la clave **del estado**. Si una capa se construyera
    /// sobre otro pool o con otra clave, esta prueba lo delata.
    #[tokio::test]
    async fn el_estado_lleva_el_pool_y_las_cuatro_capas() {
        /// Secreto de acceso distinguible y largo: si aparece en claro en la columna, el
        /// cifrado en reposo no ocurrió.
        const SECRETO: &str = "AKIAIOSFODNN7EXAMPLE-estado-1-1";
        /// Passphrase distinguible y larga, por el mismo motivo.
        const PASSPHRASE: &str = "passphrase-distinguible-000000000000000000000000001-1";

        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
        );

        let repositorio = estado
            .db
            .repositorios
            .alta(AltaDeRepositorio {
                name: "repo-1-1".to_owned(),
                endpoint: "https://s3.example.test/repo-1-1".to_owned(),
                bucket: "cubo-repo-1-1".to_owned(),
                region: "eu-west-1".to_owned(),
                access_key_id: "AKIAIOSFODNN7EXAMPLE".to_owned(),
                secret_access_key: SecretString::new(SECRETO.to_owned()),
                passphrase: SecretString::new(PASSPHRASE.to_owned()),
            })
            .await
            .expect("el alta por la capa del estado no puede fallar");

        // 1. La capa de repositorios escribe en **el** pool del estado: la fila está en
        //    el fichero con el que se abrió, y se ve con SQL directo.
        let nombre: String = sqlx::query_scalar("SELECT name FROM repositories WHERE id = ?1")
            .bind(repositorio.id.hyphenated().to_string())
            .fetch_one(&pool)
            .await
            .expect("la fila debe estar en el fichero de la base del estado");

        assert_eq!(
            nombre, "repo-1-1",
            "la capa de repositorios no escribió en el pool del estado"
        );

        // 2. Y con **la** clave del estado: el sobre almacenado se descifra con ella y da
        //    el valor que entró, así que no es otra clave.
        let guardado: String =
            sqlx::query_scalar("SELECT secret_access_key FROM repositories WHERE id = ?1")
                .bind(repositorio.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el sobre almacenado debe poder leerse");

        assert!(
            guardado.starts_with("v1:"),
            "el secreto no quedó cifrado en la columna: {guardado}"
        );
        assert_eq!(
            estado
                .db
                .clave
                .descifrar(&guardado)
                .expect("la clave del estado debe descifrar lo que escribió su propia capa")
                .expose(),
            SECRETO,
            "la capa de repositorios no cifró con la clave del estado"
        );

        // 3. Las otras tres capas están y comparten el mismo pool. El grafo entero del §3
        //    —target → repositorio → tarea → ejecución— se ata sobre las cuatro capas del
        //    estado, y cada fila se relee con SQL directo sobre ese mismo pool.
        let target = estado
            .db
            .targets
            .alta(AltaDeTarget {
                name: "target-1-1".to_owned(),
                target_type: TipoDeTarget::Directory,
                container_name: None,
                db_user: None,
                db_password: None,
                db_name: None,
                host_path: Some("/srv/target-1-1".to_owned()),
            })
            .await
            .expect("el alta del target por la capa del estado no puede fallar");

        let tarea = estado
            .db
            .tareas
            .alta(AltaDeTarea {
                target_id: target.id,
                repository_id: repositorio.id,
                cron_expression: "0 3 * * *".to_owned(),
                keep_daily: 7,
                keep_weekly: 4,
                keep_monthly: 12,
                enabled: true,
            })
            .await
            .expect("el alta de la tarea por la capa del estado no puede fallar");

        let ejecucion = estado
            .db
            .ejecuciones
            .alta(AltaDeEjecucion {
                job_id: tarea.id,
                status: Estado::Pending,
                snapshot_id: None,
                bytes_processed: 0,
                bytes_uploaded: 0,
                error_message: None,
                started_at: OffsetDateTime::now_utc(),
                finished_at: None,
            })
            .await
            .expect("el alta de la ejecución por la capa del estado no puede fallar");

        let targets_en_la_base: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets WHERE id = ?1")
                .bind(target.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el recuento de targets no puede fallar");

        assert_eq!(
            targets_en_la_base, 1,
            "la capa de targets no escribió en el pool del estado"
        );

        let tareas_en_la_base: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs WHERE id = ?1")
                .bind(tarea.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el recuento de tareas no puede fallar");

        assert_eq!(
            tareas_en_la_base, 1,
            "la capa de tareas no escribió en el pool del estado"
        );

        let ejecuciones_en_la_base: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM job_executions WHERE id = ?1")
                .bind(ejecucion.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el recuento de ejecuciones no puede fallar");

        assert_eq!(
            ejecuciones_en_la_base, 1,
            "la capa de ejecuciones no escribió en el pool del estado"
        );
    }
}
