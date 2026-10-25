//! Extracción del cuerpo JSON con el rechazo traducido a la envoltura de la API.

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};

use crate::error::AppError;

/// Cuerpo JSON cuyo rechazo sale por la envoltura de la API.
///
/// Envuelve a [`axum::Json`] para quedarse con **todo** su camino real de extracción
/// —la comprobación del tipo de contenido, la lectura del cuerpo y la deserialización—
/// y cambiar solo lo que pasa cuando falla. Merece la pena porque el rechazo por defecto
/// de axum describe el fallo con el texto del intérprete de `serde`, y ese texto **ecoa
/// el valor del campo** —medido: `invalid type: string "AKIA…", expected struct …`—, sale
/// en `text/plain` y con el código del intérprete, que no es la frontera que el contrato
/// fija. Aquí el fallo sale por donde sale todo lo demás: `400` con la envoltura
/// `{status, message}`.
///
/// El mensaje es **nuestro**, elegido por variante, y no publica el texto del intérprete.
/// Tampoco se registra ese texto: el `serde_path_to_error` que axum usa por dentro mete el
/// valor del campo en su `Display`, así que registrarlo sería escribir credenciales en el
/// log del contenedor. Lo que sí se registra es **que** una petición no se pudo
/// interpretar, sin su texto, que es lo que permite saber que hay un cliente mandando algo
/// raro sin ensuciar el registro con lo que no puede estar en él.
pub struct Json<T>(pub T);

impl<S, T> FromRequest<S> for Json<T>
where
    axum::Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(valor)) => Ok(Self(valor)),
            Err(rechazo) => Err(traducir(rechazo)),
        }
    }
}

/// Traduce el rechazo del extractor de axum a la frontera del `4xx`.
///
/// El mensaje se elige por variante y **nunca** es el `Display` del rechazo: es lo que
/// garantiza que el valor del campo no viaje al cliente. Se distinguen las variantes para
/// que el mensaje sirva de algo —decir «no es JSON» y decir «los tipos no encajan» mandan
/// a mirar sitios distintos—, que es la misma razón por la que el contrato quiere un
/// mensaje propio y no el del intérprete.
///
/// El tipo de contenido ausente también contesta `400`: axum lo rechazaría con `415`, pero
/// esta familia —lo que no se puede interpretar— tiene un solo código en el contrato, y el
/// panel se ahorra una rama.
fn traducir(rechazo: JsonRejection) -> AppError {
    let motivo = match rechazo {
        JsonRejection::MissingJsonContentType(_) => {
            "la petición no declara un tipo de contenido JSON"
        }
        JsonRejection::JsonSyntaxError(_) => "el cuerpo no es JSON válido",
        JsonRejection::JsonDataError(_) => "el cuerpo no tiene los campos que la ruta espera",
        JsonRejection::BytesRejection(_) => "el cuerpo de la petición no se pudo leer",
        // `JsonRejection` está declarado `#[non_exhaustive]`: una variante nueva de axum
        // cae aquí y sale por el mismo camino, en vez de romper la compilación.
        _ => "el cuerpo de la petición no se pudo interpretar",
    };

    // Se registra **que** falló y de qué clase, nunca el rechazo: su `Display` lleva el
    // valor del campo dentro.
    tracing::warn!(motivo, "no se pudo interpretar el cuerpo de la petición");

    AppError::InvalidInput(format!(
        "no se pudo interpretar el cuerpo de la petición: {motivo}"
    ))
}
