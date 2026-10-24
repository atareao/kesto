//! Flujo OIDC: descubrimiento del proveedor, URL de autorización y `state`/`nonce`.
//!
//! El canje del código en el `token_endpoint` y la verificación del ID token
//! contra el `jwks_uri` llegan con M4; aquí vive todo lo que el `login` necesita
//! para redirigir al proveedor, más el descubrimiento que el arranque intenta.

use std::time::Duration;

use serde::Deserialize;

use crate::config::OidcSettings;
use crate::error::AppError;
use crate::state::AuthState;

/// Ruta del documento de descubrimiento, relativa al emisor.
const RUTA_DESCUBRIMIENTO: &str = "/.well-known/openid-configuration";

/// Tiempo máximo que se espera al proveedor en el descubrimiento.
///
/// Un proveedor que acepta la conexión y no contesta no puede colgar el login
/// para siempre: el arranque ya tolera que el proveedor esté caído, y el primer
/// login tiene que fallar en un tiempo acotado en vez de quedarse esperando.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// Scopes que se piden al proveedor.
///
/// Incluye `openid` —sin él no hay ID token— y `email`, porque el `email` de la
/// sesión es un claim del ID token y la mayoría de los proveedores solo lo
/// publican con ese scope.
pub const SCOPE: &str = "openid email";

/// Longitud del `state` y del `nonce`: 32 caracteres aleatorios.
pub const TOKEN_LENGTH: usize = 32;

/// Documento de descubrimiento, con lo que de verdad se usa.
///
/// `userinfo_endpoint` **no** se declara a propósito: el ID token verificado
/// sobra y no se llama a `userinfo`, así que exigirlo rompería proveedores que
/// no lo publican. Serde ignora las claves que no estén aquí, de modo que el
/// resto del documento no molesta.
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    /// Endpoint al que se redirige al usuario para autorizar.
    pub authorization_endpoint: String,
    /// Endpoint del canje del código por los tokens (M4).
    pub token_endpoint: String,
    /// Endpoint del JWKS contra el que se verifica el ID token (M4).
    pub jwks_uri: String,
}

/// URL del documento de descubrimiento del emisor dado.
///
/// El `/` final del emisor se recorta para no construir `//.well-known/...`.
pub fn discovery_url(issuer: &str) -> String {
    format!("{}{RUTA_DESCUBRIMIENTO}", issuer.trim_end_matches('/'))
}

/// Pide el documento de descubrimiento al proveedor.
///
/// Cualquier desenlace que no sea un documento válido —no contestar, contestar
/// un error o contestar algo que no es el documento— es un
/// [`AppError::BadGateway`]: el fallo es de un servicio de terceros, no de
/// Kesto. La causa viaja en el error para que acabe en el log, nunca en el
/// cuerpo de la respuesta.
pub async fn fetch_discovery(settings: &OidcSettings) -> Result<Discovery, AppError> {
    let url = discovery_url(&settings.issuer);

    // El cliente se construye aquí y no se guarda en el estado: el
    // descubrimiento queda cacheado, así que esto ocurre una vez, y así el
    // estado no arrastra un campo que solo se usa en un sitio. El `timeout`
    // acota la espera para que un proveedor que acepta la conexión y no
    // contesta no cuelgue el login.
    let cliente = reqwest::Client::builder()
        .timeout(DISCOVERY_TIMEOUT)
        .build()
        .map_err(|error| {
            AppError::BadGateway(format!(
                "no se pudo preparar el cliente del descubrimiento: {error}"
            ))
        })?;

    let respuesta = cliente.get(&url).send().await.map_err(|error| {
        AppError::BadGateway(format!(
            "el proveedor de identidad no respondió al descubrimiento: {error}"
        ))
    })?;

    if !respuesta.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "el proveedor de identidad contestó {} al descubrimiento",
            respuesta.status()
        )));
    }

    respuesta.json::<Discovery>().await.map_err(|error| {
        AppError::BadGateway(format!(
            "el documento de descubrimiento no es válido: {error}"
        ))
    })
}

/// Devuelve el descubrimiento cacheado o lo pide y lo deja cacheado.
///
/// La caché vive en el estado y es segura entre tareas: el servidor es
/// concurrente y varios logins pueden mirarla a la vez. Se lee primero bajo un
/// candado de lectura y solo se escribe la primera vez que llega un resultado,
/// de modo que el segundo login —y el siguiente— no vuelven a pedir el
/// documento. Dos logins simultáneos sobre una caché vacía podrían pedirlo los
/// dos; el requisito es no repetirlo en cada intento de entrada, no una
/// exclusión mutua sobre la primera petición, y forzarla serializaría el login
/// entero para ahorrar una única petición redundante.
pub async fn discovery_cached(auth: &AuthState) -> Result<Discovery, AppError> {
    if let Some(discovery) = auth.discovery.read().await.clone() {
        return Ok(discovery);
    }

    let discovery = fetch_discovery(auth.settings.as_ref()).await?;

    *auth.discovery.write().await = Some(discovery.clone());

    Ok(discovery)
}

