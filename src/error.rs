//! Modelo de error de la API y su traducción a respuesta HTTP.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

use crate::api::response::ErrorResponse;
use crate::config::ConfigError;

/// Mensaje con el que el cliente recibe un fallo interno.
///
/// Es deliberadamente genérico: el texto de la causa se queda en el registro
/// del servidor, porque es donde acaban las credenciales que manejan las
/// bibliotecas.
const INTERNAL_MESSAGE: &str = "error interno del servidor";

/// Error de la aplicación, con el código HTTP que le corresponde.
#[derive(Debug, Error)]
pub enum AppError {
    /// La petición traía algo que no se puede atender: el motivo sí se explica.
    #[error("{0}")]
    InvalidInput(String),

    /// El recurso pedido no existe.
    #[error("{0}")]
    NotFound(String),

    /// El método HTTP no está declarado para esta ruta.
    #[error("{0}")]
    MethodNotAllowed(String),

    /// Fallo interno: el detalle va al registro y al cliente le llega un mensaje genérico.
    #[error("{0}")]
    Other(String),

    /// La configuración no permite arrancar.
    #[error(transparent)]
    Config(#[from] ConfigError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            // El cliente necesita saber qué traía mal la petición: el motivo se explica.
            Self::InvalidInput(motivo) => (StatusCode::BAD_REQUEST, motivo),
            Self::NotFound(motivo) => (StatusCode::NOT_FOUND, motivo),
            // El cliente se equivocó de método, no de recurso: se le explica.
            Self::MethodNotAllowed(motivo) => (StatusCode::METHOD_NOT_ALLOWED, motivo),
            // Estos dos son fallos del servidor: la causa va al log, no al cuerpo.
            Self::Other(causa) => {
                tracing::error!(error = %causa, "fallo interno al atender la petición");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    INTERNAL_MESSAGE.to_owned(),
                )
            }
            Self::Config(error) => {
                // Por construcción, esta variante solo puede dispararse al
                // atender una petición: el arranque no pasa por aquí.
                tracing::error!(error = %error, "error de configuración al atender la petición");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    INTERNAL_MESSAGE.to_owned(),
                )
            }
        };

        ErrorResponse::new(status, message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    use super::AppError;
    use crate::test_support::{body_json, body_text};

    /// Credencial distinguible: si aparece en el cuerpo, el error la filtró.
    const CREDENCIAL: &str = "AKIAIOSFODNN7EXAMPLE";

    #[tokio::test]
    async fn un_error_de_entrada_da_400_con_el_motivo() {
        let motivo = "el repositorio ya existe";
        let respuesta = AppError::InvalidInput(motivo.to_owned()).into_response();

        assert_eq!(respuesta.status(), StatusCode::BAD_REQUEST);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 400);
        assert!(
            cuerpo["message"]
                .as_str()
                .unwrap_or_default()
                .contains(motivo),
            "el 400 debe explicar el motivo: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn un_error_interno_no_filtra_la_causa() {
        let respuesta = AppError::Other(format!("credencial S3: {CREDENCIAL}")).into_response();

        assert_eq!(respuesta.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let texto = body_text(respuesta).await;

        assert!(
            !texto.contains(CREDENCIAL),
            "el 500 publicó la credencial de la causa: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 500 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 500);
        assert_eq!(
            cuerpo["message"],
            super::INTERNAL_MESSAGE,
            "el 500 debe traer exactamente el mensaje genérico: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
    }
}
