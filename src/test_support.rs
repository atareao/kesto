//! Utilidades que comparten las pruebas: lectura inyectada y arnés del router.
//!
//! Las peticiones se lanzan con `tower::ServiceExt::oneshot` sobre el router, sin
//! abrir un socket de **escucha**. Cuidado: eso describe al arnés, no a todo el
//! camino —el login (`/api/v1/auth/login`) sí abre un socket **saliente** a
//! loopback, el del proveedor falso de [`proveedor_falso`], que es un servidor
//! HTTP de verdad en `127.0.0.1`—. **Ninguna prueba sale a internet.**

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderName, Method, Request, header};
use axum::response::Response;
use bollard::container::{CreateContainerOptions, RemoveContainerOptions, StartContainerOptions};
use http_body_util::BodyExt;
use rustic_backend::local::LocalBackend;
use rustic_core::repofile::{Node, SnapshotFile, SnapshotId};
use rustic_core::{
    BackupOptions, ConfigOptions, Credentials, Excludes, KeyOptions, LocalSource,
    LocalSourceFilterOptions, LocalSourceSaveOptions, OpenStatus, Repository, RepositoryBackends,
    RepositoryOptions, SnapshotOptions, WriteBackend,
    jiff::{Timestamp, Zoned, tz::TimeZone},
};
use sqlx::SqlitePool;
use sqlx::migrate::{Migration, MigrationType, Migrator};
use tower::ServiceExt;
use uuid::Uuid;

use crate::config::{Config, Lookup};
use crate::db::crypto::Clave;
use crate::docker::Cliente;
use crate::engine::{Ajustes, Dependencias, Motor};
use crate::rustic::BarrasDelMotor;
use crate::secret::SecretString;
use crate::state::{AppState, AuthState, EstadoDeBase};

pub(crate) mod proveedor_falso;

/// Clave maestra de las pruebas: larga, para que ningún aviso se cuele.
pub(crate) const MASTER_KEY: &str =
    "clave-maestra-distinguible-000000000000000000000000000000000000";
/// Clave de firma de la sesión de las pruebas, con la longitud de sobra.
pub(crate) const SESSION_SECRET: &str =
    "clave-de-firma-distinguible-000000000000000000000000000000000000";
/// Otra clave de firma, distinguible: es la que no vale.
pub(crate) const OTRA_SESSION_SECRET: &str =
    "otra-clave-de-firma-000000000000000000000000000000000000000000";
/// Emisor del proveedor de pruebas.
pub(crate) const OIDC_ISSUER_URL: &str = "https://idp.example.test/realms/kesto";
/// Identificador público del cliente de pruebas.
pub(crate) const OIDC_CLIENT_ID: &str = "kesto-panel";
/// Secreto del cliente OIDC: solo tiene que estar presente.
pub(crate) const OIDC_CLIENT_SECRET: &str = "secreto-de-cliente-oidc-distinguible";
/// URL de retorno por defecto: la del SRS, la que se usa si no se declara otra.
pub(crate) const OIDC_REDIRECT_URL: &str = "http://localhost:8080/api/v1/auth/callback";
/// Usuario distinguible de la sesión de pruebas.
pub(crate) const SUB: &str = "usuario-distinguible-1";
/// Correo distinguible de la sesión de pruebas.
pub(crate) const EMAIL: &str = "operador-distinguible@example.test";
/// Dirección de escucha de las pruebas: un puerto efímero, **nunca** el 8080.
///
/// `crate::serve` abre un socket de verdad, así que un estado de prueba que
/// herede el puerto por defecto pone la suite roja por `AddrInUse` en cuanto
/// algo ocupe el 8080 —y el propio Kesto en desarrollo lo ocupa—: un fallo del
/// entorno disfrazado de fallo del código. Con el puerto `0` lo elige el
/// sistema, y no hay que acordarse test a test.
pub(crate) const BIND_ADDRESS: &str = "127.0.0.1:0";

/// Construye una lectura de entorno inyectada a partir de pares nombre/valor.
///
/// Lo que no esté en `pares` se comporta como ausente, sin tocar el entorno del
/// proceso.
pub(crate) fn lookup_from(pares: &[(&str, &str)]) -> impl Lookup + use<> {
    let variables: HashMap<String, String> = pares
        .iter()
        .map(|(nombre, valor)| ((*nombre).to_owned(), (*valor).to_owned()))
        .collect();

    move |nombre: &str| variables.get(nombre).cloned()
}

/// Nombre del fichero de la base en los directorios de prueba.
pub(crate) const FICHERO_DE_LA_BASE: &str = "kesto.db";

/// Directorio temporal de una prueba, borrado al soltarse.
///
/// Cada prueba de la capa de datos abre **su** base y no una compartida: la suite
/// corre en paralelo, y una base común convertiría a un test que bloquea en el
/// fallo de otro. El nombre lleva un UUID para que dos pruebas que arranquen a la
/// vez no se pisen el directorio.
pub(crate) struct DirectorioDePrueba {
    ruta: PathBuf,
}

impl DirectorioDePrueba {
    /// Ruta del directorio.
    pub(crate) fn ruta(&self) -> &Path {
        &self.ruta
    }

    /// Ruta del fichero de la base dentro del directorio.
    pub(crate) fn ruta_de_la_base(&self) -> PathBuf {
        self.ruta.join(FICHERO_DE_LA_BASE)
    }

    /// DSN de la base, con la forma de tres barras del SRS.
    ///
    /// La ruta es absoluta, así que las dos barras del esquema más la suya dan
    /// las tres de `sqlite:///…` sin que nadie tenga que contar ninguna.
    pub(crate) fn dsn(&self) -> String {
        format!("sqlite://{}", self.ruta_de_la_base().display())
    }
}

impl Drop for DirectorioDePrueba {
    fn drop(&mut self) {
        // Limpiar es cortesía, no parte de lo que se afirma: si el borrado falla
        // —una prueba dejó el directorio en solo lectura— no hay nada que
        // comprobar sobre ello, y fallar aquí taparía el resultado real del test.
        let _ = std::fs::remove_dir_all(&self.ruta);
    }
}

/// Crea el directorio temporal de una prueba.
pub(crate) fn directorio_de_prueba() -> DirectorioDePrueba {
    let ruta = std::env::temp_dir().join(format!("kesto-prueba-{}", Uuid::new_v4()));

    std::fs::create_dir_all(&ruta).expect("el directorio temporal de la prueba debe crearse");

    DirectorioDePrueba { ruta }
}

// ── Repositorios Rustic de prueba ────────────────────────────────────────────
//
// Las pruebas de `src/rustic` hablan con un repositorio **local** —un directorio del
// sistema de ficheros— y no con S3: `rustic_backend::local::LocalBackend` no está detrás
// de ninguna feature, así que estas pruebas corren en el CI sin cubo, sin credenciales y
// sin red. Lo que la API pública del módulo construye es el backend de S3; lo que se
// prueba con esto es todo lo demás.
//
// Y se prueban las dos capas: la del módulo —con [`acceso_local`], [`backends_locales`] y
// [`barras_mudas`], que es lo que `poner_en_marcha_con` recibe— y la de la biblioteca,
// con [`abrir_local`] y las lecturas de [`snapshot_guardado`] y [`volcado_guardado`]. La
// segunda es lo que permite **comprobar** lo que la primera guardó: restaurar un volcado
// es leerlo del repositorio con la biblioteca, que es exactamente lo que hace un operador
// que recupera una copia, y por eso una copia que solo pasa por el módulo no está probada.

/// Passphrase de los repositorios locales de prueba.
///
/// Larga a propósito, como las del arranque: el aviso de credencial corta se dispara
/// antes de los 32 caracteres y una prueba no debe leer avisos por su culpa.
pub(crate) const PASSPHRASE_DE_PRUEBA: &str =
    "passphrase-de-prueba-000000000000000000000000000000000000";

/// La **otra** passphrase: la que no abre el repositorio de prueba.
///
/// Existe como constante y no como literal suelto porque la prueba que la usa tiene que
/// comprobar además que **no** aparece en el motivo del error: con el literal repetido,
/// esa comprobación podría pasar mirando la cadena equivocada.
pub(crate) const OTRA_PASSPHRASE_DE_PRUEBA: &str =
    "otra-passphrase-de-prueba-0000000000000000000000000000000000";

/// Clave secreta del acceso de prueba.
///
/// Es distinguible y **no vale para nada**: con el backend local no hay firma de nadie,
/// así que lo único que se comprueba de ella es que no sale publicada en ningún formato.
pub(crate) const CLAVE_SECRETA_DE_PRUEBA: &str =
    "clave-secreta-de-prueba-distinguible-000000000000000000000000000000000000";

/// Etiqueta con la que se siembran los snapshots de prueba.
pub(crate) const ETIQUETA_SEMBRADA: &str = "tarea-sembrada-de-prueba";

/// Etiqueta de la instantánea **ajena**: la que no creó Kesto.
///
/// Distinta de [`ETIQUETA_SEMBRADA`] a propósito, y no un detalle: el criterio con el que la
/// biblioteca agrupa las instantáneas para retener incluye la etiqueta, así que una
/// instantánea con otra etiqueta cae en otro grupo —que es exactamente lo que la hace
/// ajena—. Ver [`sembrar_snapshot_ajeno`].
pub(crate) const ETIQUETA_AJENA: &str = "instantanea-de-otra-herramienta";

/// El host de la instantánea ajena: otro de los tres campos del grupo de retención.
pub(crate) const HOST_AJENO: &str = "host-ajeno";

/// El fichero que deja dentro del árbol la instantánea ajena.
pub(crate) const AJENA: &str = "paisaje.txt";

/// El contenido de ese fichero, distinguible del de la semilla.
pub(crate) const AJENA_CONTENIDO: &[u8] = b"contenido-de-la-instantanea-ajena\n";

/// El fichero que la semilla deja dentro del árbol.
pub(crate) const SEMILLA: &str = "semilla.txt";

/// El contenido de ese fichero, para poder afirmar que la restauración lo devuelve.
pub(crate) const SEMILLA_CONTENIDO: &[u8] = b"contenido-de-la-semilla\n";

