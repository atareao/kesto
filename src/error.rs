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

    /// Sin sesión válida no se atiende: el motivo sí se explica.
    #[error("{0}")]
    Unauthorized(String),

    /// Fallo interno: el detalle va al registro y al cliente le llega un mensaje genérico.
    #[error("{0}")]
    Other(String),

    /// Un servicio de terceros no contesta: la causa va al registro y al cliente
    /// le llega el mismo mensaje genérico que con un fallo interno.
    #[error("{0}")]
    BadGateway(String),

    /// La base de datos falló: es un fallo del servidor, así que al cliente le
    /// llega el mismo mensaje genérico que con [`AppError::Other`] y la causa se
    /// queda en el registro.
    ///
    /// El mensaje de [`crate::db::DbError`] **nombra rutas** —lo hace a propósito,
    /// para que el operador sepa qué directorio falta—, y por eso no puede salir
    /// al cuerpo: una ruta del despliegue no es asunto de quien pregunta por una
    /// API.
    #[error(transparent)]
    Database(#[from] crate::db::DbError),

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
            // Al cliente que no ha entrado se le explica el motivo, como en el 400.
            Self::Unauthorized(motivo) => (StatusCode::UNAUTHORIZED, motivo),
            // Estos dos son fallos del servidor: la causa va al log, no al cuerpo.
            Self::Other(causa) => {
                tracing::error!(error = %causa, "fallo interno al atender la petición");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    INTERNAL_MESSAGE.to_owned(),
                )
            }
            // También es un fallo del servidor: el 502 dice que el que falla está
            // fuera, pero el cuerpo no publica la causa —es un `5xx`—, así que
            // lleva el mismo mensaje genérico que el 500.
            Self::BadGateway(causa) => {
                tracing::error!(error = %causa, "un servicio de terceros no contestó");
                (StatusCode::BAD_GATEWAY, INTERNAL_MESSAGE.to_owned())
            }
            // Y el de la base de datos igual: la causa —que nombra rutas y puede
            // llevar el texto de una consulta— va al registro, y al cliente le
            // llega el mensaje genérico de los fallos internos.
            Self::Database(causa) => {
                tracing::error!(error = %causa, "la base de datos falló");
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

    #[tokio::test]
    async fn una_peticion_sin_sesion_da_401_con_el_motivo() {
        let motivo = "la cookie de sesión no es válida";
        let respuesta = AppError::Unauthorized(motivo.to_owned()).into_response();

        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);

        let texto = body_text(respuesta).await;

        // El 401 explica lo suyo —no es un 5xx— pero no deja el cuerpo vacío ni
        // redirige: la API contesta JSON y es el panel quien decide qué hacer.
        assert!(!texto.is_empty(), "el 401 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 401 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 401);
        assert!(
            cuerpo["message"]
                .as_str()
                .unwrap_or_default()
                .contains(motivo),
            "el 401 debe explicar el motivo: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn un_502_no_filtra_la_causa() {
        let respuesta = AppError::BadGateway(format!(
            "el proveedor de identidad no contesta: {CREDENCIAL}"
        ))
        .into_response();

        assert_eq!(respuesta.status(), StatusCode::BAD_GATEWAY);

        let texto = body_text(respuesta).await;

        // El 502 es de un servicio de terceros, pero es un 5xx: al cuerpo le rige
        // la misma regla que al fallo interno, y la causa se queda en el registro.
        assert!(
            !texto.contains(CREDENCIAL),
            "el 502 publicó el contenido de la causa: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 502 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 502);
        assert_eq!(
            cuerpo["message"],
            super::INTERNAL_MESSAGE,
            "el 502 debe traer el mismo mensaje genérico que el 500: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
    }

    /// 1.51 — Un fallo de base de datos no publica la causa.
    ///
    /// El mensaje de `DbError` nombra rutas a propósito —es lo que hace útil el
    /// log—, y es justo lo que no puede salir al cuerpo: una ruta del despliegue
    /// no es asunto de quien pregunta a la API.
    #[tokio::test]
    async fn un_error_de_base_no_filtra_la_causa() {
        let respuesta = AppError::Database(crate::db::DbError::DirectorioAusente {
            ruta: std::path::PathBuf::from(format!("/srv/{CREDENCIAL}/data/kesto.db")),
        })
        .into_response();

        assert_eq!(respuesta.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let texto = body_text(respuesta).await;

        assert!(
            !texto.contains(CREDENCIAL),
            "el 500 publicó la ruta de la causa: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 500 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 500);
        assert_eq!(
            cuerpo["message"],
            super::INTERNAL_MESSAGE,
            "el fallo de base debe traer el mensaje genérico: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
    }
}