/// Una petición de autorización ya construida.
#[derive(Debug, Clone)]
pub struct AuthorizationRequest {
    /// URL completa a la que redirigir al usuario.
    pub url: String,
    /// `state` emitido: la clave con la que M4 consumirá este login.
    pub state: String,
    /// `nonce` emitido: viaja al proveedor para compararlo con el del ID token.
    pub nonce: String,
}

/// Construye la URL de autorización y genera el `state` y el `nonce`.
///
/// Los valores añadidos a la URL van codificados. El `state` y el `nonce` son
/// de 32 caracteres aleatorios y distintos en cada petición.
pub fn build_authorization_url(
    settings: &OidcSettings,
    discovery: &Discovery,
) -> AuthorizationRequest {
    let state = token_aleatorio();
    let nonce = token_aleatorio();

    let parametros = [
        ("response_type", "code"),
        ("client_id", settings.client_id.as_str()),
        ("redirect_uri", settings.redirect_url.as_str()),
        ("scope", SCOPE),
        ("state", state.as_str()),
        ("nonce", nonce.as_str()),
    ];

    // Cada valor va codificado: el `redirect_uri` lleva `:` y `/`, que sin
    // codificar romperían la URL; y aunque hoy el `state` y el `nonce` sean
    // alfanuméricos, codificarlos es lo que hace que el flujo siga siendo
    // correcto si algún día dejan de serlo.
    let consulta = parametros
        .iter()
        .map(|(clave, valor)| {
            format!(
                "{}={}",
                urlencoding::encode(clave),
                urlencoding::encode(valor)
            )
        })
        .collect::<Vec<_>>()
        .join("&");

    // El `authorization_endpoint` lo publica el propio proveedor en su documento
    // de descubrimiento, así que se confía en él: los proveedores reales no
    // llevan `?`, `&` ni `#` en esa URL, y el proveedor falso la genera limpia.
    // Aun así el separador se elige según lo que ya traiga: un `?` fijo produciría
    // dos interrogantes si el endpoint ya viniera con consulta. No se defiende
    // contra un `#` —haría que el navegador descartara los seis parámetros—, pero
    // sería un proveedor mal formado, y defenderse exige un parser de URL entero
    // para un valor que no se controla y que ningún test puede provocar.
    let separador = if discovery.authorization_endpoint.contains('?') {
        '&'
    } else {
        '?'
    };

    AuthorizationRequest {
        url: format!("{}{separador}{consulta}", discovery.authorization_endpoint),
        state,
        nonce,
    }
}

