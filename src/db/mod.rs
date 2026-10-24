//! Persistencia en SQLite: la apertura de la base, sus pragmas y su esquema.
//!
//! Este módulo es la raíz de la capa de datos: `abrir` construye el pool con las
//! opciones que SQLite necesita para comportarse como el §3 del SRS espera
//! —claves foráneas encendidas en **toda** conexión, WAL, una espera finita ante
//! una base ocupada— y [`migrate`] aplica el esquema embebido en el binario.
//!
//! Nada de aquí conoce las tablas: los tipos y las consultas de cada una viven en
//! la capa de acceso, que es otro módulo de este mismo directorio.

pub mod crypto;
pub mod migrate;

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use thiserror::Error;

use crate::config::Config;

/// Número máximo de conexiones del pool.
///
/// SQLite serializa las **escrituras**: una sola a la vez, con las demás
/// esperando. El pool no existe para escribir en paralelo —no se puede— sino
/// para que las lecturas del panel no se queden detrás de la escritura del motor,
/// que es justo lo que WAL permite. Un puñado de conexiones basta para eso y no
/// tiene sentido pedir más: cada conexión es un hilo propio dentro de `sqlx`.
pub const MAX_CONEXIONES: u32 = 5;

/// Espera máxima ante una base ocupada, antes de rendirse con un error.
///
/// SQLite **no espera** por defecto: una escritura que coincide con otra falla en
/// el acto con «base de datos ocupada». El valor es finito a propósito —esperar
/// sin tope convertiría una escritura atascada en una petición colgada— y holgado
/// a propósito: la escritura que se está cediendo puede ser un `CHECKPOINT` de
/// WAL, que con un fichero de tamaño normal tarda milisegundos, así que cinco
/// segundos no los alcanza nadie por casualidad.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Sufijo del fichero de escritura adelantada que WAL deja junto a la base.
///
/// Va aquí y no escrito a mano en cada sitio porque es el nombre que impone
/// SQLite, y quien compruebe que el journaling está en WAL va a necesitarlo.
pub const SUFIJO_WAL: &str = "-wal";

/// Error de la capa de base de datos.
///
/// Cada variante conserva su causa: el registro del servidor es el único sitio
/// donde el diagnóstico puede leerse entero, porque el cuerpo de la respuesta
/// lleva el mensaje genérico de los fallos internos.
#[derive(Debug, Error)]
pub enum DbError {
    /// El directorio de la base no existe.
    ///
    /// Se falla en alto en vez de crear el directorio: el del `compose` del §8
    /// **es** el directorio de la base, así que crearlo aquí escondería un
    /// volumen mal montado hasta que el fallo apareciera en otro sitio y por otro
    /// motivo. El mensaje nombra la ruta para que el diagnóstico sea «falta
    /// montar esto» y no «no arranca».
    #[error(
        "el directorio de la base de datos no existe: {} (revísalo: Kesto no lo crea)",
        .ruta.display()
    )]
    DirectorioAusente {
        /// Ruta del directorio que falta.
        ruta: PathBuf,
    },

    /// La ruta del directorio de la base existe, pero es un fichero.
    ///
    /// `exists()` es verdadero para un fichero regular, así que sin esta
    /// comprobación el fallo llegaría más tarde como un [`DbError::Apertura`]
    /// genérico con un `(code: 14)` crudo, sin decir que el problema es que el DSN
    /// apunta a algo que no es un directorio. El mensaje nombra la ruta.
    #[error(
        "la ruta del directorio de la base de datos no es un directorio: {} (hay un fichero donde \
         se esperaba un directorio)",
        .ruta.display()
    )]
    RutaNoEsDirectorio {
        /// Ruta que debería ser un directorio y no lo es.
        ruta: PathBuf,
    },

    /// El DSN no se puede leer como una cadena de conexión de SQLite.
    #[error("el DSN de la base de datos no se puede leer: {dsn}")]
    DsnIlegible {
        /// DSN tal y como llegó de la configuración.
        dsn: String,
        /// Motivo, tal y como lo informa `sqlx`.
        #[source]
        causa: sqlx::Error,
    },

    /// No se pudo abrir la base: fichero ilegible, directorio sin permiso de
    /// escritura —WAL necesita crear `-wal` y `-shm` junto a la base—, etc.
    #[error("no se pudo abrir la base de datos en {}: {causa}", .ruta.display())]
    Apertura {
        /// Ruta del fichero de la base.
        ruta: PathBuf,
        /// Motivo, tal y como lo informa `sqlx`.
        #[source]
        causa: sqlx::Error,
    },

    /// Las migraciones no se pudieron aplicar.
    ///
    /// Es un fallo de arranque, no un modo degradado: un esquema a medias es peor
    /// que un proceso que no arranca, porque el motor escribiría contra columnas
    /// que no existen y el síntoma sería una copia perdida.
    #[error("no se pudieron aplicar las migraciones: {causa}")]
    Migracion {
        /// Motivo, tal y como lo informa `sqlx`.
        #[source]
        causa: sqlx::migrate::MigrateError,
    },

    /// Una consulta de la capa de acceso falló.
    #[error("la consulta falló: {causa}")]
    Consulta {
        /// Motivo, tal y como lo informa `sqlx`.
        #[source]
        causa: sqlx::Error,
    },
}

