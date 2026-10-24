//! El *gate* de la superficie protegida: sin cookie de sesión válida no se pasa.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;

use crate::auth::session::{self, SESSION_COOKIE_NAME};
use crate::error::AppError;
use crate::state::AuthState;

/// Identidad de la sesión ya verificada, tal y como la ve un manejador.
///
/// Viaja en las *extensions* de la petición —el camino natural de axum para
/// pasar datos de una capa al manejador— y no vuelve a leerse de la cookie: el
/// manejador no tiene por qué saber que la identidad viaja dentro de un JWT, ni
/// tiene por qué verificarlo otra vez.
///
/// Lleva **sólo** `sub` y `email`, no los `iat`/`exp` de
/// [`crate::auth::session::Claims`]: la vigencia ya la comprobó el *gate*, y
/// exponerla invitaría a reimplementar esa comprobación en cada manejador.
#[derive(Debug, Clone)]
pub struct SessionIdentity {
    /// Identificador del usuario en el proveedor de identidad.
    pub sub: String,
    /// Correo del usuario.
    pub email: String,
}

/// Motivo con el que se rechaza una petición que no trae cookie de sesión.
const SIN_SESION: &str = "hace falta una sesión válida para esta ruta";

/// *Gate* de la superficie protegida.
///
/// Extrae la cookie de sesión, verifica su firma y su vigencia con la clave del
/// estado y, sin cookie válida, contesta `401` con la envoltura
/// `{status, message}` —nunca con el cuerpo vacío, nunca con HTML y nunca con
/// una redirección: la API contesta JSON y es el panel quien decide qué hacer—.
///
/// Con cookie válida deja la identidad en las *extensions* de la petición y pasa
/// al manejador.
pub async fn gate(
    State(auth): State<AuthState>,
    jar: CookieJar,
    mut request: Request,
    next: Next,
) -> Response {
    let identidad = match jar.get(SESSION_COOKIE_NAME) {
        Some(cookie) => match session::verify(&auth.secret, cookie.value()) {
            Ok(claims) => SessionIdentity {
                sub: claims.sub,
                email: claims.email,
            },
            // El rechazo no distingue «no hay cookie» de «la cookie no vale»:
            // decir cuál de las dos es contaría a quien no ha entrado si su
            // token caducó o si la firma no cuadra. El motivo concreto se queda
            // en el registro, dentro de `session::verify`.
            Err(error) => return error.into_response(),
        },
        None => return AppError::Unauthorized(SIN_SESION.to_owned()).into_response(),
    };

    // La identidad viaja en las *extensions*: el manejador la lee de ahí y no
    // vuelve a tocar la cookie.
    request.extensions_mut().insert(identidad);

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::extract::Extension;
    use axum::http::StatusCode;

    use super::SessionIdentity;
    use crate::api::protected_router;
    use crate::api::response::{ApiResponse, Data};
    use crate::auth::session;
    use crate::secret::SecretString;
    use crate::test_support::{
        EMAIL, OTRA_SESSION_SECRET, SUB, auth_oidc, body_json, body_text, cookie_de_sesion, get,
        get_with_cookie, session_cookie_header,
    };

    /// Ruta ficticia de negocio, colgada de la mitad protegida como lo estará
    /// cualquier ruta real el día que su change la declare.
    const RUTA: &str = "/negocio";

    /// Manejador que devuelve la identidad que el *gate* dejó en las
    /// *extensions*: es lo que prueba que el manejador la ve sin leer la cookie.
    async fn quien_soy(Extension(identidad): Extension<SessionIdentity>) -> ApiResponse {
        ApiResponse::new(
            StatusCode::OK,
            "sesión válida",
            Data::Some(serde_json::json!({
                "sub": identidad.sub,
                "email": identidad.email,
            })),
        )
    }

    /// La mitad protegida de producción con una ruta de negocio ficticia.
    fn router() -> Router {
        protected_router(
            Router::new().route(RUTA, axum::routing::get(quien_soy)),
            &auth_oidc(),
        )
    }

    #[tokio::test]
    async fn una_peticion_protegida_sin_cookie_contesta_401_con_la_envoltura() {
        // Escenario 1.27.
        let respuesta = get(router(), RUTA).await;

        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 401 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 401 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 401);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 401 debe traer un mensaje: {texto}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {texto}"
        );
    }

    #[tokio::test]
    async fn una_cookie_valida_deja_pasar_y_el_manejador_ve_la_identidad() {
        // Escenario 1.31.
        let respuesta = get_with_cookie(router(), RUTA, &cookie_de_sesion()).await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["sub"], SUB,
            "el manejador no vio el usuario de la sesión: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["email"], EMAIL,
            "el manejador no vio el correo de la sesión: {cuerpo}"
        );
        // Y sin volver a leer la cookie: la única fuente del manejador son las
        // *extensions* que dejó el *gate*.
        assert!(
            cuerpo.to_string().contains(SUB),
            "la respuesta no trae la identidad: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn una_cookie_firmada_con_otra_clave_no_se_acepta() {
        // Escenario «una cookie firmada con otra clave no se acepta» de
        // `specs/auth/spec.md`, esta vez atravesando el *gate*: el rechazo no
        // sale del verificador sino de quien lo llama, y hay que saber que ese
        // camino devuelve el 401 de la envoltura y no pasa al manejador.
        let token = session::issue(
            &SecretString::new(OTRA_SESSION_SECRET.to_owned()),
            SUB.to_owned(),
            EMAIL.to_owned(),
        )
        .expect("la emisión de la sesión de prueba no puede fallar");

        let respuesta = get_with_cookie(router(), RUTA, &session_cookie_header(&token)).await;

        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 401);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 401 debe traer un mensaje: {cuerpo}"
        );
    }
}
