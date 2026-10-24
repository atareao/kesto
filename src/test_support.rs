//! Utilidades que comparten las pruebas: lectura inyectada y arnés del router.
//!
//! El router se ejercita con `tower::ServiceExt::oneshot`, sin abrir sockets.

use std::collections::HashMap;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request};
use axum::response::Response;
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::config::Lookup;

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