/// Ruta del fichero de la base a partir del DSN.
///
/// El DSN del SRS es `sqlite://<ruta>` y, para una ruta absoluta, lleva **tres**
/// barras: las dos del esquema y la que abre la ruta. La lectura no se hace a
/// mano —contar barras es exactamente donde se cuela la de más— sino con el
/// propio lector de `sqlx`, que ya resuelve esa forma y además descodifica los
/// escapes `%` que la URL permite.
pub fn ruta_del_dsn(dsn: &str) -> Result<PathBuf, DbError> {
    let opciones = opciones_del_dsn(dsn)?;

    Ok(opciones.get_filename().to_path_buf())
}

/// Las opciones de conexión que el DSN pide, con los pragmas ya fijados.
///
/// Se construyen en un solo sitio y en un solo sitio se leen sus valores por
/// defecto: las opciones que la aplicación **exige** van declaradas aquí, no
/// heredadas de la biblioteca. `foreign_keys` está encendido por defecto en
/// `sqlx`, pero se declara igualmente: es la diferencia entre una regla y una
/// casualidad, y el día que ese defecto cambie, el `ON DELETE CASCADE` del §3
/// dejaría de ocurrir sin que nada lo delatara.
fn opciones_del_dsn(dsn: &str) -> Result<SqliteConnectOptions, DbError> {
    let opciones = SqliteConnectOptions::from_str(dsn).map_err(|causa| DbError::DsnIlegible {
        dsn: dsn.to_owned(),
        causa,
    })?;

    Ok(opciones
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(BUSY_TIMEOUT))
}

/// Comprueba que el directorio de la base existe, sin crearlo.
///
/// Se falla en alto en vez de crear el directorio: el del `compose` del §8 **es**
/// el directorio de la base, así que crearlo aquí escondería un volumen mal
/// montado. WAL deja además `-wal` y `-shm` junto a la base, así que el
/// directorio tiene que ser escribible de todas formas.
fn exigir_directorio(ruta: &Path) -> Result<(), DbError> {
    // Una ruta sin directorio —`kesto.db`— se resuelve contra el directorio de
    // trabajo, que existe por definición: no hay nada que comprobar.
    let Some(directorio) = ruta.parent().filter(|padre| !padre.as_os_str().is_empty()) else {
        return Ok(());
    };

    if directorio.is_dir() {
        return Ok(());
    }

    // `exists()` es verdadero también para un fichero regular: se distingue del
    // caso ausente para no devolver un `Apertura` genérico que no explique nada.
    if directorio.exists() {
        return Err(DbError::RutaNoEsDirectorio {
            ruta: directorio.to_path_buf(),
        });
    }

    Err(DbError::DirectorioAusente {
        ruta: directorio.to_path_buf(),
    })
}

