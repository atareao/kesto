//! Envoltorio único de las respuestas de la API.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Serialize, Serializer};

/// Envoltorio de toda respuesta atendida: `{status, message, data}`.
#[derive(Debug, Serialize)]
pub struct ApiResponse {
    /// Código HTTP de la respuesta, duplicado en el cuerpo.
    ///
    /// Se guarda el número para que viaje en el JSON, pero el constructor
    /// recibe el [`StatusCode`] ya resuelto: así el cuerpo no puede decir un
    /// código distinto del que lleva la cabecera.
    pub status: u16,
    /// Mensaje legible para el cliente.
    pub message: String,
    /// Contenido de la respuesta, `null` cuando no hay ninguno.
    pub data: Data,
}

impl ApiResponse {
    /// Construye una respuesta con su código, su mensaje y su contenido.
    pub fn new(status: StatusCode, message: impl Into<String>, data: Data) -> Self {
        Self {
            status: status.as_u16(),
            message: message.into(),
            data,
        }
    }
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        (status_code(self.status), Json(self)).into_response()
    }
}

/// Envoltorio de una respuesta fallida: `{status, message}`.
///
/// La causa de un `5xx` no entra aquí: va al registro del servidor.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// Código HTTP de la respuesta, duplicado en el cuerpo.
    pub status: u16,
    /// Mensaje legible para el cliente.
    pub message: String,
}

impl ErrorResponse {
    /// Construye la envoltura de error a partir del código y el mensaje.
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status: status.as_u16(),
            message: message.into(),
        }
    }
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        (status_code(self.status), Json(self)).into_response()
    }
}

/// Traduce el código duplicado en el cuerpo a un `StatusCode` de axum.
fn status_code(raw: u16) -> StatusCode {
    StatusCode::from_u16(raw).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Contenido de una respuesta: ausente o un valor JSON.
#[derive(Debug, Clone)]
pub enum Data {
    /// Sin contenido: se serializa como `null`.
    None,
    /// Un valor JSON.
    Some(serde_json::Value),
}

impl Serialize for Data {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::None => serializer.serialize_none(),
            Self::Some(valor) => valor.serialize(serializer),
        }
    }
}
