//! Rutas del flujo de entrada: `/api/v1/auth/*`.
//!
//! Aquí solo hay HTTP: la verificación de la sesión vive en `src/auth/`, y el
//! flujo OIDC también —el descubrimiento, la URL de autorización, el canje del
//! código y la verificación del ID token, todo en `src/auth/oidc.rs`, y la
//! sesión en `src/auth/session.rs`—. Este módulo traduce la petición en llamadas
//! a ese módulo y compone la respuesta: un `302` en los dos extremos del flujo
//! —`login` hacia el proveedor, `callback` de vuelta al panel— y la cookie de
//! sesión en el callback.

use axum::body::Body;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Extension, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::Deserialize;

use crate::api::response::{ApiResponse, Data};
use crate::auth::middleware::SessionIdentity;
use crate::auth::session::SESSION_COOKIE_NAME;
use crate::auth::{oidc, session};
use crate::error::AppError;
use crate::state::AppState;

/// Mensaje único con el que se rechaza un intento de entrada que no cuadra.
///
/// Los tres modos de fallo del `state` —desconocido, caducado y reutilizado— se
/// distinguen en el código y en el registro, pero comparten este mensaje: quien
/// no ha entrado no aprende cuál de los tres es el suyo.
const INTENTO_INVALIDO: &str = "el intento de entrada no es válido";

/// Mensaje con el que se rechaza un callback al que le faltan los parámetros del
/// flujo.
const CALLBACK_INCOMPLETO: &str = "el callback no trae los parámetros del flujo";

/// Destino de la vuelta tras el callback: la raíz del panel.
///
/// El panel (`ui/`) todavía no existe —llega con su propio change—, así que la
/// vuelta apunta a `/`, que es donde se servirá. Cuando el panel exista, esta
/// constante pasa a ser su ruta real; hoy la raíz es lo natural y no arrastra
/// ningún parámetro del flujo.
const DESTINO_PANEL: &str = "/";

/// `GET /api/v1/auth/login` — pública.
///
/// Redirige al `authorization_endpoint` del proveedor con los seis parámetros
/// del flujo, guarda el `state`/`nonce` emitidos y **no** emite sesión: la
/// cookie llega con el callback.
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

    // El `state` se guarda con su marca de tiempo y junto al `nonce` emitido: el
    // callback comparará ese `nonce` con el del ID token al verificar. La
    // respuesta no emite cookie: la sesión llega con el callback.
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
/// Retorno del proveedor: consume el `state`, canjea el código, verifica el ID
/// token y, si todo cuadra, emite la cookie de sesión y vuelve al panel. Todo
/// rechazo contesta la envoltura y **no** emite cookie.
///
/// La consulta se extrae **tipada** —`Result<Query<_>, QueryRejection>` y no
/// `Query<_>` a secas— para poder traducir el rechazo del extractor: servir el
/// que trae `Query` por defecto rompe las dos reglas de la envoltura. Sale con
/// `text/plain` —no con el JSON de `api-shell`— y con el detalle de serde, que
/// publica la estructura interna del struct («duplicate field `code`»). Y no es
/// un borde teórico: un `code` duplicado en la URL lo provoca cualquiera.
pub async fn callback(
    State(state): State<AppState>,
    params: Result<Query<CallbackParams>, QueryRejection>,
) -> Response {
    // La ruta solo se monta en modo `Oidc` (`api_router`), así que este camino
    // es inalcanzable por construcción. No se confía en ello.
    let Some(auth) = state.auth.as_ref() else {
        return AppError::Other("el callback se atendió sin estado de autenticación".to_owned())
            .into_response();
    };

    // Una consulta que el extractor no sabe leer es un callback mal formado, y se
    // contesta como tal: `400` con la envoltura y el mensaje genérico del flujo.
    // El detalle del rechazo —el de serde— no se traduce ni al cuerpo ni al
    // registro: nombra campos internos del struct y al cliente no le sirve.
    let Query(params) = match params {
        Ok(params) => params,
        Err(_) => {
            tracing::debug!("la consulta del callback no pasa el extractor");
            return AppError::InvalidInput(CALLBACK_INCOMPLETO.to_owned()).into_response();
        }
    };

    // Un `error` en la consulta es el proveedor diciendo que no: se rechaza sin
    // canjear el código —el `token_endpoint` no se toca—.
    if params.error.is_some() {
        return AppError::Unauthorized(INTENTO_INVALIDO.to_owned()).into_response();
    }

    let Some(state_recibido) = params.state.as_deref() else {
        return AppError::InvalidInput(CALLBACK_INCOMPLETO.to_owned()).into_response();
    };
    let Some(code) = params.code.as_deref() else {
        return AppError::InvalidInput(CALLBACK_INCOMPLETO.to_owned()).into_response();
    };

    // El `state` se consume: extraerlo del mapa y descartarlo. Un `state`
    // desconocido, caducado o reutilizado se distingue en el código —y en el
    // registro— pero contesta el mismo mensaje al cliente.
    let pendiente = match auth.consumir_login(state_recibido).await {
        Ok(pendiente) => pendiente,
        Err(rechazo) => {
            tracing::debug!(?rechazo, "callback con un state no utilizable");
            return AppError::Unauthorized(INTENTO_INVALIDO.to_owned()).into_response();
        }
    };

    let discovery = match oidc::discovery_cached(auth).await {
        Ok(discovery) => discovery,
        Err(error) => return error.into_response(),
    };

    let id_token = match oidc::canjear_codigo(auth.settings.as_ref(), &discovery, code).await {
        Ok(id_token) => id_token,
        Err(error) => return error.into_response(),
    };

    let identidad = match oidc::verificar_id_token(
        auth.settings.as_ref(),
        &discovery,
        &id_token,
        &pendiente.nonce,
    )
    .await
    {
        Ok(identidad) => identidad,
        Err(error) => return error.into_response(),
    };

    let token = match session::issue(&auth.secret, identidad.sub, identidad.email) {
        Ok(token) => token,
        Err(error) => return error.into_response(),
    };

    let cookie = match session::session_cookie(token, &auth.settings.redirect_url) {
        Ok(cookie) => cookie,
        Err(error) => return error.into_response(),
    };

    redireccion_302_al_panel(cookie)
}

