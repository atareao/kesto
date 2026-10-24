//! Rutas del flujo de entrada: `/api/v1/auth/*`.
//!
//! Aquí solo hay HTTP: la verificación de la sesión vive en `src/auth/`, y el
//! flujo OIDC vive repartido —el descubrimiento y la URL de autorización ya en
//! `src/auth/oidc.rs`; el canje del código y la verificación del ID token,
//! todavía por llegar en M4—. De los dos endpoints del flujo, **`login` ya no es
//! un esqueleto**: solo `callback` lo sigue siendo.

use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar};

use crate::api::response::{ApiResponse, Data, ErrorResponse};
use crate::auth::middleware::SessionIdentity;
use crate::auth::oidc;
use crate::auth::session::SESSION_COOKIE_NAME;
use crate::error::AppError;
use crate::state::AppState;

/// Mensaje de un esqueleto: la ruta existe, el flujo todavía no.
///
/// Es constante y no lleva causa ninguna —un `5xx` no la cuenta—, así que no
/// hay nada que redactar y no se inventa una variante de error que solo sirva
/// para decir «aún no».
const SIN_IMPLEMENTAR: &str = "la ruta existe, pero su flujo todavía no está implementado";

/// `GET /api/v1/auth/login` — pública.
///
/// Redirige al `authorization_endpoint` del proveedor con los seis parámetros
/// del flujo, guarda el `state`/`nonce` emitidos y **no** emite sesión: la
/// cookie llega con el callback (M4).
pub async fn login(State(state): State<AppState>) -> Response {
    // La ruta solo se monta en modo `Oidc` (`api_router`), así que este camino
    // es inalcanzable por construcción. No se confía en ello: sin estado de
    // autenticación se contesta el `5xx` genérico en vez de un `panic`.
    let Some(auth) = state.auth.as_ref() else {
        return AppError::Other("el login se atendió sin estado de autenticación".to_owned())
            .into_response();
    };

    // El descubrimiento cacheado o su reintento. Si falla, el error ya es un
    // `BadGateway`, que traduce a `502` con la envoltura y el mensaje genérico.
    let discovery = match oidc::discovery_cached(auth).await {
        Ok(discovery) => discovery,
        Err(error) => return error.into_response(),
    };

    let solicitud = oidc::build_authorization_url(auth.settings.as_ref(), &discovery);

    // El `state` se guarda con su marca de tiempo y junto al `nonce` emitido:
    // M4 tendrá que comparar ese `nonce` con el del ID token. La respuesta no
    // emite cookie: la sesión llega con el callback.
    auth.registrar_login(&solicitud.state, &solicitud.nonce)
        .await;

    redireccion_302(&solicitud.url)
}

/// Respuesta `302` con la cabecera `Location`.
///
/// Se construye a mano porque `axum::response::Redirect` no ofrece `302`
/// —`to` es `303`, y el flujo de entrada exige `302` literal—. La respuesta
/// lleva solo `Location`, sin `Set-Cookie`: el login no establece sesión.
fn redireccion_302(destino: &str) -> Response {
    match HeaderValue::from_str(destino) {
        Ok(location) => {
            let mut respuesta = Response::new(Body::empty());
            *respuesta.status_mut() = StatusCode::FOUND;
            respuesta.headers_mut().insert(header::LOCATION, location);

            respuesta
        }
        // Una URL que no cabe en una cabecera no es un destino a medias: es un
        // fallo al construirla.
        Err(error) => AppError::Other(format!(
            "la URL de autorización no es una cabecera válida: {error}"
        ))
        .into_response(),
    }
}

/// `GET /api/v1/auth/callback` — pública.
///
/// En M2 es un esqueleto: el retorno del proveedor es M4.
pub async fn callback() -> ErrorResponse {
    ErrorResponse::new(StatusCode::NOT_IMPLEMENTED, SIN_IMPLEMENTAR)
}