/// Acceso a un repositorio **local** de prueba.
///
/// El [`Acceso`](crate::rustic::Acceso) describe un repositorio de S3 —es lo que el §3
/// guarda y lo que la API pública construye—, así que los campos que solo tienen sentido
/// para S3 se rellenan con valores de mentira. Los dos que sí se usan son la `passphrase`
/// —la clave con la que se abre el repositorio— y el `endpoint`, que aquí lleva **la
/// ruta** del directorio y es, por eso, el nombre con el que los motivos de error nombran
/// el destino: una prueba tiene que poder comprobar que el motivo señala el sitio del que
/// habla.
pub(crate) fn acceso_local(ruta: &Path, passphrase: &str) -> crate::rustic::Acceso {
    crate::rustic::Acceso {
        endpoint: ruta.to_string_lossy().into_owned(),
        bucket: "cubo-de-prueba".to_owned(),
        region: "region-de-prueba".to_owned(),
        access_key_id: "clave-de-acceso-de-prueba".to_owned(),
        secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
        passphrase: SecretString::new(passphrase.to_owned()),
    }
}

/// Los backends de un repositorio **local** sobre el directorio dado.
///
/// Es la mitad de la puesta en marcha que en producción construye el backend de S3: la
/// función que los recibe ya construidos existe justo para que las pruebas puedan
/// entregar éstos.
pub(crate) fn backends_locales(ruta: &Path) -> RepositoryBackends {
    let backend: Arc<dyn WriteBackend> = Arc::new(
        LocalBackend::new(ruta.to_string_lossy().as_ref(), Vec::new())
            .expect("el backend local del arnés se construye sobre cualquier ruta"),
    );

    RepositoryBackends::new(backend, None)
}

/// Barras del motor que no hacen nada con los avisos.
///
/// El progreso tiene sus propias pruebas en `src/rustic/progreso.rs`: aquí estorba, pero
/// no se puede omitir porque la biblioteca las pide en cada operación. Es el mismo
/// [`BarrasDelMotor::mudas`](crate::rustic::BarrasDelMotor::mudas) que usan las dos operaciones de
/// la fachada del motor que no son una copia —abrirlo y listarlo—, y se dice para que no parezcan
/// dos cosas distintas.
pub(crate) fn barras_mudas() -> BarrasDelMotor {
    BarrasDelMotor::mudas()
}

/// El abridor de repositorios del motor, para las pruebas: abre un repositorio **local**.
///
/// El motor abre su repositorio con un puntero a función que entra por los ajustes, y sin este
/// gancho sus pruebas necesitarían un S3 de verdad, que no hay: con él, el ciclo de vida completo
/// —cola, hilo, archivado, fila— se recorre contra un directorio del sistema de ficheros, sin red,
/// sin credenciales y sin motor de contenedores, que es lo que permite que corra en el CI.
///
/// La ruta del repositorio sale del `endpoint` del [`Acceso`](crate::rustic::Acceso): es lo que
/// [`acceso_local`] deja ahí, y por eso un aparato de prueba solo tiene que escribir esa ruta en la
/// fila del §3 para que el motor abra **ese** repositorio. Y es un `fn` y no un cierre porque el
/// tipo que el motor acepta es un puntero a función: lo que se inyecta no puede llevar dentro ni
/// una credencial ni una ruta escondida.
pub(crate) fn abridor_local(
    acceso: &crate::rustic::Acceso,
    barras: BarrasDelMotor,
) -> Result<(crate::rustic::Repositorio, crate::rustic::PuestaEnMarcha), crate::rustic::Error> {
    crate::rustic::poner_en_marcha_con(
        acceso,
        backends_locales(Path::new(&acceso.endpoint)),
        barras,
    )
}

/// Un destino de S3 que **rechaza** en el acto, sin salir de la máquina.
///
/// Devuelve el `endpoint` de un oyente local que contesta `403` —con un error de S3 dentro y
/// cerrando la conexión— a lo que le pidan. Es lo que hace falta para ejercitar el camino de
/// **S3** —el único que entra en pánico si se llama a la biblioteca desde dentro de un runtime, y
/// el que ninguna prueba automática tocaba— sin red y sin contenedores.
///
/// # Por qué un oyente y no un puerto donde no escucha nadie
///
/// Está medido, y es lo que decide la forma: contra un puerto muerto (`http://127.0.0.1:9`) la
/// biblioteca tarda **entre 41 y 54 segundos** en decirlo, porque `rustic_backend` reintenta cinco
/// veces con retroceso exponencial y temblor (`DEFAULT_RETRY = 5`); contra este oyente tarda
/// **825 µs**, porque `opendal` clasifica el `403` como un fallo permanente y no lo reintenta. Lo
/// que se mide en estas pruebas es el **desenlace** —que no hay pánico y que el error llega
/// depurado—, no el reintento, y ese precio no lo debe pagar cada `cargo test`.
///
/// El oyente se queda escuchando hasta que el proceso termina, como el de [`emisor_caido`]: no hay
/// forma de pararlo desde fuera y tampoco hace falta.
pub(crate) fn destino_de_s3_que_rechaza() -> String {
    let oyente = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("el oyente local de la prueba se abre en un puerto libre");
    let puerto = oyente
        .local_addr()
        .expect("el oyente local tiene dirección")
        .port();

    std::thread::spawn(move || {
        for conexion in oyente.incoming() {
            let Ok(mut flujo) = conexion else { break };

            // Se lee lo que mande el cliente —la petición entera cabe de sobra— y se contesta
            // igual a todo: lo que la prueba necesita es un rechazo inmediato, no un S3.
            let mut peticion = [0_u8; 4096];
            let _ = std::io::Read::read(&mut flujo, &mut peticion);

            let cuerpo = r#"<?xml version="1.0" encoding="UTF-8"?><Error><Code>AccessDenied</Code><Message>Access Denied</Message><RequestId>prueba</RequestId></Error>"#;
            let respuesta = format!(
                "HTTP/1.1 403 Forbidden\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{cuerpo}",
                cuerpo.len()
            );

            let _ = std::io::Write::write_all(&mut flujo, respuesta.as_bytes());
        }
    });

    format!("http://127.0.0.1:{puerto}")
}

/// La ruta de destino de un repositorio de prueba, dentro de un directorio temporal **retenido**.
///
/// El alta de un repositorio dejó de ser un apunte: desde el change `backup-engine` **habla con el
/// destino** —lo inicializa o conecta con él—, así que el `endpoint` de una fila de prueba tiene que
/// ser una ruta de verdad donde [`abridor_local`] pueda abrir un repositorio. Un `endpoint` de
/// mentira (`https://s3.example.test/…`) haría fallar el alta por destino inalcanzable, que es
/// justamente lo que ese camino tiene que hacer.
///
/// La guarda del directorio se **retiene** hasta que el proceso de pruebas termina, y ése es el
/// motivo de que este ayudante exista en vez de que cada prueba se monte el suyo: el alta ocurre
/// **después** de construir el cuerpo de la petición, así que soltar la guarda al volver de aquí
/// borraría la ruta antes de que nadie la abriera. Es el mismo trato que [`GUARDAS_DEL_ARNES`], y la
/// misma deuda declarada: el directorio no se borra durante la corrida.
pub(crate) fn destino_local_de_prueba(nombre: &str) -> String {
    let directorio = directorio_de_prueba();
    let ruta = directorio.ruta().join(nombre);

    retener(directorio);

    ruta.to_string_lossy().into_owned()
}

/// Abre el repositorio local del directorio dado, con la biblioteca y sin pasar por el
/// módulo.
///
/// Es el camino del **lector**: un operador que recupera una copia no usa el motor de
/// Kesto, usa la biblioteca. Que las pruebas lean por aquí es lo que hace que lo que
/// comprueban sea el contenido del repositorio y no lo que el módulo dice haber escrito.
pub(crate) fn abrir_local(ruta: &Path, passphrase: &str) -> Repository<OpenStatus> {
    Repository::new(&RepositoryOptions::default(), &backends_locales(ruta))
        .expect("el repositorio local se construye")
        .open(&Credentials::Password(passphrase.to_owned()))
        .expect("el repositorio local de prueba se abre con su passphrase")
}

/// El snapshot que el repositorio guarda con ese identificador, leído del repositorio.
///
/// El identificador entra en **hex completo**, nunca en la forma corta de ocho caracteres
/// que da su `Display`: es un prefijo ambiguo y con dos snapshots que empiecen igual
/// resolvería el que no es.
pub(crate) fn snapshot_guardado(ruta: &Path, passphrase: &str, id: &str) -> SnapshotFile {
    let repositorio = abrir_local(ruta, passphrase);

    repositorio
        .get_snapshots(&[id])
        .expect("el snapshot del identificador dado se lee del repositorio")
        .into_iter()
        .next()
        .expect("el identificador tiene que resolver exactamente un snapshot")
}

/// El nodo del fichero `nombre` tal y como quedó en el snapshot `id`.
///
/// Publica el nodo entero y no solo sus bytes porque hay afirmaciones que son **sobre el
/// nodo**: que el del volcado declara tamaño cero, por ejemplo, que es la razón de que el
/// emparejamiento con la instantánea anterior tenga que desactivarse para un volcado. Con
/// [`volcado_guardado`] eso no se puede medir —los bytes estarían ahí igual—.
///
/// Entra en pánico si el snapshot no tiene ese nodo: quien quiera afirmar «no está» tiene
/// [`volcado_guardado`], que devuelve `Option` justo para eso.
pub(crate) fn nodo_del_snapshot(ruta: &Path, passphrase: &str, id: &str, nombre: &str) -> Node {
    let repositorio = abrir_local(ruta, passphrase);
    let snapshot = repositorio
        .get_snapshots(&[id])
        .expect("el snapshot del identificador dado se lee del repositorio")
        .into_iter()
        .next()
        .expect("el identificador tiene que resolver exactamente un snapshot");
    let indexado = repositorio
        .to_indexed()
        .expect("el índice del repositorio de prueba se lee");

    indexado
        .node_from_snapshot_and_path(&snapshot, nombre)
        .expect("el snapshot tiene que tener ese nodo dentro")
}

