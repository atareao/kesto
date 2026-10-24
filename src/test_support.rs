//! Utilidades que comparten las pruebas: lectura inyectada y arnés del router.
//!
//! El router se ejercita con `tower::ServiceExt::oneshot`, sin abrir sockets.

use std::collections::HashMap;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderName, Method, Request, header};
use axum::response::Response;
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::config::{Config, Lookup};
use crate::secret::SecretString;
use crate::state::{AppState, AuthState};

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

/// Las tres variables `OIDC_*` más la clave de firma: el modo `Oidc` entero.
///
/// Lleva también la dirección de escucha efímera: un estado de prueba no puede
/// heredar el puerto por defecto.
pub(crate) fn variables_oidc() -> Vec<(&'static str, &'static str)> {
    vec![
        ("KESTO_MASTER_KEY", MASTER_KEY),
        ("KESTO_SECRET", SESSION_SECRET),
        ("OIDC_ISSUER_URL", OIDC_ISSUER_URL),
        ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
        ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ("BIND_ADDRESS", BIND_ADDRESS),
    ]
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
pub(crate) fn estado_oidc() -> AppState {
    let config = Config::from_lookup(&lookup_from(&variables_oidc()))
        .expect("la configuración OIDC es válida");

    AppState::new(config).expect("el estado OIDC se construye")
}

/// Estado de autenticación suelto, para colgar el *gate* de un router de prueba.
pub(crate) fn auth_oidc() -> AuthState {
    AuthState {
        secret: SecretString::new(SESSION_SECRET.to_owned()),
    }
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