/// `POST /api/v1/auth/logout` — protegida.
///
/// Borra la cookie de sesión —la **misma** cookie, con el mismo `Path` y con
/// `Max-Age=0`— y contesta la envoltura. Se declara como `POST` y no como `GET`
/// porque cambia estado: un `GET` lo dispararían un enlace, un prefetch o una
/// imagen.
pub async fn logout(jar: CookieJar) -> (CookieJar, ApiResponse) {
    // La `Set-Cookie` de borrado viaja por el propio frasco de la petición: es
    // lo que hace que la respuesta lleve la cabecera y la envoltura a la vez.
    let jar = jar.add(cookie_de_borrado());

    (
        jar,
        ApiResponse::new(StatusCode::OK, "sesión cerrada", Data::None),
    )
}

/// `GET /api/v1/auth/session` — protegida.
///
/// Contesta la identidad de la sesión —su `sub` y su `email`— sin devolver el
/// JWT. Sin cookie válida, el `401` del *gate* **es** la respuesta a «¿hay
/// sesión?».
pub async fn session(Extension(identidad): Extension<SessionIdentity>) -> ApiResponse {
    // La identidad entra por las *extensions* que dejó el *gate*: aquí no se
    // lee la cookie ni se verifica el JWT otra vez, y el token no se devuelve.
    ApiResponse::new(
        StatusCode::OK,
        "sesión en vigor",
        Data::Some(serde_json::json!({
            "sub": identidad.sub,
            "email": identidad.email,
        })),
    )
}