/// Los bytes del fichero `nombre` tal y como quedaron en el snapshot `id`.
///
/// Devuelve `None` cuando el snapshot **no tiene** ese fichero, y el `Option` no es
/// comodidad: «el volcado no está dentro» es un desenlace real y medido —el archivado de
/// la biblioteca descarta la entrada que falla y guarda el snapshot sin ella—, así que
/// tiene que poder afirmarse sin que la prueba confunda «no está» con «no se pudo leer».
pub(crate) fn volcado_guardado(
    ruta: &Path,
    passphrase: &str,
    id: &str,
    nombre: &str,
) -> Option<Vec<u8>> {
    let repositorio = abrir_local(ruta, passphrase);
    let snapshot = repositorio
        .get_snapshots(&[id])
        .expect("el snapshot del identificador dado se lee del repositorio")
        .into_iter()
        .next()
        .expect("el identificador tiene que resolver exactamente un snapshot");
    let indexado = repositorio
        .to_indexed()
        .expect("el índice del repositorio de prueba se lee");

    let Ok(nodo) = indexado.node_from_snapshot_and_path(&snapshot, nombre) else {
        return None;
    };

    let mut salida = Vec::new();
    indexado
        .dump(&nodo, &mut salida)
        .expect("un nodo de fichero se vuelca al buffer");

    Some(salida)
}

/// El instante de una fecha escrita en RFC 3339, con la zona de `UTC` colgada.
///
/// La fecha entra como texto porque es la forma en que una prueba la nombra; la conversión
/// pasa por el **instante** —que es lo que una fecha dice de verdad— y no por un análisis
/// de fecha civil con zona, que exigiría escribir la zona dentro del propio texto.
fn instante(fecha: &str) -> Zoned {
    fecha
        .parse::<Timestamp>()
        .expect("la fecha de la prueba es un instante en RFC 3339")
        .to_zoned(TimeZone::UTC)
}

/// Lo que dejó una siembra de instantáneas.
///
/// El camino del fichero dentro del árbol se devuelve porque **no se puede adivinar**: un
/// origen local archiva las rutas tal y como se las da el sistema de ficheros, así que el
/// árbol reproduce el camino de la fuente sin su barra inicial, y quien quiera restaurar el
/// fichero necesita ese camino. Devolverlo aquí es lo que hace que las pruebas de retención
/// comprueben que el contenido **sigue ahí** en vez de que la instantánea sigue listada.
pub(crate) struct Siembra {
    /// Los identificadores, en hex completo, en el orden de las fechas que se pidieron.
    pub identificadores: Vec<String>,
    /// El camino del fichero sembrado **dentro del árbol** de cada instantánea.
    pub nodo: String,
}

/// Deja en el destino un snapshot por cada fecha, todos con el **mismo árbol real**, y
/// devuelve sus identificadores en hex completo en el orden de `fechas`.
///
/// Se siembra con la **biblioteca** y no con el módulo por un motivo que decide la prueba:
/// el sello de tiempo de un snapshot es el del momento en que se archivó, y un módulo que
/// lo eligiera sería un módulo que miente sobre cuándo se copió. La retención, en cambio,
/// se decide por fechas, así que una prueba de retención necesita fechas que no dependan
/// del reloj: con tres días distintos, una regla diaria de uno solo puede conservar
/// exactamente uno, corra la prueba a la hora que corra.
///
/// El árbol es de verdad —se archiva un directorio de verdad— porque la purga lo recorre
/// para saber qué datos siguen referenciados: un árbol inventado haría que el `prune`
/// fallara por una razón que no tiene nada que ver con lo que se prueba.
///
/// El destino tiene que estar **vacío**: esto inicializa el repositorio.
pub(crate) fn sembrar_snapshots(ruta: &Path, passphrase: &str, fechas: &[&str]) -> Siembra {
    let backends = backends_locales(ruta);
    let credenciales = Credentials::Password(passphrase.to_owned());

    let repositorio = Repository::new(&RepositoryOptions::default(), &backends)
        .expect("el repositorio local se construye")
        .init(
            &credenciales,
            &KeyOptions::default(),
            &ConfigOptions::default(),
        )
        .expect("un destino vacío se inicializa");

    // La fuente del árbol, en **su** directorio temporal: al soltarse se borra, y para
    // entonces los datos ya están dentro del repositorio.
    let fuente = directorio_de_prueba();
    let fichero = fuente.ruta().join(SEMILLA);
    std::fs::write(&fichero, SEMILLA_CONTENIDO).expect("el fichero de la semilla se escribe");
    // El camino del nodo dentro del árbol: el de la fuente sin su barra inicial, que es
    // como lo guarda la biblioteca.
    let nodo = fichero.to_string_lossy().trim_start_matches('/').to_owned();

    let instantanea = SnapshotFile::from_options(
        &SnapshotOptions::default()
            .label(ETIQUETA_SEMBRADA.to_owned())
            .host("host-sembrado".to_owned())
            .command("comando-sembrado".to_owned()),
    )
    .expect("la instantánea sembrada se construye");
    let rutas = vec![fuente.ruta().to_path_buf()];
    let origen = LocalSource::new(
        LocalSourceSaveOptions::default(),
        &Excludes::default(),
        &LocalSourceFilterOptions::default(),
        &rutas,
    )
    .expect("la fuente local de la semilla se construye");

    let indexado = repositorio
        .to_indexed()
        .expect("el índice del repositorio recién creado se lee");
    let modelo = indexado
        .archive(&BackupOptions::default(), &origen, instantanea, &rutas)
        .expect("la semilla se archiva");

    // Las copias con fecha: mismo árbol, mismo resumen y misma etiqueta, otro sello de
    // tiempo. El identificador se deja en blanco porque lo calcula la biblioteca al
    // guardar —es el resumen del contenido, y el contenido cambia con la fecha—.
    let copias: Vec<SnapshotFile> = fechas
        .iter()
        .map(|fecha| {
            let mut copia = modelo.clone();
            copia.time = instante(fecha);
            copia.id = SnapshotId::default();
            copia
        })
        .collect();

    indexado
        .save_snapshots(copias)
        .expect("las copias sembradas se guardan");
    indexado
        .delete_snapshots(&[modelo.id])
        .expect("la instantánea que sirvió de modelo se retira");

    // Los identificadores, en el orden de `fechas`: el que la biblioteca calculó al
    // guardar no se puede predecir desde aquí, así que se leen del repositorio y se
    // emparejan por su fecha.
    let identificadores: Vec<String> = indexado
        .list::<SnapshotId>()
        .expect("el repositorio lista sus snapshots")
        .map(|id| id.to_hex().as_str().to_owned())
        .collect();
    let guardados = indexado
        .get_snapshots(&identificadores)
        .expect("los snapshots sembrados se leen del repositorio");

    let identificadores = fechas
        .iter()
        .map(|fecha| {
            let buscada = instante(fecha);

            guardados
                .iter()
                .find(|snapshot| snapshot.time == buscada)
                .expect("la fecha sembrada tiene que estar en el repositorio")
                .id
                .to_hex()
                .as_str()
                .to_owned()
        })
        .collect();

    Siembra {
        identificadores,
        nodo,
    }
}

/// Deja en el destino **una instantánea ajena**: otro host, otra etiqueta, otro comando y
/// otro árbol, con un fichero distinto dentro.
///
/// Existe porque el listado tiene que salir **del repositorio** y no del histórico de
/// ejecuciones de Kesto, y las instantáneas que siembra [`sembrar_snapshots`] no permiten
/// medirlo: las tres salen del mismo ayudante, así que un listado que dedujera el contenido
/// del histórico las devolvería igualmente. Esta no se parece a ninguna de ellas —ni el
/// host, ni la etiqueta, ni el comando, ni el camino del fichero, ni su contenido—, de modo
/// que si aparece en el listado es porque el listado leyó el repositorio.
///
/// Se archiva con la **biblioteca** y no con el módulo, que es lo que la hace ajena: el
/// módulo de Kesto no ha intervenido en escribirla.
///
/// El destino tiene que estar ya **inicializado** —esto abre, no inicializa—: se siembra
/// después de [`sembrar_snapshots`].
///
/// Devuelve el identificador en hex completo y el camino del fichero **dentro del árbol**,
/// por la misma razón que [`Siembra`]: no se puede adivinar.
pub(crate) fn sembrar_snapshot_ajeno(ruta: &Path, passphrase: &str) -> (String, String) {
    let repositorio = abrir_local(ruta, passphrase)
        .to_indexed_ids()
        .expect("el índice reducido del repositorio sembrado se lee");

    // La fuente, en su propio directorio temporal: al soltarse se borra, y para entonces los
    // datos ya están dentro del repositorio.
    let fuente = directorio_de_prueba();
    let fichero = fuente.ruta().join(AJENA);
    std::fs::write(&fichero, AJENA_CONTENIDO).expect("el fichero ajeno se escribe");
    let nodo = fichero.to_string_lossy().trim_start_matches('/').to_owned();

    let instantanea = SnapshotFile::from_options(
        &SnapshotOptions::default()
            .label(ETIQUETA_AJENA.to_owned())
            .host(HOST_AJENO.to_owned())
            .command("rustic -r otro-sitio backup /otro/sitio".to_owned()),
    )
    .expect("la instantánea ajena se construye");
    let rutas = vec![fuente.ruta().to_path_buf()];
    let origen = LocalSource::new(
        LocalSourceSaveOptions::default(),
        &Excludes::default(),
        &LocalSourceFilterOptions::default(),
        &rutas,
    )
    .expect("la fuente local de la instantánea ajena se construye");

    let guardada = repositorio
        .archive(&BackupOptions::default(), &origen, instantanea, &rutas)
        .expect("la instantánea ajena se archiva");

    (guardada.id.to_hex().as_str().to_owned(), nodo)
}