/// Abre el pool de la base de datos del DSN dado.
///
/// El fichero se crea si no existe, pero **el directorio no**: si falta, esto
/// falla nombrando la ruta.
pub async fn abrir(dsn: &str) -> Result<SqlitePool, DbError> {
    // El DSN se lee **una** vez: de las mismas opciones salen la ruta que se
    // comprueba y la conexión que se abre.
    let opciones = opciones_del_dsn(dsn)?;
    let ruta = opciones.get_filename().to_path_buf();
    exigir_directorio(&ruta)?;

    let pool = SqlitePoolOptions::new()
        .max_connections(MAX_CONEXIONES)
        .connect_with(opciones)
        .await
        .map_err(|causa| DbError::Apertura {
            ruta: ruta.clone(),
            causa,
        })?;

    // Se fuerza la apertura real de una conexión: el pool es perezoso, así que un
    // fichero ilegible o un directorio sin permiso de escritura —WAL necesita
    // crear `-wal` y `-shm`— no aparecerían hasta la primera consulta. Aquí, en
    // cambio, salen como error de apertura y el mensaje nombra la ruta.
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .map_err(|causa| DbError::Apertura { ruta, causa })?;

    Ok(pool)
}

/// Abre la base de la configuración y aplica el esquema embebido.
///
/// Es la primera cosa que hace el arranque y su fallo es error de arranque: no
/// hay proceso sirviendo contra un esquema a medias.
pub async fn preparar(config: &Config) -> Result<SqlitePool, DbError> {
    preparar_con(config, &migrate::migrador()).await
}