/// `302` al panel con la cookie de sesión en `Set-Cookie`.
///
/// El destino es la raíz y no arrastra ningún parámetro: la `Location` no puede
/// llevar el código de autorización ni el `state`.
fn redireccion_302_al_panel(cookie: Cookie<'static>) -> Response {
    let mut respuesta = redireccion_302(DESTINO_PANEL);

    match HeaderValue::from_str(&cookie.to_string()) {
        Ok(valor) => {
            respuesta.headers_mut().insert(header::SET_COOKIE, valor);
            respuesta
        }
        // Una cookie que no cabe en una cabecera no es una sesión a medias: es
        // un fallo al construirla.
        Err(error) => AppError::Other(format!(
            "la cookie de sesión no es una cabecera válida: {error}"
        ))
        .into_response(),
    }
}

/// Parámetros de la consulta del callback.
///
/// Es `pub` porque el manejador que lo extrae también lo es: un parámetro de
/// tipo privado en una firma pública no compila.
#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    /// Código de autorización que devuelve el proveedor.
    code: Option<String>,
    /// `state` emitido en el login: la clave con la que se recupera el `nonce`.
    state: Option<String>,
    /// Error declarado por el proveedor (`access_denied`, por ejemplo).
    error: Option<String>,
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
        EMAIL, OIDC_CLIENT_ID, OIDC_CLIENT_SECRET, SUB, body_json, body_text, callback_url,
        capturar_log, cookie_de_sesion, emisor_caido, estado_oidc_con_emisor, get, get_with_cookie,
        header_text, login_state_y_nonce, parametros_de_consulta, post, post_with_cookie,
        proveedor_falso::ACCESS_TOKEN, proveedor_falso::IdToken, proveedor_falso::ProveedorFalso,
        router_oidc, router_oidc_con_emisor, session_cookie_header, token_de_set_cookie,
    };

    #[tokio::test]
    async fn el_login_guarda_el_state_con_su_nonce_y_su_marca_de_tiempo() {
        // Candado del SHALL «el state SHALL guardarse en memoria con su marca de
        // tiempo» —el contrato que el callback consume—. Sin esta prueba, ninguna
        // leía `auth.logins`: quitar la llamada a `registrar_login` dejaba la
        // suite entera verde, y el fallo habría aparecido como un callback que
        // rechaza *todos* los state y parece un bug del callback.
        let proveedor = ProveedorFalso::servir().await;
        let estado = estado_oidc_con_emisor(&proveedor.issuer).await;
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
        let respuesta = post_with_cookie(
            router_oidc().await,
            "/api/v1/auth/logout",
            &cookie_de_sesion(),
        )
        .await;

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
        let respuesta = post(router_oidc().await, "/api/v1/auth/logout").await;

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
        let respuesta = get_with_cookie(
            router_oidc().await,
            "/api/v1/auth/logout",
            &cookie_de_sesion(),
        )
        .await;

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
        let respuesta = get(router_oidc().await, "/api/v1/auth/logout").await;

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
        let respuesta = get_with_cookie(
            router_oidc().await,
            "/api/v1/auth/session",
            &cookie_de_sesion(),
        )
        .await;

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
        let respuesta = get(router_oidc().await, "/api/v1/auth/session").await;

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
        let router = router_oidc_con_emisor(&emisor_caido()).await;

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

    #[tokio::test]
    async fn el_callback_correcto_contesta_302() {
        // Escenario 1.26: tras emitir la cookie, el callback vuelve al panel con
        // un `302`. La `Location` no puede arrastrar el código de autorización
        // (escenario «La vuelta al panel no arrastra el código»).
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer).await;

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let claims = IdToken::valido(&proveedor.issuer, OIDC_CLIENT_ID, &nonce);
        proveedor
            .montar_token_endpoint(&proveedor.firmar(&claims))
            .await;

        let respuesta = get(router, &callback_url(&state, "codigo-de-autorizacion")).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::FOUND,
            "el callback correcto debe volver al panel"
        );

        let location =
            header_text(&respuesta, header::LOCATION).expect("el 302 debe traer Location");

        assert!(
            !location.contains("codigo-de-autorizacion"),
            "la vuelta al panel no puede arrastrar el código de autorización: {location}"
        );
        assert!(
            token_de_set_cookie(&respuesta).is_some(),
            "el callback correcto debe emitir la cookie de sesión"
        );
    }

    #[tokio::test]
    async fn una_consulta_de_callback_duplicada_contesta_la_envoltura_json() {
        // A1. El rechazo del extractor `Query` se sirve antes que este handler, y
        // con `Query<CallbackParams>` a secas rompía las dos reglas de
        // `api-shell`: salía `text/plain` —no la envoltura— y con el detalle de
        // serde, que publica la estructura interna del struct. Un `code`
        // duplicado lo provoca cualquier cliente, no es un borde teórico.
        let respuesta = get(router_oidc().await, "/api/v1/auth/callback?code=a&code=b").await;

        assert_eq!(
            respuesta.status(),
            StatusCode::BAD_REQUEST,
            "una consulta ilegible debe rechazarse con un 400"
        );

        let tipo = header_text(&respuesta, header::CONTENT_TYPE)
            .expect("la respuesta debe traer content-type");

        assert!(
            tipo.starts_with("application/json"),
            "el rechazo no puede volver con el text/plain del extractor: {tipo}"
        );
        assert!(
            !tipo.contains("text/plain"),
            "el rechazo volvió con text/plain en vez de la envoltura: {tipo}"
        );

        let texto = body_text(respuesta).await;

        assert!(
            !texto.contains("duplicate field"),
            "el 400 publicó el detalle de serde: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 400 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 400);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 400 debe traer la envoltura con message: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el rechazo no debe traer la clave data: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn un_callback_sin_parametros_contesta_400_envuelto_y_no_un_401() {
        // El otro lado de A1: una consulta **ausente** no es un rechazo del
        // extractor —todos los campos son `Option`, así que se leen vacíos—, y el
        // handler contesta su `400`. El escenario 1.4 de `src/api/mod.rs` depende
        // de que esta ruta no conteste `401`, y por eso se deja candado aquí.
        let respuesta = get(router_oidc().await, "/api/v1/auth/callback").await;

        assert_ne!(
            respuesta.status(),
            StatusCode::UNAUTHORIZED,
            "el callback sin parámetros no puede contestar 401"
        );
        assert_eq!(respuesta.status(), StatusCode::BAD_REQUEST);

        let tipo = header_text(&respuesta, header::CONTENT_TYPE)
            .expect("la respuesta debe traer content-type");

        assert!(
            tipo.starts_with("application/json"),
            "el 400 debe llevar la envoltura JSON: {tipo}"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 400);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 400 debe traer la envoltura con message: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn el_flujo_completo_no_deja_credenciales_en_el_log() {
        // Escenario 1.38 — un SHALL del requisito «Ninguna credencial del
        // proveedor acaba en el log ni en la cookie». Con el registro a `trace`,
        // el flujo no puede dejar en el log ninguna de las credenciales que
        // maneja. Se ejercitan los **dos** caminos, porque la fuga puede estar en
        // cualquiera de ellos:
        //
        //   * el de **error** del canje, que es el único sitio por el que un
        //     texto ajeno entra al registro —`tracing::error!(error = %causa, …)`
        //     de `AppError::BadGateway`, con la causa del proveedor interpolada—,
        //     y
        //   * el **feliz**: login, callback, sesión emitida y una petición con
        //     ella.
        //
        // **Por qué las dos primeras aserciones son sobre líneas del flujo.** La
        // marca de control que había aquí —un `tracing::trace!` lanzado desde
        // este hilo— probaba que la captura estaba en pie, pero **no** que lo que
        // el flujo registra en su propio contexto llegara al buffer: si el
        // manejador se despachara en otro hilo, el flujo registraría fuera del
        // buffer de éste —el suscriptor es global, pero el buffer de la captura
        // es **de hilo**—, la marca seguiría apareciendo y las cuatro búsquedas
        // de abajo pasarían por vacío, en verde. Lo que se afirma ahora son dos
        // líneas que emite el propio flujo ejercitado —una del callback, otra de
        // la traducción del `502`—, así que el día que la captura deje de
        // alcanzar al flujo, este test cae.
        let (buffer, _guard) = capturar_log();

        // ── Un rechazo del propio callback ───────────────────────────────────
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer).await;

        let rechazo = get(
            router.clone(),
            &callback_url("state-que-nadie-emitio", "codigo-inventado"),
        )
        .await;

        assert_eq!(
            rechazo.status(),
            StatusCode::UNAUTHORIZED,
            "un state que nadie emitió debe rechazarse"
        );

        // ── El camino de error del canje, con un cuerpo ajeno ────────────────
        let proveedor_caido = ProveedorFalso::servir().await;
        let router_caido = router_oidc_con_emisor(&proveedor_caido.issuer).await;
        // El cuerpo del `502` es hostil a propósito: lleva credenciales de verdad,
        // para que registrarlo quedara en evidencia. Kesto ni lo lee —solo mira
        // el código de estado—, así que lo que aquí se fija es que no lo lea
        // nadie y, sobre todo, que no se registre.
        let cuerpo_ajeno = format!(
            "{{\"error\":\"invalid_grant\",\"client_secret\":\"{OIDC_CLIENT_SECRET}\",\
             \"access_token\":\"{ACCESS_TOKEN}\"}}"
        );
        // 502: el proveedor contesta un error al canje del código.
        proveedor_caido
            .montar_token_endpoint_con_error(502, &cuerpo_ajeno)
            .await;

        let (state_del_error, _nonce) = login_state_y_nonce(router_caido.clone()).await;
        let codigo_del_error = "codigo-del-502-distinguible";
        let caida = get(
            router_caido,
            &callback_url(&state_del_error, codigo_del_error),
        )
        .await;

        assert_eq!(
            caida.status(),
            StatusCode::BAD_GATEWAY,
            "un proveedor que contesta un error al canje debe dejar el callback en un 502"
        );
        assert!(
            token_de_set_cookie(&caida).is_none(),
            "un 502 no puede emitir cookie de sesión"
        );

        // La causa del 502 no sale al cuerpo: la respuesta es la envoltura con el
        // mensaje genérico de todo `5xx`.
        let texto_caida = body_text(caida).await;

        assert!(
            !texto_caida.contains(OIDC_CLIENT_SECRET),
            "el 502 del callback publicó el client_secret en el cuerpo: {texto_caida}"
        );

        // ── El camino feliz, entero ──────────────────────────────────────────
        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let id_token =
            proveedor.firmar(&IdToken::valido(&proveedor.issuer, OIDC_CLIENT_ID, &nonce));
        proveedor.montar_token_endpoint(&id_token).await;

        let codigo = "codigo-de-autorizacion-distinguible";
        let respuesta = get(router.clone(), &callback_url(&state, codigo)).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::FOUND,
            "el flujo debe completarse para que el log sea el de un flujo real"
        );

        let jwt = token_de_set_cookie(&respuesta).expect("el callback debe emitir la sesión");
        let sesion =
            get_with_cookie(router, "/api/v1/auth/session", &session_cookie_header(&jwt)).await;

        assert_eq!(sesion.status(), StatusCode::OK);

        let log = buffer.texto();

        // Que la captura esté en pie lo prueba el propio **flujo**, no este hilo:
        // el rechazo del callback y la traducción del `502` —con la causa del
        // proveedor dentro—. Si el flujo registrara fuera del alcance del
        // suscriptor, esto cae, y con ello deja de poder pasar por vacío lo de
        // abajo.
        assert!(
            log.contains("callback con un state no utilizable"),
            "la captura no está en pie: el rechazo del callback no quedó registrado: {log}"
        );
        assert!(
            log.contains("un servicio de terceros no contestó")
                && log.contains(
                    "el proveedor de identidad contestó 502 Bad Gateway al canje del código"
                ),
            "la captura no está en pie: la causa del 502 no quedó registrada: {log}"
        );

        for (nombre, credencial) in [
            ("client_secret", OIDC_CLIENT_SECRET),
            ("código de autorización", codigo),
            (
                "código de autorización del camino de error",
                codigo_del_error,
            ),
            ("ID token", id_token.as_str()),
            ("token de acceso", ACCESS_TOKEN),
        ] {
            assert!(
                !log.contains(credencial),
                "el log publicó el {nombre}: {log}"
            );
        }

        // Y el payload del ID token, además del token entero: es el fragmento que
        // un error de deserialización podría arrastrar.
        let payload = id_token
            .split('.')
            .nth(1)
            .expect("un JWT tiene tres partes");

        assert!(
            !log.contains(payload),
            "el log publicó el payload del ID token: {log}"
        );
    }
}