/// Configuración válida que apunta a una base de datos dada.
pub(crate) fn config_con_dsn(dsn: &str) -> Config {
    Config::from_lookup(&lookup_from(&[
        ("KESTO_MASTER_KEY", MASTER_KEY),
        ("BIND_ADDRESS", BIND_ADDRESS),
        ("DATABASE_URL", dsn),
    ]))
    .expect("la configuración de prueba con base de datos es válida")
}

/// Directorio temporal, pool con el esquema del §3 ya aplicado y la clave de cifrado.
///
/// Es el punto de partida que comparten las pruebas de la capa de acceso: base **de
/// verdad** —nada de dobles del driver, porque lo que se prueba es el comportamiento
/// de SQLite—, esquema aplicado por las migraciones embebidas y una `Clave` dentro de
/// un `Arc`, que es como la recibirá cada capa. El directorio viaja en el retorno para
/// que quien llame lo mantenga vivo: al soltarlo se borra, y con él la base.
pub(crate) async fn base_con_clave() -> (DirectorioDePrueba, SqlitePool, Arc<Clave>) {
    let directorio = directorio_de_prueba();

    let pool = crate::db::preparar(&config_con_dsn(&directorio.dsn()))
        .await
        .expect("el esquema del §3 debe aplicarse sobre la base de prueba");

    let clave = Arc::new(Clave::desde_maestra(&SecretString::new(
        MASTER_KEY.to_owned(),
    )));

    (directorio, pool, clave)
}

/// Guardas de los directorios de base que el arnés abre y **retiene** hasta que el
/// proceso de pruebas termina.
///
/// [`DirectorioDePrueba`] borra su directorio al soltarse, y el pool que un ayudante
/// entrega dentro del estado lo **sobrevive**: soltar la guarda al terminar el ayudante
/// dejaría el fichero de la base sin enlazar bajo una conexión abierta. Eso no falla en
/// el acto —POSIX mantiene el fichero vivo mientras haya un descriptor abierto—, así
/// que el síntoma no es «el fichero no está» sino un fallo **intermitente**: en cuanto
/// el pool abre la siguiente conexión no encuentra el directorio, y la petición que lo
/// provocó contesta `404` sobre una fila que existe. Retener la guarda lo elimina por
/// construcción.
///
/// Lo que se paga, y hay que saberlo: los directorios del arnés **no se borran** durante
/// la corrida y quedan en el directorio temporal del sistema, que es quien los recoge.
/// Se prefiere eso a un fallo intermitente: un directorio que no se borra se ve con
/// `ls`, y una petición que contesta `404` sobre una fila que existe no se ve en
/// ninguna parte. Esta decisión se revisa el día que el arnés se replantee.
static GUARDAS_DEL_ARNES: OnceLock<Mutex<Vec<DirectorioDePrueba>>> = OnceLock::new();

/// Retiene la guarda de un directorio de base del arnés. Ver [`GUARDAS_DEL_ARNES`].
fn retener(directorio: DirectorioDePrueba) {
    GUARDAS_DEL_ARNES
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("el registro de guardas del arnés no puede envenenarse")
        .push(directorio);
}

/// Base de prueba del arnés: el DSN, el pool ya migrado y la clave, con el directorio
/// **retenido**.
///
/// La guarda viaja al registro de [`GUARDAS_DEL_ARNES`] porque lo que se entrega aquí
/// sobrevive a esta función: el estado que la recibe se lleva el pool, y el pool vive
/// más que el ayudante que lo abrió.
async fn base_del_arnes() -> (String, SqlitePool, Arc<Clave>) {
    let (directorio, pool, clave) = base_con_clave().await;
    let dsn = directorio.dsn();

    retener(directorio);

    (dsn, pool, clave)
}

/// Estado de prueba con una base **ya abierta y migrada**.
///
/// Es el equivalente de lo que hace `main` al arrancar: quien abre la base y aplica el
/// esquema es de fuera, y el constructor del estado la recibe hecha. La configuración
/// entra tal cual —una prueba que compruebe **qué** no se publica necesita una
/// configuración distinguible, con su propio `DATABASE_URL`—, así que su cadena de base
/// puede no ser la de la base que se entrega: lo que el estado usa es el pool, no la
/// cadena.
///
/// El motor entra ya arrancado, igual que en el arranque de verdad: el estado no lo
/// monta —el motor captura el `Handle` del runtime al construirse, y eso lo hace
/// [`motor_de_pruebas`] desde dentro del runtime de la prueba—.
pub(crate) async fn estado_con_base(config: Config) -> AppState {
    let (_dsn, pool, clave) = base_del_arnes().await;

    estado_de_prueba(config, EstadoDeBase::nuevo(pool, clave))
}

/// El socket de un motor de contenedores que no existe: el del arnés.
///
/// Ninguna prueba de la suite normal habla con contenedores, así que el motor se monta
/// apuntando a una ruta muerta: si apuntara al Podman de esta máquina, la suite mediría
/// el entorno —el CI no tiene ese socket— y una prueba que no habla con contenedores
/// fallaría por lo que hay montado fuera. El valor es el mismo que usa el módulo del
/// motor en sus propias pruebas, repetido aquí porque aquél es privado de su módulo y
/// desde este arnés no se alcanza.
const SOCKET_MUERTO_DEL_MOTOR: &str = "unix:///no/existe/kesto-prueba.sock";

/// El motor con el que se construye cada estado de prueba.
///
/// Vive en **un solo sitio** a propósito: el `AppState` lleva el motor desde este change,
/// y son decenas los puntos de uso que construyen un estado —casi todos a través de los
/// ayudantes de este arnés—. Montarlo aquí deja que los ayudantes cambien **una vez** y
/// que ningún punto de uso se entere de que el motor existe siquiera.
///
/// Va con el socket muerto de arriba y con el abridor **local**: es lo que permite que
/// las pruebas del alta y del explorador abran un repositorio del sistema de ficheros en
/// la ruta que lleve el `endpoint` del acceso, sin S3 y sin motor de contenedores —que es
/// lo que hace que todo esto corra en el CI—. La firma del abridor ya es la que el motor
/// acepta, así que no hay ninguna adaptación en medio.
///
/// Es síncrona porque [`Motor::arrancar`] lo es: lo que necesita no es un `await`, sino
/// estar dentro de un runtime —captura su `Handle`—, y los siete ayudantes ya son `async`,
/// así que la llamada ocurre dentro. Es el mismo camino que usa el módulo del motor en
/// `Aparato::motor_con`.
fn motor_de_pruebas(db: &EstadoDeBase) -> Motor {
    let ajustes = Ajustes::nuevos(SOCKET_MUERTO_DEL_MOTOR).con_abridor(abridor_local);
    let dependencias = Dependencias {
        repositorios: db.repositorios.clone(),
        targets: db.targets.clone(),
        tareas: db.tareas.clone(),
        ejecuciones: db.ejecuciones.clone(),
    };

    Motor::arrancar(ajustes, dependencias)
        .expect("el motor de prueba se monta desde dentro del runtime de la prueba")
}

/// El estado de prueba con la base **dada** y el motor del arnés.
///
/// Es el punto por el que pasan **todas** las pruebas que construyen un estado: el motor
/// se monta una vez, aquí, con el socket muerto y el abridor local de
/// [`motor_de_pruebas`], y quien construye un estado desde una prueba no tiene que saber
/// que el motor existe. Los ayudantes de este arnés lo usan, y los módulos de prueba que
/// construyen su estado a mano —los de `src/api`, que necesitan una configuración
/// distinguible— también: una prueba que construyera el estado con otro motor dejaría de
/// estar midiendo lo que mide la suite.
pub(crate) fn estado_de_prueba(config: Config, db: EstadoDeBase) -> AppState {
    let motor = motor_de_pruebas(&db);

    AppState::new(config, db, motor).expect("el estado de prueba se construye")
}

/// Migrador que falla: su única migración no es SQL.
///
/// Es lo que permite ejercitar «una migración que falla es error de arranque» sin
/// tocar las migraciones de verdad del repositorio: el migrador embebido está en
/// verde por construcción, así que ese camino solo se puede recorrer inyectando
/// uno que falle.
pub(crate) fn migrador_roto() -> Migrator {
    Migrator {
        migrations: Cow::Owned(vec![Migration::new(
            1,
            Cow::Borrowed("rota"),
            MigrationType::Simple,
            Cow::Borrowed("ESTO NO ES SQL"),
            false,
        )]),
        ignore_missing: false,
        locking: false,
        no_tx: false,
    }
}

/// Estado con la autenticación apagada: el modo de desarrollo.
///
/// Su `DATABASE_URL` es el de la base que se le entrega, para que la configuración del
/// arnés no mienta sobre dónde está la base. Las bases de los ayudantes se abren en un
/// directorio temporal y su guarda se **retiene** (ver [`retener`]): la base sobrevive
/// al ayudante, así que la guarda también tiene que sobrevivir.
pub(crate) async fn estado_desarrollo() -> AppState {
    let (dsn, pool, clave) = base_del_arnes().await;

    let config = Config::from_lookup(&lookup_from(&[
        ("KESTO_MASTER_KEY", MASTER_KEY),
        ("BIND_ADDRESS", BIND_ADDRESS),
        ("DATABASE_URL", dsn.as_str()),
    ]))
    .expect("la configuración de desarrollo es válida");

    let db = EstadoDeBase::nuevo(pool, clave);

    estado_de_prueba(config, db)
}

/// Estado con la autenticación encendida.
///
/// Apunta a un puerto local donde nadie escucha, **no** a un emisor de verdad:
/// así cualquier login que una prueba dispare por accidente falla contra
/// `127.0.0.1` al instante, sin resolver un DNS ni salir a la red. Las pruebas
/// que quieren un descubrimiento que funcione usan
/// [`estado_oidc_con_emisor`] con el proveedor falso.
pub(crate) async fn estado_oidc() -> AppState {
    estado_oidc_con_emisor(&emisor_caido()).await
}