/// Abre la base de la configuración y aplica el migrador **dado**.
///
/// Existe para poder ejercitar el camino de fallo de [`preparar`] con un migrador
/// que sí falla: el embebido está en verde por construcción, así que un escenario
/// que exija «una migración que falla impide arrancar» necesita inyectarlo.
pub async fn preparar_con(config: &Config, migrator: &Migrator) -> Result<SqlitePool, DbError> {
    let pool = abrir(&config.database_url).await?;

    migrate::aplicar(&pool, migrator).await?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use sqlx::Row;
    use sqlx::SqlitePool;

    use super::{BUSY_TIMEOUT, DbError, SUFIJO_WAL, abrir, preparar, preparar_con, ruta_del_dsn};
    use crate::test_support::{config_con_dsn, directorio_de_prueba, migrador_roto};

    /// Tablas declaradas en la base, por nombre y en orden alfabético.
    async fn tablas(pool: &SqlitePool) -> Vec<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
        )
        .fetch_all(pool)
        .await
        .expect("la consulta de tablas no puede fallar")
    }

    /// Volcado del esquema: es lo que se compara entre dos arranques.
    async fn esquema(pool: &SqlitePool) -> Vec<String> {
        sqlx::query("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
            .fetch_all(pool)
            .await
            .expect("la consulta del esquema no puede fallar")
            .iter()
            .map(|fila| {
                let tipo: String = fila.try_get("type").expect("type es texto");
                let nombre: String = fila.try_get("name").expect("name es texto");
                let sql: Option<String> = fila.try_get("sql").expect("sql es texto o nulo");

                format!("{tipo}|{nombre}|{}", sql.unwrap_or_default())
            })
            .collect()
    }

    /// Migraciones registradas en la tabla de control.
    async fn migraciones_registradas(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(pool)
            .await
            .expect("la tabla de control de migraciones debe existir")
    }

    /// 1.1 — Un fichero de base que no existe se crea al abrir.
    #[tokio::test]
    async fn el_fichero_de_la_base_se_crea_al_abrir() {
        let directorio = directorio_de_prueba();
        let ruta = directorio.ruta_de_la_base();

        assert!(
            !ruta.exists(),
            "el aparato de prueba no debe crear el fichero por su cuenta"
        );

        let pool = abrir(&directorio.dsn())
            .await
            .expect("la base debe abrirse en un directorio que existe");

        assert!(
            ruta.exists(),
            "abrir una base que no existe debe crear el fichero en {}",
            ruta.display()
        );

        pool.close().await;
    }

    /// 1.2 — Un directorio inexistente detiene la apertura nombrando la ruta.
    #[tokio::test]
    async fn un_directorio_inexistente_hace_fallar_la_apertura_nombrando_la_ruta() {
        let directorio = directorio_de_prueba();
        let ausente = directorio.ruta().join("no-existe");
        let dsn = format!("sqlite://{}/kesto.db", ausente.display());

        let error = abrir(&dsn)
            .await
            .expect_err("un directorio ausente no puede dar una base abierta");

        assert!(
            matches!(error, DbError::DirectorioAusente { .. }),
            "el fallo debe ser el directorio ausente, no otro: {error}"
        );
        assert!(
            error.to_string().contains(&ausente.display().to_string()),
            "el mensaje debe nombrar la ruta del directorio que falta: {error}"
        );
        assert!(
            !ausente.exists(),
            "el directorio no debe crearse: {}",
            ausente.display()
        );
    }

    /// 1.3 — El DSN de tres barras da la ruta absoluta, sin una barra de más.
    #[test]
    fn el_dsn_de_tres_barras_da_la_ruta_absoluta_sin_barra_de_mas() {
        // El DSN por defecto del SRS, tal y como lo resuelve la configuración
        // cuando `DATABASE_URL` no está en el entorno.
        let config = crate::config::Config::from_lookup(&crate::test_support::lookup_from(&[(
            "KESTO_MASTER_KEY",
            crate::test_support::MASTER_KEY,
        )]))
        .expect("la configuración por defecto es válida");

        assert_eq!(
            config.database_url, "sqlite:///app/data/kesto.db",
            "el defecto del SRS no es el que esta prueba cree"
        );

        let ruta = ruta_del_dsn(&config.database_url).expect("el DSN del SRS debe leerse");

        assert_eq!(ruta, PathBuf::from("/app/data/kesto.db"));
        assert!(ruta.is_absolute(), "la ruta debe ser absoluta: {ruta:?}");
        assert_eq!(
            ruta.file_name().and_then(|nombre| nombre.to_str()),
            Some("kesto.db"),
            "el nombre del fichero no debe quedar con una barra de más: {ruta:?}"
        );

        // Y lo mismo con otra ruta absoluta, que no sea la del SRS.
        assert_eq!(
            ruta_del_dsn("sqlite:///tmp/x/y.db").expect("el DSN debe leerse"),
            PathBuf::from("/tmp/x/y.db")
        );
    }

    /// 1.4 — Tras migrar existen las cuatro tablas y la de control.
    #[tokio::test]
    async fn tras_migrar_existen_las_cuatro_tablas_y_las_de_control() {
        let directorio = directorio_de_prueba();
        let pool = preparar(&config_con_dsn(&directorio.dsn()))
            .await
            .expect("preparar debe abrir la base y dejar el esquema aplicado");

        let declaradas = tablas(&pool).await;

        for esperada in [
            "repositories",
            "backup_targets",
            "backup_jobs",
            "job_executions",
            "_sqlx_migrations",
        ] {
            assert!(
                declaradas.iter().any(|tabla| tabla == esperada),
                "falta la tabla {esperada}: {declaradas:?}"
            );
        }

        pool.close().await;
    }

    /// 1.5 — Arrancar dos veces no cambia el esquema.
    #[tokio::test]
    async fn arrancar_dos_veces_no_cambia_el_esquema() {
        let directorio = directorio_de_prueba();
        let config = config_con_dsn(&directorio.dsn());

        let pool = preparar(&config)
            .await
            .expect("el primer arranque debe dejar la base lista");
        let antes = esquema(&pool).await;
        let migraciones_antes = migraciones_registradas(&pool).await;
        pool.close().await;

        let pool = preparar(&config)
            .await
            .expect("el segundo arranque debe dejar la base lista");
        let despues = esquema(&pool).await;
        let migraciones_despues = migraciones_registradas(&pool).await;
        pool.close().await;

        assert_eq!(
            migraciones_antes, migraciones_despues,
            "el segundo arranque volvió a aplicar una migración ya aplicada"
        );

        assert!(
            migraciones_antes >= 2,
            "las dos migraciones deben quedar registradas: {migraciones_antes}"
        );

        assert_eq!(
            antes, despues,
            "el esquema cambió entre dos arranques sobre la misma base"
        );
    }

    /// 1.6 — Una migración que falla es error de arranque y no deja nada a medias.
    #[tokio::test]
    async fn una_migracion_que_falla_es_error_de_arranque() {
        let directorio = directorio_de_prueba();
        let config = config_con_dsn(&directorio.dsn());

        let error = preparar_con(&config, &migrador_roto())
            .await
            .expect_err("una migración que falla no puede dar un arranque en pie");

        assert!(
            matches!(error, DbError::Migracion { .. }),
            "el fallo debe llegar como error de migración: {error}"
        );

        // Y no queda registrada: el arranque falló entero, no a medias.
        let pool = abrir(&directorio.dsn())
            .await
            .expect("la base debe poder abrirse para comprobar el registro");
        let registradas = migraciones_registradas(&pool).await;
        pool.close().await;

        assert_eq!(
            registradas, 0,
            "una migración que falla no debe quedar registrada como aplicada"
        );
    }

    /// 1.7 — El journaling queda en WAL y el fichero `-wal` aparece al lado.
    #[tokio::test]
    async fn el_journaling_queda_en_wal_y_aparece_el_fichero_wal() {
        let directorio = directorio_de_prueba();
        let pool = preparar(&config_con_dsn(&directorio.dsn()))
            .await
            .expect("preparar debe dejar la base lista");

        let modo = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .expect("el modo de journaling debe poder consultarse");

        assert_eq!(
            modo.to_lowercase(),
            "wal",
            "el journaling debe quedar en WAL, no en {modo}"
        );

        let wal = PathBuf::from(format!(
            "{}{SUFIJO_WAL}",
            directorio.ruta_de_la_base().display()
        ));

        assert!(
            wal.exists(),
            "WAL deja su fichero junto a la base y no está: {}",
            wal.display()
        );

        pool.close().await;
    }

    /// 1.8 — Las claves foráneas están encendidas en **toda** conexión del pool.
    #[tokio::test]
    async fn las_claves_foraneas_estan_encendidas_en_toda_conexion_del_pool() {
        let directorio = directorio_de_prueba();
        let pool = preparar(&config_con_dsn(&directorio.dsn()))
            .await
            .expect("preparar debe dejar la base lista");

        // Se retienen varias a la vez: mientras están cogidas, el pool no puede
        // entregar la misma dos veces, así que son conexiones distintas de verdad.
        let mut conexiones = Vec::new();

        for _ in 0..3 {
            conexiones.push(
                pool.acquire()
                    .await
                    .expect("el pool debe entregar una conexión"),
            );
        }

        assert!(
            pool.size() >= 3,
            "el pool debe tener las tres conexiones abiertas a la vez, y tiene {}",
            pool.size()
        );

        for conexion in &mut conexiones {
            let encendidas = sqlx::query_scalar::<_, bool>("PRAGMA foreign_keys")
                .fetch_one(&mut **conexion)
                .await
                .expect("el estado de las claves foráneas debe poder consultarse");

            assert!(
                encendidas,
                "una conexión del pool tiene las claves foráneas apagadas: \
                 el PRAGMA suelto solo afecta a la conexión que lo ejecuta"
            );
        }

        // Se devuelven las conexiones **antes** de cerrar: `close` espera a que
        // todas vuelvan, así que retenerlas aquí lo dejaría esperando para siempre.
        drop(conexiones);

        pool.close().await;
    }

    /// 1.9 — Sin permiso de escritura en el directorio, la apertura falla.
    ///
    /// `chmod 0o555` **no** impide escribir a `root`: en un contenedor con uid 0 la
    /// apertura tendría éxito, `expect_err` entraría en pánico y el test caería
    /// **siempre**. No es una carrera, es dependencia del entorno, así que se tantea
    /// el directorio antes de medir: si sigue siendo escribible, no hay nada que
    /// medir y el test sale sin fallar.
    #[cfg(unix)]
    #[tokio::test]
    async fn un_directorio_sin_permiso_de_escritura_hace_fallar_la_apertura() {
        use std::os::unix::fs::PermissionsExt;

        let directorio = directorio_de_prueba();
        let solo_lectura = directorio.ruta().join("solo-lectura");
        std::fs::create_dir(&solo_lectura).expect("el directorio de la prueba debe crearse");
        std::fs::set_permissions(&solo_lectura, std::fs::Permissions::from_mode(0o555))
            .expect("los permisos del directorio deben poder cambiarse");

        // El tanteo, sin `unsafe`: si un `File::create` dentro del directorio en
        // solo lectura tiene éxito, quien corre la suite es `root` o equivalente
        // —`CAP_DAC_OVERRIDE`— y los permisos no miden nada.
        if std::fs::File::create(solo_lectura.join("tanteo")).is_ok() {
            std::fs::set_permissions(&solo_lectura, std::fs::Permissions::from_mode(0o755))
                .expect("los permisos del directorio deben poder devolverse");

            eprintln!(
                "se omite la aserción de permisos del test 1.9: el directorio en solo lectura \
                 sigue siendo escribible —la suite corre como root o equivalente—, así que los \
                 permisos no impiden la escritura y no hay nada que medir"
            );

            return;
        }

        let resultado = abrir(&format!("sqlite://{}/kesto.db", solo_lectura.display())).await;

        // Los permisos se devuelven antes de cualquier aserción: con el
        // directorio en solo lectura, ni el `Drop` ni la limpieza podrían tocarlo.
        std::fs::set_permissions(&solo_lectura, std::fs::Permissions::from_mode(0o755))
            .expect("los permisos del directorio deben poder devolverse");

        let error = resultado
            .expect_err("WAL necesita crear ficheros junto a la base: sin escritura, no hay base");

        assert!(
            error
                .to_string()
                .contains(&solo_lectura.display().to_string()),
            "el mensaje debe nombrar la ruta de la base: {error}"
        );
    }

    /// Un fichero donde el DSN espera un directorio falla nombrando la ruta.
    ///
    /// `exists()` es verdadero para un fichero regular, así que sin la variante
    /// propia el fallo llegaría como un `Apertura` genérico con un `(code: 14)`
    /// crudo: esto es lo que fija que el diagnóstico nombre el problema.
    #[tokio::test]
    async fn un_fichero_donde_se_espera_un_directorio_falla_nombrando_la_ruta() {
        let directorio = directorio_de_prueba();
        let fichero = directorio.ruta().join("no-soy-un-directorio");
        std::fs::write(&fichero, b"contenido").expect("el fichero de la prueba debe crearse");

        let dsn = format!("sqlite://{}/kesto.db", fichero.display());

        let error = abrir(&dsn)
            .await
            .expect_err("un fichero donde se espera un directorio no puede dar una base abierta");

        assert!(
            matches!(error, DbError::RutaNoEsDirectorio { .. }),
            "el fallo debe ser «no es un directorio», no otro: {error}"
        );
        assert!(
            error.to_string().contains(&fichero.display().to_string()),
            "el mensaje debe nombrar la ruta que no es un directorio: {error}"
        );

        // Y el fichero sigue siendo un fichero: no se ha convertido en nada.
        assert!(
            fichero.is_file(),
            "la ruta debe seguir siendo un fichero: {}",
            fichero.display()
        );
    }

    /// 1.10 — Una escritura que coincide con otra espera, no falla en el acto.
    ///
    /// Determinista: el bloqueo **no** se suelta por reloj, sino por una señal que el
    /// test dispara después de comprobar que la segunda escritura sigue pendiente.
    /// Así la espera observada es un límite inferior real y no depende de que el
    /// hilo principal no se retrase —con el `sleep` viejo, un retraso de más de la
    /// ventana soltaba el candado antes de medir y el test caía bajo carga—.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn una_escritura_que_coincide_con_otra_espera_en_vez_de_fallar() {
        // Ventana en la que se comprueba que la escritura sigue pendiente.
        const VENTANA_PENDIENTE: Duration = Duration::from_millis(200);

        let directorio = directorio_de_prueba();
        let pool = preparar(&config_con_dsn(&directorio.dsn()))
            .await
            .expect("preparar debe dejar la base lista");

        sqlx::query("CREATE TABLE prueba_de_bloqueo (x INTEGER)")
            .execute(&pool)
            .await
            .expect("la tabla de la prueba debe crearse fuera de la transacción");

        // Quien bloquea, en su tarea: coge el bloqueo de escritura, avisa de que lo
        // tiene, y **espera una señal** para soltarlo. Nada de `sleep`: el bloqueo
        // no se suelta por reloj.
        let (aviso, espera) = tokio::sync::oneshot::channel::<()>();
        let (soltar, espera_suelta) = tokio::sync::oneshot::channel::<()>();
        let pool_bloqueante = pool.clone();
        let bloqueo = tokio::spawn(async move {
            let mut conexion = pool_bloqueante
                .acquire()
                .await
                .expect("el pool debe entregar la conexión que bloquea");

            sqlx::query("BEGIN IMMEDIATE")
                .execute(&mut *conexion)
                .await
                .expect("el bloqueo de escritura debe poder tomarse");
            sqlx::query("INSERT INTO prueba_de_bloqueo (x) VALUES (0)")
                .execute(&mut *conexion)
                .await
                .expect("la primera escritura debe poder hacerse");

            aviso.send(()).expect("el test debe seguir esperando");

            // Se suelta cuando el test lo manda, no cuando pasa el tiempo.
            espera_suelta.await.expect("el test debe soltar el bloqueo");

            sqlx::query("COMMIT")
                .execute(&mut *conexion)
                .await
                .expect("el bloqueo debe poder soltarse");
        });

        espera.await.expect("el bloqueo debe establecerse");

        // La segunda escritura, en su propia tarea, para poder observarla pendiente.
        let pool_segunda = pool.clone();
        let mut segunda = tokio::spawn(async move {
            sqlx::query("INSERT INTO prueba_de_bloqueo (x) VALUES (1)")
                .execute(&pool_segunda)
                .await
        });

        let inicio = Instant::now();

        // Mientras el bloqueo sigue cogido, la escritura no puede completarse: que
        // este `timeout` expire es la prueba de que seguía pendiente, y no al revés.
        let pendiente = tokio::time::timeout(VENTANA_PENDIENTE, &mut segunda).await;
        assert!(
            pendiente.is_err(),
            "la segunda escritura terminó pese al bloqueo: no esperó nada"
        );

        // Ya con constancia de que seguía pendiente, se suelta el bloqueo.
        soltar.send(()).expect("el bloqueo debe poder soltarse");

        let resultado = segunda
            .await
            .expect("la tarea de la segunda escritura no debe entrar en pánico");
        let transcurrido = inicio.elapsed();

        bloqueo
            .await
            .expect("la tarea que bloquea no debe entrar en pánico");

        resultado.expect(
            "la segunda escritura debe esperar al bloqueo y completarse, \
             no rendirse antes de agotar el `busy_timeout`",
        );

        // El límite inferior es real: el `timeout` probó que la escritura seguía
        // pendiente al cabo de `VENTANA_PENDIENTE`, y hasta que no se soltó el
        // bloqueo no pudo completarse.
        assert!(
            transcurrido >= VENTANA_PENDIENTE,
            "la segunda escritura no esperó al bloqueo: {transcurrido:?}"
        );
        assert!(
            transcurrido < BUSY_TIMEOUT,
            "la segunda escritura esperó más que el propio `busy_timeout`: {transcurrido:?}"
        );

        pool.close().await;
    }
}