/// 32 caracteres aleatorios alfanuméricos.
///
/// `rand` está en 0.8.8, así que la API es `thread_rng()` y
/// `distributions::Alphanumeric`; `rand::rng()` y `distr::Alphanumeric` son de
/// 0.9/0.10 y no compilan aquí.
fn token_aleatorio() -> String {
    use rand::Rng;

    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(TOKEN_LENGTH)
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::http::{StatusCode, header};

    use super::{Discovery, build_authorization_url};
    use crate::config::OidcSettings;
    use crate::secret::SecretString;
    use crate::test_support::{
        OIDC_CLIENT_ID, OIDC_REDIRECT_URL, get, header_text, parametros_de_consulta,
        proveedor_falso::ProveedorFalso, router_oidc_con_emisor,
    };

    /// Ubica la redirección de un login, sin seguirla.
    async fn location_del_login(router: Router) -> String {
        let respuesta = get(router, "/api/v1/auth/login").await;

        assert_eq!(
            respuesta.status(),
            StatusCode::FOUND,
            "el login debe contestar 302"
        );

        header_text(&respuesta, header::LOCATION).expect("el 302 debe traer Location")
    }

    #[tokio::test]
    async fn la_redireccion_lleva_los_seis_parametros() {
        // Escenario 1.7.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let location = location_del_login(router).await;

        assert!(
            location.starts_with(&format!("{}/authorize?", proveedor.issuer)),
            "la redirección no apunta al authorization_endpoint: {location}"
        );

        // El recuento y la codificación se afirman sobre la `Location` **cruda**:
        // si se quitara el `urlencoding::encode`, los parámetros seguirían
        // decodificándose a los mismos valores, así que una aserción solo sobre lo
        // ya decodificado no notaría el cambio. Y el recuento por `&` es lo que
        // hace caer el test ante un parámetro de más (un `prompt=none`, por
        // ejemplo): serían siete.
        let consulta = location
            .split_once('?')
            .map(|(_, consulta)| consulta)
            .expect("la redirección debe traer cadena de consulta");

        assert_eq!(
            consulta.split('&').count(),
            6,
            "la redirección debe llevar exactamente seis parámetros: {location}"
        );
        assert!(
            location.contains(
                "redirect_uri=http%3A%2F%2Flocalhost%3A8080%2Fapi%2Fv1%2Fauth%2Fcallback"
            ),
            "el redirect_uri debe ir codificado en la Location: {location}"
        );
        assert!(
            location.contains("scope=openid%20email"),
            "el scope debe ir codificado (openid%20email) en la Location: {location}"
        );

        let parametros = parametros_de_consulta(&location);

        assert_eq!(
            parametros.get("response_type").map(String::as_str),
            Some("code"),
            "falta response_type=code: {location}"
        );
        assert_eq!(
            parametros.get("client_id").map(String::as_str),
            Some(OIDC_CLIENT_ID),
            "falta el client_id: {location}"
        );
        assert_eq!(
            parametros.get("redirect_uri").map(String::as_str),
            Some(OIDC_REDIRECT_URL),
            "falta el redirect_uri: {location}"
        );

        let scope = parametros
            .get("scope")
            .expect("la redirección debe traer scope");

        assert!(
            scope.split_whitespace().any(|valor| valor == "openid"),
            "el scope debe incluir openid: {scope}"
        );
        assert!(
            scope.split_whitespace().any(|valor| valor == "email"),
            "el scope debe incluir email: {scope}"
        );

        assert!(
            parametros
                .get("state")
                .is_some_and(|state| !state.is_empty()),
            "falta el state: {location}"
        );
        assert!(
            parametros
                .get("nonce")
                .is_some_and(|nonce| !nonce.is_empty()),
            "falta el nonce: {location}"
        );
    }

    #[tokio::test]
    async fn cada_login_usa_un_state_y_un_nonce_nuevos() {
        // Escenario 1.8.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let primera = parametros_de_consulta(&location_del_login(router.clone()).await);
        let segunda = parametros_de_consulta(&location_del_login(router).await);

        let state_a = primera
            .get("state")
            .expect("el primer login debe traer state");
        let state_b = segunda
            .get("state")
            .expect("el segundo login debe traer state");
        let nonce_a = primera
            .get("nonce")
            .expect("el primer login debe traer nonce");
        let nonce_b = segunda
            .get("nonce")
            .expect("el segundo login debe traer nonce");

        assert_ne!(state_a, state_b, "dos logins no pueden compartir state");
        assert_ne!(nonce_a, nonce_b, "dos logins no pueden compartir nonce");

        for (nombre, valor) in [
            ("state", state_a),
            ("state", state_b),
            ("nonce", nonce_a),
            ("nonce", nonce_b),
        ] {
            assert_eq!(
                valor.chars().count(),
                32,
                "el {nombre} debe medir 32 caracteres: {valor}"
            );
        }
    }

    #[tokio::test]
    async fn el_login_no_establece_sesion() {
        // Escenario 1.9.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let respuesta = get(router, "/api/v1/auth/login").await;

        assert_eq!(respuesta.status(), StatusCode::FOUND);
        assert!(
            header_text(&respuesta, header::SET_COOKIE).is_none(),
            "el login no puede emitir una Set-Cookie de sesión"
        );
    }

    #[test]
    fn el_endpoint_que_ya_trae_consulta_no_duplica_el_interrogante() {
        // El separador de la consulta se elige según lo que ya traiga el
        // `authorization_endpoint`: un endpoint con `?` recibe `&`, de modo que no
        // aparece un segundo interrogante que descartaría su consulta original.
        let settings = OidcSettings {
            issuer: "https://proveedor.test".to_owned(),
            client_id: OIDC_CLIENT_ID.to_owned(),
            client_secret: SecretString::new("secreto-de-prueba".to_owned()),
            redirect_url: OIDC_REDIRECT_URL.to_owned(),
        };
        let discovery = Discovery {
            authorization_endpoint: "https://proveedor.test/authorize?audience=kesto".to_owned(),
            token_endpoint: "https://proveedor.test/token".to_owned(),
            jwks_uri: "https://proveedor.test/jwks".to_owned(),
        };

        let solicitud = build_authorization_url(&settings, &discovery);

        assert!(
            solicitud
                .url
                .starts_with("https://proveedor.test/authorize?audience=kesto&"),
            "el endpoint que ya trae consulta debe continuar con `&`: {}",
            solicitud.url
        );
        assert_eq!(
            solicitud.url.matches('?').count(),
            1,
            "no puede haber dos interrogantes en la URL: {}",
            solicitud.url
        );
    }
}