/// Lectura inyectada con las variables `OIDC_*`, apuntando a un emisor dado.
///
/// Es lo que permite que una prueba hable con el proveedor falso local: el
/// emisor entra por parámetro en vez de fijarse en el entorno del proceso. El `dsn`
/// entra por el mismo camino, para que la configuración apunte a la base que el estado
/// lleva de verdad.
pub(crate) fn lookup_oidc_con_emisor(emisor: &str, dsn: &str) -> impl Lookup + use<> {
    let variables: HashMap<String, String> = [
        ("KESTO_MASTER_KEY", MASTER_KEY),
        ("KESTO_SECRET", SESSION_SECRET),
        ("OIDC_ISSUER_URL", emisor),
        ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
        ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ("BIND_ADDRESS", BIND_ADDRESS),
        ("DATABASE_URL", dsn),
    ]
    .into_iter()
    .map(|(nombre, valor)| (nombre.to_owned(), valor.to_owned()))
    .collect();

    move |nombre: &str| variables.get(nombre).cloned()
}

/// Estado en modo `Oidc` con el emisor indicado.
pub(crate) async fn estado_oidc_con_emisor(emisor: &str) -> AppState {
    let (dsn, pool, clave) = base_del_arnes().await;

    let config = Config::from_lookup(&lookup_oidc_con_emisor(emisor, &dsn))
        .expect("la configuración OIDC es válida");

    let db = EstadoDeBase::nuevo(pool, clave);

    estado_de_prueba(config, db)
}

/// Router de producción en modo `Oidc`, apuntando al emisor indicado.
pub(crate) async fn router_oidc_con_emisor(emisor: &str) -> Router {
    crate::api::api_router(estado_oidc_con_emisor(emisor).await)
}

/// Emisor al que nada contesta: un puerto local que se reserva **y no se suelta**.
///
/// El puerto se obtiene con un `TcpListener` efímero que se **conserva**: un hilo
/// acepta cada conexión y la cierra sin escribir nada, de modo que `reqwest` falla
/// al instante con un EOF en vez de esperar al timeout del descubrimiento.
///
/// Antes el puerto se reservaba, se **soltaba** y se devolvía la URL. Eso dejaba
/// una carrera real: entre soltarlo y el `reqwest` del login, otro test en
/// paralelo —o el `MockServer` de otro— puede ocupar ese puerto, y entonces este
/// «proveedor caído» contestaría un descubrimiento válido y el escenario del
/// `502` fallaría con un `302`. La probabilidad es ínfima, pero la suite corre en
/// CI muchas veces. Conservar el listener elimina la carrera por construcción: el
/// puerto no vuelve a quedar libre, así que nadie puede ocuparlo. **No reintroducir
/// el `drop(listener)`**: «aceptar y cerrar» sigue siendo «un proveedor al que
/// nada contesta».
///
/// **No sale a la red**: todo queda en `127.0.0.1`.
pub(crate) fn emisor_caido() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("la reserva de un puerto efímero no puede fallar");
    let puerto = listener
        .local_addr()
        .expect("el puerto reservado debe conocerse")
        .port();

    // El hilo vive tanto como el proceso de pruebas: sostiene el puerto mientras
    // el test lo necesite, que es lo que impide que otro lo ocupe.
    std::thread::spawn(move || {
        // Cerrar la conexión sin contestar deja a `reqwest` con un EOF
        // inmediato: el descubrimiento falla rápido.
        for stream in listener.incoming().flatten() {
            drop(stream);
        }
    });

    format!("http://127.0.0.1:{puerto}")
}

/// Parámetros de la cadena de consulta de una URL, ya decodificados.
pub(crate) fn parametros_de_consulta(url: &str) -> HashMap<String, String> {
    let consulta = url
        .split_once('?')
        .map(|(_, consulta)| consulta)
        .unwrap_or("");

    consulta
        .split('&')
        .filter_map(|par| par.split_once('='))
        .map(|(clave, valor)| {
            let decodificar = |texto: &str| {
                urlencoding::decode(texto)
                    .map(|decodificada| decodificada.into_owned())
                    .unwrap_or_else(|_| texto.to_owned())
            };

            (decodificar(clave), decodificar(valor))
        })
        .collect()
}

/// Estado de autenticación suelto, para colgar el *gate* de un router de prueba.
pub(crate) async fn auth_oidc() -> AuthState {
    estado_oidc()
        .await
        .auth
        .expect("el modo Oidc construye estado de autenticación")
}

/// Router de producción con la autenticación apagada.
pub(crate) async fn router_desarrollo() -> Router {
    crate::api::api_router(estado_desarrollo().await)
}

/// Router de producción con la autenticación encendida.
pub(crate) async fn router_oidc() -> Router {
    crate::api::api_router(estado_oidc().await)
}

/// Cabecera `Cookie` con la cookie de sesión y el token dado.
pub(crate) fn session_cookie_header(token: &str) -> String {
    format!("{}={token}", crate::auth::session::SESSION_COOKIE_NAME)
}

/// Token del JWT de sesión que viaja en la `Set-Cookie` de una respuesta.
///
/// Devuelve solo el valor de la cookie —el JWT—, sin atributos: es lo que un
/// test necesita para verificar la identidad de la sesión emitida.
pub(crate) fn token_de_set_cookie(response: &Response) -> Option<String> {
    header_text(response, header::SET_COOKIE).and_then(|cabecera| {
        cabecera
            .split(';')
            .next()
            .and_then(|par| {
                par.trim()
                    .strip_prefix(&format!("{}=", crate::auth::session::SESSION_COOKIE_NAME))
            })
            .map(ToOwned::to_owned)
    })
}

/// URL del callback con el `code` y el `state` dados, ya codificados.
///
/// Se codifican aunque hoy los dos sean alfanuméricos: si un día dejan de serlo,
/// la URL sigue siendo correcta sin que nadie se acuerde de este arnés.
pub(crate) fn callback_url(state: &str, code: &str) -> String {
    format!(
        "/api/v1/auth/callback?code={}&state={}",
        urlencoding::encode(code),
        urlencoding::encode(state)
    )
}

/// Lanza un `GET /api/v1/auth/login` y devuelve el `state` y el `nonce` emitidos.
///
/// Es el arranque del flujo que el callback consume: los tests que necesitan un
/// `state` de verdad —y su `nonce` para firmar un ID token coherente— empiezan
/// aquí y no inventan ni uno ni otro.
pub(crate) async fn login_state_y_nonce(router: Router) -> (String, String) {
    let respuesta = get(router, "/api/v1/auth/login").await;

    assert_eq!(
        respuesta.status(),
        axum::http::StatusCode::FOUND,
        "el login debe redirigir para emitir el state y el nonce"
    );

    let location =
        header_text(&respuesta, header::LOCATION).expect("el 302 del login trae Location");
    let parametros = parametros_de_consulta(&location);

    (
        parametros
            .get("state")
            .expect("la redirección del login debe traer state")
            .clone(),
        parametros
            .get("nonce")
            .expect("la redirección del login debe traer nonce")
            .clone(),
    )
}

/// Cookie de sesión válida emitida por el propio Kesto.
pub(crate) fn cookie_de_sesion() -> String {
    let token = crate::auth::session::issue(
        &SecretString::new(SESSION_SECRET.to_owned()),
        SUB.to_owned(),
        EMAIL.to_owned(),
    )
    .expect("la emisión de la sesión de prueba no puede fallar");

    session_cookie_header(&token)
}

/// Lanza una petición sin cuerpo contra `router`, sin abrir un socket.
async fn request(router: Router, metodo: Method, uri: &str) -> Response {
    let peticion = Request::builder()
        .method(metodo)
        .uri(uri)
        .body(Body::empty())
        .expect("petición de prueba válida");

    router
        .oneshot(peticion)
        .await
        .expect("el router no debe fallar")
}

/// Lanza un `GET` contra `router` sin abrir un socket.
pub(crate) async fn get(router: Router, uri: &str) -> Response {
    request(router, Method::GET, uri).await
}

/// Lanza un `POST` sin cuerpo contra `router` sin abrir un socket.
pub(crate) async fn post(router: Router, uri: &str) -> Response {
    request(router, Method::POST, uri).await
}

/// Lanza un `POST` con un cuerpo JSON contra `router` sin abrir un socket.
///
/// Es lo que hace falta para ejercitar el camino real de un alta: el cuerpo lleva el
/// `Content-Type` que axum exige para extraerlo con `Json`, y se serializa del `Value`
/// que da la prueba, sin declarar un segundo tipo de cuerpo en el arnés.
pub(crate) async fn post_json(router: Router, uri: &str, cuerpo: &serde_json::Value) -> Response {
    let peticion = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(cuerpo.to_string()))
        .expect("petición de prueba válida");

    router
        .oneshot(peticion)
        .await
        .expect("el router no debe fallar")
}

/// Lanza un `PUT` con un cuerpo JSON contra `router` sin abrir un socket.
///
/// Es lo que hace falta para ejercitar la actualización, que es reemplazo completo: el
/// cuerpo lleva los mismos campos que el de un alta y viaja con el `Content-Type` que axum
/// exige para extraerlo con `Json`. Se serializa del `Value` que da la prueba, sin declarar
/// un segundo tipo de cuerpo en el arnés, y la cabecera se pone aquí por el mismo motivo
/// que en [`post_json`]: sin ella caería el extractor por la cabecera y la prueba mediría
/// un caso distinto del que quiere medir.
pub(crate) async fn put_json(router: Router, uri: &str, cuerpo: &serde_json::Value) -> Response {
    let peticion = Request::builder()
        .method(Method::PUT)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(cuerpo.to_string()))
        .expect("petición de prueba válida");

    router
        .oneshot(peticion)
        .await
        .expect("el router no debe fallar")
}

/// Lanza un `DELETE` contra `router` sin abrir un socket.
///
/// Se apoya en [`request`] —igual que [`get`]— porque un borrado no lleva cuerpo: lo que
/// hay que ejercitar es el camino del manejador y el código que contesta, no un extractor
/// de cuerpo que esta ruta no tiene.
pub(crate) async fn delete(router: Router, uri: &str) -> Response {
    request(router, Method::DELETE, uri).await
}