/// Cookie de borrado: la misma cookie, mismo `Path` y `Max-Age=0`.
///
/// Se construye con `make_removal` en vez de con `CookieJar::remove` a
/// propósito: `remove` solo emite la `Set-Cookie` si la petición traía esa
/// cookie, y el borrado no puede depender de que la cookie esté —el `logout`
/// responde igual aunque el cliente la haya perdido por el camino—.
fn cookie_de_borrado() -> Cookie<'static> {
    let mut cookie = Cookie::build((SESSION_COOKIE_NAME, "")).path("/").build();
    cookie.make_removal();
    cookie
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;

    use crate::api::api_router;
    use crate::auth::session::SESSION_COOKIE_NAME;
    use crate::error::AppError;
    use crate::test_support::{
        EMAIL, OIDC_CLIENT_SECRET, SUB, body_json, body_text, cookie_de_sesion, emisor_caido,
        estado_oidc_con_emisor, get, get_with_cookie, header_text, parametros_de_consulta, post,
        post_with_cookie, proveedor_falso::ProveedorFalso, router_oidc, router_oidc_con_emisor,
    };

    #[tokio::test]
    async fn el_login_guarda_el_state_con_su_nonce_y_su_marca_de_tiempo() {
        // Candado del SHALL «el state SHALL guardarse en memoria con su marca de
        // tiempo» —el contrato que M4, el callback, va a consumir—. Hasta ahora
        // ninguna prueba leía `auth.logins`: quitar la llamada a `registrar_login`
        // dejaba la suite entera verde, y el fallo habría aparecido en M4 como un
        // callback que rechaza *todos* los state y parece un bug del callback.
        let proveedor = ProveedorFalso::servir().await;
        let estado = estado_oidc_con_emisor(&proveedor.issuer);
        // El mapa vive tras un `Arc`, así que esta copia comparte el mismo mapa
        // que usa el router: no hace falta deformar el estado para poder mirarlo.
        let auth = estado
            .auth
            .clone()
            .expect("en modo Oidc hay estado de autenticación");
        let router = api_router(estado);

        let antes = Instant::now();
        let respuesta = get(router, "/api/v1/auth/login").await;
        let despues = Instant::now();

        assert_eq!(
            respuesta.status(),
            StatusCode::FOUND,
            "el login debe redirigir"
        );

        let location = header_text(&respuesta, header::LOCATION).expect("el 302 trae Location");
        let parametros = parametros_de_consulta(&location);
        let state = parametros
            .get("state")
            .expect("la redirección debe traer state")
            .clone();
        let nonce = parametros
            .get("nonce")
            .expect("la redirección debe traer nonce")
            .clone();

        let logins = auth.logins.lock().await;
        let entrada = logins
            .get(&state)
            .expect("el state emitido debe quedar guardado en el mapa");

        assert_eq!(
            entrada.nonce, nonce,
            "el nonce guardado no es el emitido junto al state: {entrada:?}"
        );
        assert!(
            entrada.issued_at >= antes && entrada.issued_at <= despues,
            "la marca de tiempo guardada no es la del login: {entrada:?}"
        );
        assert!(
            !entrada.expirado_en(Instant::now()),
            "un state recién emitido no puede estar caducado: {entrada:?}"
        );
    }

    #[tokio::test]
    async fn el_cierre_borra_la_cookie() {
        // Escenario 1.33.
        let respuesta =
            post_with_cookie(router_oidc(), "/api/v1/auth/logout", &cookie_de_sesion()).await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cabecera = header_text(&respuesta, header::SET_COOKIE)
            .expect("el logout debe traer una Set-Cookie de borrado");

        assert!(
            cabecera.starts_with(&format!("{SESSION_COOKIE_NAME}=;")),
            "la cookie de borrado debe dejar el valor vacío: {cabecera}"
        );
        assert!(
            cabecera.contains("Max-Age=0"),
            "la cookie de borrado debe llevar Max-Age=0: {cabecera}"
        );
        assert!(
            cabecera.contains("Path=/"),
            "la cookie de borrado debe llevar el mismo Path que la de sesión: {cabecera}"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 200);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el logout debe contestar la envoltura con message: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn sin_sesion_el_cierre_no_se_atiende() {
        // Escenario 1.34.
        let respuesta = post(router_oidc(), "/api/v1/auth/logout").await;

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

    #[tokio::test]
    async fn el_cierre_no_se_declara_como_get() {
        // Escenario 1.35.
        let respuesta =
            get_with_cookie(router_oidc(), "/api/v1/auth/logout", &cookie_de_sesion()).await;

        assert_eq!(respuesta.status(), StatusCode::METHOD_NOT_ALLOWED);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 405);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 405 debe traer un mensaje: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn sin_sesion_no_se_aprende_que_metodos_hay() {
        // Escenario 1.45.
        let respuesta = get(router_oidc(), "/api/v1/auth/logout").await;

        assert_ne!(
            respuesta.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "sin sesión, el 405 revelaría qué métodos existen"
        );
        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 401 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 401 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 401);
    }

    #[tokio::test]
    async fn con_sesion_se_contesta_la_identidad() {
        // Escenario 1.36.
        let respuesta =
            get_with_cookie(router_oidc(), "/api/v1/auth/session", &cookie_de_sesion()).await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 200);
        assert_eq!(
            cuerpo["data"]["sub"], SUB,
            "la sesión debe contestar el sub: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["email"], EMAIL,
            "la sesión debe contestar el email: {cuerpo}"
        );
        // El JWT no se devuelve: la cookie es su único sitio.
        assert!(
            !cuerpo.to_string().contains("eyJ"),
            "la sesión no debe devolver el JWT: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn sin_sesion_el_401_es_la_respuesta() {
        // Escenario 1.37.
        let respuesta = get(router_oidc(), "/api/v1/auth/session").await;

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
    }

    #[tokio::test]
    async fn un_login_con_el_proveedor_caido_contesta_502() {
        // Escenario 1.10.
        //
        // El 502 y no un 500: el fallo es de un servicio de terceros —el
        // proveedor no contesta—, no de Kesto. Y como todo `5xx`, el cuerpo
        // lleva el mensaje genérico: la causa se queda en el registro del
        // servidor y no se estrena un mensaje que nombre al proveedor.
        let router = router_oidc_con_emisor(&emisor_caido());

        let respuesta = get(router, "/api/v1/auth/login").await;

        assert_eq!(
            respuesta.status(),
            StatusCode::BAD_GATEWAY,
            "un proveedor caído debe dejar el login en un 502"
        );

        let texto = body_text(respuesta).await;

        assert!(
            !texto.contains(OIDC_CLIENT_SECRET),
            "el 502 publicó el client_secret: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 502 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 502);

        // El mensaje es **el mismo** de cualquier `5xx`: se compara con el de un
        // 500 genérico, no con un literal, para que no baste con «algo que
        // suene genérico».
        let generico = body_json(
            AppError::Other("una causa distinguible que no debe salir al cuerpo".to_owned())
                .into_response(),
        )
        .await;

        assert_eq!(
            cuerpo["message"], generico["message"],
            "el 502 debe traer el mismo mensaje genérico que un 500: {cuerpo}"
        );
    }
}
