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

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderName, Method, Request, header};
use axum::response::Response;
use http_body_util::BodyExt;
use sqlx::SqlitePool;
use sqlx::migrate::{Migration, MigrationType, Migrator};
use tower::ServiceExt;
use uuid::Uuid;

use crate::config::{Config, Lookup};
use crate::db::crypto::Clave;
use crate::secret::SecretString;
use crate::state::{AppState, AuthState};

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
pub(crate) fn estado_desarrollo() -> AppState {
    let config = Config::from_lookup(&lookup_from(&[
        ("KESTO_MASTER_KEY", MASTER_KEY),
        ("BIND_ADDRESS", BIND_ADDRESS),
    ]))
    .expect("la configuración de desarrollo es válida");

    AppState::new(config).expect("el estado de desarrollo se construye")
}

/// Estado con la autenticación encendida.
///
/// Apunta a un puerto local donde nadie escucha, **no** a un emisor de verdad:
/// así cualquier login que una prueba dispare por accidente falla contra
/// `127.0.0.1` al instante, sin resolver un DNS ni salir a la red. Las pruebas
/// que quieren un descubrimiento que funcione usan
/// [`estado_oidc_con_emisor`] con el proveedor falso.
pub(crate) fn estado_oidc() -> AppState {
    estado_oidc_con_emisor(&emisor_caido())
}

/// Lectura inyectada con las variables `OIDC_*`, apuntando a un emisor dado.
///
/// Es lo que permite que una prueba hable con el proveedor falso local: el
/// emisor entra por parámetro en vez de fijarse en el entorno del proceso.
pub(crate) fn lookup_oidc_con_emisor(emisor: &str) -> impl Lookup + use<> {
    let variables: HashMap<String, String> = [
        ("KESTO_MASTER_KEY", MASTER_KEY),
        ("KESTO_SECRET", SESSION_SECRET),
        ("OIDC_ISSUER_URL", emisor),
        ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
        ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ("BIND_ADDRESS", BIND_ADDRESS),
    ]
    .into_iter()
    .map(|(nombre, valor)| (nombre.to_owned(), valor.to_owned()))
    .collect();

    move |nombre: &str| variables.get(nombre).cloned()
}

/// Estado en modo `Oidc` con el emisor indicado.
pub(crate) fn estado_oidc_con_emisor(emisor: &str) -> AppState {
    let config = Config::from_lookup(&lookup_oidc_con_emisor(emisor))
        .expect("la configuración OIDC es válida");

    AppState::new(config).expect("el estado OIDC se construye")
}

/// Router de producción en modo `Oidc`, apuntando al emisor indicado.
pub(crate) fn router_oidc_con_emisor(emisor: &str) -> Router {
    crate::api::api_router(estado_oidc_con_emisor(emisor))
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
pub(crate) fn auth_oidc() -> AuthState {
    estado_oidc()
        .auth
        .expect("el modo Oidc construye estado de autenticación")
}

/// Router de producción con la autenticación apagada.
pub(crate) fn router_desarrollo() -> Router {
    crate::api::api_router(estado_desarrollo())
}

/// Router de producción con la autenticación encendida.
pub(crate) fn router_oidc() -> Router {
    crate::api::api_router(estado_oidc())
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