/// Lanza un `POST` con un cuerpo crudo contra `router` sin abrir un socket.
///
/// Es lo que hace falta para ejercitar un cuerpo que **no se puede construir** con
/// [`post_json`]: una cadena suelta que no es un objeto, o texto que ni siquiera es JSON.
/// El cuerpo viaja tal cual, sin serializar ni validar antes, y la cabecera
/// `Content-Type` se pone a propósito: lo que se quiere provocar es el rechazo del
/// **cuerpo** —su sintaxis o sus tipos—, no el de la cabecera, que si faltara haría caer
/// al extractor por otra razón y la prueba mediría un caso distinto.
pub(crate) async fn post_crudo(router: Router, uri: &str, cuerpo: &str) -> Response {
    let peticion = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(cuerpo.to_owned()))
        .expect("petición de prueba válida");

    router
        .oneshot(peticion)
        .await
        .expect("el router no debe fallar")
}

/// Lanza una petición con una cabecera `Cookie` ya formada.
async fn request_with_cookie(router: Router, metodo: Method, uri: &str, cookie: &str) -> Response {
    let peticion = Request::builder()
        .method(metodo)
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .expect("petición de prueba válida");

    router
        .oneshot(peticion)
        .await
        .expect("el router no debe fallar")
}

/// Lanza un `GET` con la cabecera `Cookie` indicada.
pub(crate) async fn get_with_cookie(router: Router, uri: &str, cookie: &str) -> Response {
    request_with_cookie(router, Method::GET, uri, cookie).await
}

/// Lanza un `POST` con la cabecera `Cookie` indicada.
pub(crate) async fn post_with_cookie(router: Router, uri: &str, cookie: &str) -> Response {
    request_with_cookie(router, Method::POST, uri, cookie).await
}

/// Cuerpo de la respuesta, como texto.
pub(crate) async fn body_text(response: Response) -> String {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("el cuerpo de la respuesta debe poder leerse")
        .to_bytes();

    String::from_utf8(bytes.to_vec()).expect("el cuerpo debe ser UTF-8")
}

/// Cuerpo de la respuesta, como JSON.
pub(crate) async fn body_json(response: Response) -> serde_json::Value {
    let texto = body_text(response).await;

    serde_json::from_str(&texto).expect("el cuerpo debe ser JSON")
}

/// El siguiente **evento** del cuerpo de un stream, ya como texto, o `None` si no llega ninguno.
///
/// Existe porque el cuerpo de `text/event-stream` **no termina**: [`body_text`] se quedaría
/// esperando para siempre a que llegue el final de un cuerpo que no lo tiene. Lo que hace falta es
/// leer por tramos, y `data:` de un evento puede venir partido entre dos tramos del cuerpo —quien
/// escribe el stream no promete nada sobre dónde corta—, así que aquí se **acumula** hasta que el
/// texto contiene el final de un evento (una línea en blanco) y solo entonces se devuelve.
///
/// El plazo es obligatorio y del llamante: sin él, una prueba que espera un evento que el motor no
/// publica se cuelga, y un cuelgue no dice qué faltaba.
pub(crate) async fn siguiente_evento(cuerpo: &mut Body, plazo: Duration) -> Option<String> {
    let limite = Instant::now() + plazo;
    let mut acumulado = String::new();

    loop {
        let restante = limite.saturating_duration_since(Instant::now());

        let Ok(Some(Ok(tramo))) = tokio::time::timeout(restante, cuerpo.frame()).await else {
            // Se agotó el plazo, o el cuerpo terminó, o falló la lectura: en los tres casos no
            // llegó un evento, y quien llama tiene que poder distinguirlo de «llegó vacío».
            return None;
        };

        let Ok(datos) = tramo.into_data() else {
            // Un tramo que no son datos (los *trailers* de un cuerpo por trozos) no forma parte
            // del stream y no se acumula.
            continue;
        };

        acumulado.push_str(&String::from_utf8_lossy(&datos));

        if acumulado.contains("\n\n") {
            return Some(acumulado);
        }
    }
}

/// El campo `data` del evento, ya interpretado como JSON.
///
/// El cuerpo de un stream son líneas `event:` y `data:`, y el `data` es el JSON del §5. Se saca
/// aquí —y no en cada prueba— porque todas las pruebas del stream hacen lo mismo con él: leerlo y
/// mirar sus campos.
pub(crate) fn datos_del_evento(evento: &str) -> serde_json::Value {
    let crudo = evento
        .lines()
        .find_map(|linea| linea.strip_prefix("data:"))
        .unwrap_or_else(|| panic!("el evento no trae ninguna línea `data:`: {evento:?}"));

    serde_json::from_str(crudo.trim())
        .unwrap_or_else(|error| panic!("el `data` del evento no es JSON ({error}): {crudo:?}"))
}

/// Valor textual de una cabecera de la respuesta, si está.
pub(crate) fn header_text(response: &Response, name: HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|valor| valor.to_str().ok())
        .map(ToOwned::to_owned)
}

/// Buffer de registro en memoria, compartido por todas las copias que se clonen.
///
/// Es el destino al que el suscriptor de [`capturar_log`] escribe: lo que se
/// registre en el hilo pasa a ser texto que la prueba puede inspeccionar, en vez
/// de salir por la salida estándar.
#[derive(Clone, Default)]
pub(crate) struct BufferDeLog(Arc<Mutex<Vec<u8>>>);

impl BufferDeLog {
    /// Texto acumulado hasta este instante.
    pub(crate) fn texto(&self) -> String {
        let bytes = self
            .0
            .lock()
            .expect("el buffer de log no puede envenenarse");

        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl io::Write for BufferDeLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("el buffer de log no puede envenenarse")
            .extend_from_slice(buf);

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufferDeLog {
    type Writer = BufferDeLog;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

// ── Captura del registro ─────────────────────────────────────────────────────
//
// El arnés instala **un suscriptor global**, una sola vez por proceso, y elige a
// qué buffer escribe cada evento por **hilo**: un `thread_local` guarda el buffer
// del test que está capturando en ese hilo, y el evento que sale de un hilo sin
// captura se descarta en silencio.
//
// Antes esto era un `set_default` —un suscriptor **de hilo**— y de ahí salía un
// fallo que no se reproduce en local y sí en CI, y cuyo síntoma no se parece a lo
// que es: **un registro que falta**, no un test rojo. La cadena, medida en
// `tracing-core` (`0.1.32`; el `0.1.36` que resuelve este árbol lleva el mismo
// código):
//
//   * `callsite.rs:541` — `has_just_one` nace en `true` y solo se recalcula en
//     `register_dispatch` como `dispatchers.len() <= 1`, ya retenidas las
//     referencias débiles muertas.
//   * `callsite.rs:566` (la `565` en `0.1.36`) —
//     `Rebuilder::JustOne => dispatcher::get_default(f)`: con un solo despachante
//     vivo, reconstruir el interés **no itera el registro global**, le pregunta
//     al despachante **del hilo actual**.
//   * `subscriber.rs:661` — `Interest::and`: mezclar opiniones da `sometimes`
//     (inofensivo); el interés solo se queda en `never` si **todos** los
//     consultados dicen `never`.
//
// Consecuencia: un callsite que se registre por primera vez desde un hilo **sin**
// suscriptor, mientras solo haya un despachante vivo, queda cacheado como
// `Interest::never()` **para todo el proceso** —`get_default` devolvió
// `Dispatch::none()`—. A partir de ahí ese `tracing::error!` no construye el
// evento y lo descarta, en cualquier hilo, aunque el suscriptor esté instalado.
// Es lo que pasa en CI: `AppError::BadGateway::into_response` lo ejecutan también
// tests que **no** capturan, y con dos CPUs rara vez hay dos capturando a la vez,
// así que `has_just_one` casi siempre es `true`. El síntoma es exactamente el
// medido: llega un `DEBUG` del propio camino de la petición y falta el `ERROR`
// del mismo camino, con el mismo hilo y el mismo despachante.
//
// Con el suscriptor **global**, `get_default` lo resuelve desde **cualquier**
// hilo —`SCOPED_COUNT == 0` va directo a `get_global()`, y con `SCOPED_COUNT > 0`
// el `None` del hilo cae también al global—, así que `Rebuilder::JustOne` ya no
// puede consultar un hilo sin suscriptor: el interés de un callsite no puede
// quedar en `never` (en `sometimes` sí, si otro despachante opina distinto).
// Quedan dos caminos a `Dispatch::none()` —`state.enter()` devolviendo `None`
// por un despacho anidado, y `try_with` fallando durante la destrucción del
// `thread_local`—: ninguno se da en esta suite, porque estrenar un callsite
// exigiría hacerlo desde dentro de un despachante o de un destructor de TLS.
// **No reintroducir `set_default` ni `with_default` en este arnés**: es el único
// que instala un despachante en el binario de pruebas, y volver al de hilo
// devuelve el fallo de arriba, que no se ve en local.

thread_local! {
    /// Buffer de captura del hilo actual, si ese hilo está capturando.
    static BUFFER_DEL_HILO: RefCell<Option<BufferDeLog>> = const { RefCell::new(None) };
}

/// Marca de que el suscriptor global del arnés ya está instalado.
///
/// Un `OnceLock` y no un `bool`: la instalación tiene que ser **idempotente** y
/// segura ante dos tests que la pidan a la vez, porque `set_global_default` solo
/// se puede llamar una vez por proceso.
static SUSCRIPTOR_GLOBAL: OnceLock<()> = OnceLock::new();

/// Escritor del suscriptor global: escribe en el buffer del hilo, si lo hay.
///
/// Es lo que el suscriptor entrega a cada evento. Fuera del alcance de una
/// captura no hay buffer y el registro se descarta **en silencio**, como un
/// `/dev/null`: así un hilo que no captura no contamina a nadie, la suite sigue
/// corriendo en paralelo y no hace falta serializar tests ni compartir un
/// `Mutex` global.
struct EscritorDeCaptura(Option<BufferDeLog>);

impl io::Write for EscritorDeCaptura {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &mut self.0 {
            Some(buffer) => buffer.write(buf),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `MakeWriter` del suscriptor global: elige el buffer **por hilo**.
struct CapturaGlobal;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturaGlobal {
    type Writer = EscritorDeCaptura;

    fn make_writer(&'a self) -> Self::Writer {
        EscritorDeCaptura(BUFFER_DEL_HILO.with(|celda| celda.borrow().clone()))
    }
}

/// Instala el suscriptor global de las pruebas, una sola vez por proceso.
///
/// El filtro es por **nivel**, no por `target` ni por `EnvFilter`: `LevelFilter`
/// contesta `Interest::always` a todo lo que pasa el nivel y no cachea interés
/// por callsite, así que le quita una capa de caché al fallo que este arnés
/// existe para no tener. El nivel es `TRACE` a propósito: un candado de «esto no
/// se registra» solo vale si el registro estaba en su nivel más hablador cuando
/// se comprobó.
///
/// `set_global_default` solo se puede llamar una vez. Si devuelve error es que
/// **otro** sitio de este binario ya instaló un despachante, y eso rompe la
/// captura —el que gane no sabe nada de `BUFFER_DEL_HILO`—: por eso se cae en
/// alto en vez de seguir con pruebas que podrían pasar por vacío. Este arnés es
/// el único que instala un suscriptor en el binario de pruebas.
///
/// **No cambiar esto por `init()` ni `try_init()`**: ésos instalan además el
/// puente `log` → `tracing`, y `wiremock` instrumenta con `log` —`debug!` con el
/// cuerpo de la petición que no esperaba, y el cuerpo del canje lleva el
/// `client_secret`—, así que el puente metería credenciales en el buffer de
/// captura y pondría en rojo las aserciones de ausencia.
///
/// Y la consecuencia de eso hay que tenerla presente, porque es una **trampa**:
/// como el puente no está, un registro que una dependencia haya hecho con `log`
/// **no llega a `tracing`** y no aparece en ningún buffer. Una prueba que busque ahí
/// el valor de una credencial —la 1.37, «la contraseña no sale en el registro»—
/// pasaría sin medir nada: no es que el registro no lleve el valor, es que el
/// registro no llega. Quien necesite medir eso tiene que montar el puente **en su
/// propia prueba**; ver
/// `src/engine/extraccion.rs::tests::la_credencial_de_la_base_no_sale_en_el_registro_ni_al_nivel_mas_hablador`,
/// que instala un puente propio —solo para el objetivo del motor, y reemitiendo el
/// objetivo de cada registro, que es lo que el filtro mira— y comprueba con un
/// **control** que lo que mide llega de verdad.
fn instalar_suscriptor_global() {
    SUSCRIPTOR_GLOBAL.get_or_init(|| {
        let suscriptor = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .without_time()
            .with_writer(CapturaGlobal)
            .finish();

        if let Err(error) = tracing::subscriber::set_global_default(suscriptor) {
            panic!(
                "el arnés de captura no pudo instalar su suscriptor global: {error}. \
                 Este arnés es el único que instala un suscriptor en el binario de \
                 pruebas: quita el que lo pisa —el único del crate está en \
                 `src/main.rs:99`, y usa `try_init()`—, o la captura de registro \
                 deja de ser fiable."
            );
        }

        // Aquí hace falta, y no es ceremonia: `set_global_default` ya reconstruye
        // el interés de lo **ya registrado** —`Dispatch::new` llama a
        // `register_dispatch`, que itera el registro con `Rebuilder::Write`—,
        // pero lo hace **antes** de poner `GLOBAL_INIT` en `INITIALIZED`. En esa
        // ventana, un callsite estrenado por un test que no captura toma
        // `Rebuilder::JustOne` → `dispatcher::get_default` → `GLOBAL_INIT !=
        // INITIALIZED` → `Dispatch::none()` → `Interest::never`, y puede caer
        // fuera del recorrido del `Write`. Esta llamada, ya con el global puesto,
        // es lo que repara esos callsites: sin ella quedarían envenenados para
        // todo el proceso.
        tracing::callsite::rebuild_interest_cache();
    });
}

/// Guardián de una captura: al soltarse, devuelve al hilo el buffer que tenía.
///
/// Guarda el valor **anterior** del `thread_local`, no un `None` a secas: si una
/// captura se anidara dentro de otra, al cerrar la de dentro hay que volver a la
/// de fuera, no dejar el hilo sin buffer. Mantenerlo vivo mientras se lean los
/// eventos es obligatorio: al soltarlo, el hilo deja de capturar.
/// No la muevas de hilo: su `Drop` escribe en el `thread_local` del hilo que la
/// suelta, así que soltarla en otro hilo le dejaría a ése el buffer prestado.
#[must_use = "soltar la guardia deja de capturar: manténla viva mientras leas el buffer"]
pub(crate) struct GuardiaDeCaptura(Option<BufferDeLog>);

impl Drop for GuardiaDeCaptura {
    fn drop(&mut self) {
        BUFFER_DEL_HILO.with(|celda| *celda.borrow_mut() = self.0.take());
    }
}

/// Captura el registro del hilo actual en memoria y devuelve el buffer.
///
/// La firma no cambia para sus ocho puntos de uso —todos
/// `let (buffer, _guard) = capturar_log();`—: cambia el motor. El suscriptor es
/// **global** y se instala una sola vez ([`instalar_suscriptor_global`]); lo que
/// cada llamada fija es el **buffer del hilo**. Ese es el arreglo: con un
/// suscriptor de hilo, reconstruir el interés podía preguntar a un hilo sin
/// suscriptor y dejar un callsite envenenado como `never`, y desde entonces su
/// registro desaparece para todo el proceso.
///
/// **El síntoma de aquel fallo es «un registro que falta», no un test rojo**, y
/// eso es lo que lo hace traicionero: el test no cae por lo que registra de menos,
/// sino por la aserción que comprueba que la captura está en pie. Si algún día
/// esta captura vuelve a fallar así, se arregla aquí —no aflojando la aserción—.
pub(crate) fn capturar_log() -> (BufferDeLog, GuardiaDeCaptura) {
    instalar_suscriptor_global();

    let buffer = BufferDeLog::default();
    let anterior = BUFFER_DEL_HILO.with(|celda| celda.borrow_mut().replace(buffer.clone()));

    (buffer, GuardiaDeCaptura(anterior))
}

// ── Volcados de prueba ───────────────────────────────────────────────────────
//
// El puente de `src/rustic` entrega el volcado por un `ReadSource` —el trait de la
// biblioteca de copia—, así que leerlo desde una prueba exige pasar por el mismo camino que
// pasa `archive()`: `entries()` → la entrada → su `open` → `.open()`. Ese camino vive aquí y
// no en cada módulo porque quien lo recorre es un **consumidor** del puente —el motor—, y las
// pruebas del motor no tienen por qué conocer el trait de la biblioteca.

/// Abre el lector del volcado de un origen, por el mismo camino que la biblioteca de copia.
///
/// Si la entrada del origen no trajera cómo abrirse, la biblioteca no podría leer nada: por
/// eso la prueba pasa por aquí y no por un atajo propio, que probaría otra cosa.
pub(crate) fn lector_del_volcado(
    origen: &crate::rustic::OrigenDelVolcado,
) -> crate::rustic::LectorDeVolcado {
    use rustic_core::{ReadSource as _, ReadSourceOpen as _};

    let mut entradas = origen.entries();
    let entrada = entradas
        .next()
        .expect("el origen de un volcado entrega una entrada")
        .expect("la entrada de un volcado no puede ser un error");

    entrada
        .open
        .expect("la entrada del volcado tiene que traer cómo abrirse")
        .open()
        .expect("el lector del volcado tiene que abrirse")
}

// ── Motor de contenedores ────────────────────────────────────────────────────
//
// Las pruebas que necesitan un motor de contenedores van marcadas `#[ignore]`, y hoy son
// **veinte**: once en `src/docker` (todas en `cliente.rs`) y nueve en `src/engine` (siete
// en `extraccion.rs` y dos en `mod.rs`). El motivo de que estén marcadas y no se
// ejecuten es el CI: el job `backend` de `.github/workflows/ci.yml` no declara
// servicios, así que `cargo test` tiene que quedar verde sin motor. Lo de abajo es el
// arnés de toda esa capa, y quien las lance con `cargo test -- --ignored` necesita un
// motor de verdad en la máquina.
//
// El motivo completo va escrito **en cada** `#[ignore]` y no solo aquí: quien lee una
// prueba ignorada lee su atributo, y una nota que vive en otro fichero es una nota que
// no se lee. Aquí queda el conjunto —cuántas hay y en qué módulos—, que es lo que no se
// puede saber mirando una sola prueba.

/// Imagen del contenedor de prueba.
///
/// **Ya está en la caché local de esta máquina**: las pruebas no la descargan, y por eso
/// el nombre lleva el registro delante, que es como la tiene Podman.
const IMAGEN_DE_PRUEBA: &str = "docker.io/library/alpine:3.23";

/// Comando del contenedor de prueba: se queda vivo sin gastar CPU.
///
/// Va troceado en argumentos y no como `"sleep 600"`: la API de Docker no pasa por un
/// intérprete, así que una cadena con un espacio dentro sería el nombre de un binario que
/// no existe.
const COMANDO_DE_PRUEBA: &[&str] = &["sleep", "600"];

/// Tiempo máximo por petición de las pruebas con motor.
///
/// Holgado a propósito, y **medido**: con siete pruebas creando y arrancando contenedores a
/// la vez, las peticiones al motor se hacen cola y una de ellas superó los 60 segundos
/// —`RequestTimeoutError` en el alta— con el motor perfectamente vivo. Un tiempo corto aquí
/// solo produce pruebas que fallan por el entorno y no por el código, así que el valor es
/// cinco veces el que reventó.
pub(crate) const TIMEOUT_DEL_MOTOR: Duration = Duration::from_secs(300);

/// Socket del motor de contenedores de esta máquina, si hay uno.
///
/// Primero `DOCKER_HOST`, que es la variable que respeta el contrato de configuración y la
/// que define un despliegue de verdad; si no está, el socket de Podman de un usuario, que
/// es lo que hay en esta máquina de desarrollo —aquí no hay Docker—. Devuelve `None` si la
/// ruta no existe: devolver una que no está convertiría «aquí no hay motor» en un fallo de
/// conexión de la prueba, que es un diagnóstico peor.
///
/// Que este ayudante lea el entorno **no** contradice la regla de que el socket entre por
/// parámetro en `Cliente::conectar`: lo que el contrato prohíbe es que el **código de
/// producción** lo lea por su cuenta, porque entonces dos pruebas no podrían apuntar a dos
/// sockets distintos a la vez. El arnés solo necesita *encontrar* el motor que hay aquí.
pub(crate) fn socket_del_motor() -> Option<String> {
    let candidato = std::env::var("DOCKER_HOST")
        .ok()
        .filter(|valor| !valor.trim().is_empty())
        .unwrap_or_else(ruta_de_podman);

    // La biblioteca acepta también la ruta pelada, así que la comprobación se hace sobre
    // ella; lo que se devuelve es el valor tal y como estaba, que es lo que hay que pasarle.
    let ruta = candidato.trim_start_matches("unix://");

    Path::new(ruta).exists().then_some(candidato)
}

/// Ruta del socket de Podman de esta máquina, si no hay `DOCKER_HOST`.
fn ruta_de_podman() -> String {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) if !dir.is_empty() => format!("unix://{dir}/podman/podman.sock"),
        // Último recurso: la ruta medida en esta máquina.
        _ => "unix:///run/user/1000/podman/podman.sock".to_owned(),
    }
}

/// Cliente del motor de contenedores de esta máquina, si hay uno.
///
/// Es el punto de entrada de las pruebas marcadas `#[ignore]`. Si no hay socket devuelve
/// `None`, y quien las lance con `--ignored` tiene que tratarlo como un fallo del entorno y
/// no del código: el mensaje del `expect` lo dice.
pub(crate) fn cliente_del_motor() -> Option<Cliente> {
    let socket = socket_del_motor()?;

    Cliente::conectar(&socket, TIMEOUT_DEL_MOTOR).ok()
}

/// Contenedor de prueba: se crea al construirse y se borra al soltarse.
///
/// El nombre es **único por prueba** —quien llama pasa el nombre de su propia prueba— porque
/// la suite corre en paralelo: un contador compartido, además de necesitar sincronización,
/// dejaría el nombre sin relación con la prueba que dejó el contenedor vivo.
pub(crate) struct ContenedorDePrueba {
    /// Cliente de la biblioteca, solo para el borrado por la API.
    motor: bollard::Docker,
    /// Nombre del contenedor.
    nombre: String,
    /// Socket con el que se creó: lo necesita el `Drop`, que no puede ser asíncrono.
    socket: String,
}

impl ContenedorDePrueba {
    /// Crea y arranca un contenedor de la imagen de prueba, con `sleep 600`.
    ///
    /// El guardián (`Self`) se construye **entre** el alta y el arranque: en cuanto el
    /// contenedor existe en el motor hay algo que borrar, y construirlo al final dejaba el
    /// arranque —y cualquier pánico intermedio— sin nadie que lo limpiara. Ver el comentario
    /// de dentro.
    pub(crate) async fn nuevo(prueba: &str) -> Self {
        let socket = socket_del_motor()
            .expect("las pruebas del motor necesitan un socket de Docker o Podman");
        let motor = bollard::Docker::connect_with_socket(
            &socket,
            TIMEOUT_DEL_MOTOR.as_secs(),
            bollard::API_DEFAULT_VERSION,
        )
        .expect("el socket existe, así que el cliente del arnés se construye");

        let nombre = format!("kesto-prueba-{prueba}");

        // Una corrida anterior que muriera sin limpiar deja el nombre ocupado, y el alta
        // fallaría con un `409` que no hablaría de nada de esto. Se retira lo que hubiera con
        // ese nombre y se ignora el fallo: si no hay nada, el motor contesta que no existe, y
        // eso no es un problema.
        let _ = motor
            .remove_container(&nombre, Some(opciones_de_borrado()))
            .await;

        motor
            .create_container(
                Some(CreateContainerOptions {
                    name: nombre.clone(),
                    platform: None,
                }),
                bollard::container::Config {
                    image: Some(IMAGEN_DE_PRUEBA.to_owned()),
                    cmd: Some(
                        COMANDO_DE_PRUEBA
                            .iter()
                            .map(|arg| (*arg).to_owned())
                            .collect(),
                    ),
                    ..Default::default()
                },
            )
            .await
            .expect("el contenedor de prueba debe poder crearse");

        // El guardián se construye **aquí**, en cuanto el contenedor existe en el motor, y no
        // al final: a partir de esta línea el nombre está fijado y hay algo que borrar, así que
        // cualquier camino de error posterior —el `start` que sigue, un pánico en medio—
        // suelta el guardián y deja el contenedor limpio. Construirlo al final dejaba una
        // **ventana sin dueño**: medido, un `timeout` bajo en el alta dejó un
        // `kesto-prueba-…` en estado `Created` que ya nadie borraba, porque el `Self` que lo
        // habría hecho no llegó a existir.
        let contenedor = Self {
            motor,
            nombre,
            socket,
        };

        contenedor
            .motor
            .start_container(&contenedor.nombre, None::<StartContainerOptions<String>>)
            .await
            .expect("el contenedor de prueba debe poder arrancarse");

        contenedor
    }

    /// Nombre del contenedor.
    pub(crate) fn nombre(&self) -> &str {
        &self.nombre
    }

    /// Lo retira por la API, que es el camino principal de limpieza.
    ///
    /// El `Drop` es la red de seguridad —no puede esperar a nada—, así que cada prueba llama
    /// a esto al terminar y el borrado explícito queda en su salida. Un fallo aquí no tumba
    /// la prueba: se cuenta por `stderr` y el `Drop` vuelve a intentarlo con `podman`.
    pub(crate) async fn retirar(&self) {
        if let Err(error) = self
            .motor
            .remove_container(&self.nombre, Some(opciones_de_borrado()))
            .await
        {
            eprintln!(
                "el contenedor de prueba {} no se pudo retirar por la API: {error}",
                self.nombre
            );
        }
    }
}

/// Opciones de borrado: con `force`, para que un contenedor en marcha también caiga.
fn opciones_de_borrado() -> RemoveContainerOptions {
    RemoveContainerOptions {
        force: true,
        ..Default::default()
    }
}

impl Drop for ContenedorDePrueba {
    fn drop(&mut self) {
        // Aquí no se puede esperar a nada: el borrado por la API es asíncrono y este `Drop`
        // corre en un hilo que puede estar soltando su runtime. Se llama al programa `podman`
        // —en esta máquina no hay `docker`— y el fallo se ignora: es una red de seguridad, no
        // el camino principal, que es `retirar`. Si el contenedor ya no está, `rm -f` hizo su
        // trabajo.
        let _ = std::process::Command::new("podman")
            .args(["--remote", "--url", &self.socket, "rm", "-f", &self.nombre])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::capturar_log;

    /// Texto del callsite de `debug!` de la prueba de abajo.
    const MENSAJE_DE_DEBUG: &str = "mensaje del callsite de depuración del arnés";
    /// Texto del callsite de `error!` de la prueba de abajo.
    const MENSAJE_DE_ERROR: &str = "mensaje del callsite de error del arnés";

    /// Emite desde **su propio** callsite: el `debug!` de esta función.
    ///
    /// Solo lo usa la prueba de abajo, y solo desde el hilo que captura.
    fn emitir_por_debug() {
        tracing::debug!("{MENSAJE_DE_DEBUG}");
    }

    /// Emite desde **su propio** callsite: el `error!` de esta función.
    ///
    /// Solo lo usa la prueba de abajo, y su **primer** registro tiene que salir
    /// del hilo ajeno de esa prueba: es el que estrena el callsite.
    fn emitir_por_error() {
        tracing::error!("{MENSAJE_DE_ERROR}");
    }

    /// El candado del arnés: un callsite **estrenado desde un hilo sin captura**
    /// tiene que llegar igualmente al buffer del hilo que captura, y otro
    /// callsite distinto también.
    ///
    /// Es la invariante que se rompió en CI, en su forma mínima: allí llegaba al
    /// buffer el `DEBUG` de un callsite y no el `ERROR` de otro. El `ERROR` se
    /// había estrenado desde un hilo sin suscriptor —`AppError::BadGateway` lo
    /// ejecutan también tests que no capturan— y quedó cacheado como
    /// `Interest::never()` **para todo el proceso**: desde entonces su registro
    /// se descarta en cualquier hilo, incluido el que captura.
    ///
    /// Por eso el `debug!` no basta y no puede bastar: se registra desde el hilo
    /// que captura y llegaría igual con el arnés viejo. Lo que distingue a este
    /// candado es el `error!`, y sobre todo **el orden**: la captura tiene que
    /// estar en pie antes de que el hilo ajeno estrene el callsite, porque
    /// instalar el suscriptor global reconstruye el interés de lo ya registrado.
    /// Con el despachante de hilo, este test cae en cuanto es el único
    /// capturando, que es la condición del fallo; con el global, no puede caer.
    #[test]
    fn un_callsite_estrenado_sin_captura_llega_al_buffer() {
        // El orden es el del fallo: la captura primero, y con ella el único
        // despachante vivo.
        let (buffer, _guard) = capturar_log();

        // El callsite del `error!` se estrena aquí, en un hilo sin captura: es
        // lo que hacía el test que no capturaba en CI. Con el arnés viejo este
        // primer registro pregunta al despachante de **este** hilo —ninguno— y
        // cachea `never` para todo el proceso.
        std::thread::spawn(emitir_por_error)
            .join()
            .expect("el hilo que estrena el callsite no puede entrar en pánico");

        // Y ahora ese mismo callsite desde el hilo que captura, más un callsite
        // distinto que estrena este hilo.
        emitir_por_error();
        emitir_por_debug();

        let log = buffer.texto();

        assert!(
            log.contains(MENSAJE_DE_ERROR),
            "el registro de un callsite estrenado sin captura se perdió: {log}"
        );
        assert!(
            log.contains(MENSAJE_DE_DEBUG),
            "el registro del callsite de `debug!` no llegó al buffer: {log}"
        );
    }
}
