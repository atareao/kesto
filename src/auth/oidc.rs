//! Flujo OIDC: descubrimiento del proveedor, URL de autorización, `state`/`nonce`
//! y, ya en el retorno, el canje del código y la verificación del ID token.
//!
//! Aquí vive todo el flujo salvo el HTTP: el descubrimiento que el arranque
//! intenta, lo que el `login` necesita para redirigir al proveedor, y lo que el
//! `callback` necesita para convertir el retorno en una identidad verificada.

use std::time::Duration;

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode_header};
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
    /// Endpoint del canje del código por los tokens.
    pub token_endpoint: String,
    /// Endpoint del JWKS contra el que se verifica el ID token.
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
    /// `state` emitido: la clave con la que el callback consumirá este login.
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

/// Tiempo máximo que se espera al proveedor en el canje y en el JWKS.
///
/// El mismo criterio que en el descubrimiento: un proveedor que acepta la
/// conexión y no contesta no puede colgar el callback para siempre.
pub const INTERCAMBIO_TIMEOUT: Duration = Duration::from_secs(10);

/// Respuesta del `token_endpoint`, con lo que de verdad se usa.
///
/// Sin `Debug` a propósito: el `id_token` es una credencial del proveedor y un
/// `?respuesta` en un `debug!` la publicaría. Solo se lee el campo que hace
/// falta; el `access_token` y el resto del cuerpo se ignoran —y, al no tocarlos,
/// no hay forma de que acaben en el log—.
#[derive(Deserialize)]
struct TokenResponse {
    /// ID token firmado que hay que verificar antes de emitir sesión.
    id_token: String,
}

/// Claims del ID token que se releen después de que [`Validation`] compruebe
/// firma, `iss`, `aud` y `exp`.
///
/// Solo `sub`, `email`, `nonce` e `iat`: `iss` y `aud` los valida la propia
/// biblioteca sobre el token, así que releerlos aquí sería duplicar la
/// comprobación. `nonce` e `iat` sí se miran aquí porque `jsonwebtoken` no los
/// valida.
///
/// `exp` **no se declara a propósito**, aunque sí se valide. Quien lo exige es
/// `required_spec_claims`, que `jsonwebtoken` comprueba sobre el mapa **crudo**
/// del token —no sobre este struct—, de modo que un token sin `exp` se rechaza
/// sin que aquí haya un campo. Añadirlo sin leerlo daría un `dead_code` que
/// rompería `-D warnings`, y comprobarlo a mano duplicaría lo que ya hace la
/// biblioteca: el candado de esa exigencia es el test
/// `un_id_token_sin_exp_no_se_acepta`.
#[derive(Deserialize)]
struct IdTokenClaims {
    /// Usuario.
    sub: String,
    /// Correo.
    email: String,
    /// `nonce` devuelto, que se compara con el emitido en el login.
    nonce: String,
    /// Emisión, en segundos desde la época.
    iat: u64,
}

/// Identidad tomada del ID token ya verificado.
#[derive(Debug, Clone)]
pub struct IdentidadVerificada {
    /// Identificador del usuario en el proveedor.
    pub sub: String,
    /// Correo del usuario.
    pub email: String,
}

/// Mensaje único con el que se rechaza un ID token que no supera la verificación.
///
/// El cliente no necesita saber qué comprobación falló; el detalle va al
/// registro. Es el mismo criterio que la sesión: `401` con un motivo genérico.
const ID_TOKEN_INVALIDO: &str = "el ID token no es válido";

/// Canjea el código de autorización en el `token_endpoint`.
///
/// Devuelve el ID token **sin verificar**: verificar es el paso siguiente y no
/// se mezcla con el canje. El `client_secret` viaja en el **cuerpo** del POST
/// —`application/x-www-form-urlencoded`—, nunca en la cadena de consulta, y no
/// se interpola en ningún registro.
pub async fn canjear_codigo(
    settings: &OidcSettings,
    discovery: &Discovery,
    code: &str,
) -> Result<String, AppError> {
    let cliente = reqwest::Client::builder()
        .timeout(INTERCAMBIO_TIMEOUT)
        .build()
        .map_err(|error| {
            AppError::BadGateway(format!("no se pudo preparar el cliente del canje: {error}"))
        })?;

    let formulario = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", settings.redirect_url.as_str()),
        ("client_id", settings.client_id.as_str()),
        ("client_secret", settings.client_secret.expose()),
    ];

    let respuesta = cliente
        .post(&discovery.token_endpoint)
        .form(&formulario)
        .send()
        .await
        .map_err(|error| {
            AppError::BadGateway(format!(
                "el proveedor de identidad no respondió al canje del código: {error}"
            ))
        })?;

    if !respuesta.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "el proveedor de identidad contestó {} al canje del código",
            respuesta.status()
        )));
    }

    respuesta
        .json::<TokenResponse>()
        .await
        .map(|datos| datos.id_token)
        .map_err(|error| {
            AppError::BadGateway(format!("la respuesta del canje no es válida: {error}"))
        })
}

/// Pide el JWKS contra el que se verifica el ID token.
///
/// Un JWKS que no se puede leer es un fallo del proveedor, no de Kesto: la causa
/// viaja en un [`AppError::BadGateway`] y acaba en el log, nunca en el cuerpo.
async fn fetch_jwks(url: &str) -> Result<JwkSet, AppError> {
    let cliente = reqwest::Client::builder()
        .timeout(INTERCAMBIO_TIMEOUT)
        .build()
        .map_err(|error| {
            AppError::BadGateway(format!("no se pudo preparar el cliente del JWKS: {error}"))
        })?;

    let respuesta = cliente.get(url).send().await.map_err(|error| {
        AppError::BadGateway(format!(
            "el proveedor de identidad no respondió al JWKS: {error}"
        ))
    })?;

    if !respuesta.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "el proveedor de identidad contestó {} al JWKS",
            respuesta.status()
        )));
    }

    respuesta
        .json::<JwkSet>()
        .await
        .map_err(|error| AppError::BadGateway(format!("el JWKS no es válido: {error}")))
}

/// Rechaza un ID token dejando la causa en el registro y un motivo genérico al
/// cliente.
fn rechazar_id_token(causa: &str) -> AppError {
    tracing::debug!(causa, "ID token rechazado");
    AppError::Unauthorized(ID_TOKEN_INVALIDO.to_owned())
}

/// Verifica el ID token y devuelve la identidad que lleva dentro.
///
/// Cinco comprobaciones: firma `RS256` contra las claves del `jwks_uri`, `iss`,
/// `aud`, vigencia de `exp` e `iat`, y `nonce` igual al emitido en el login.
/// Cualquier fallo rechaza con [`AppError::Unauthorized`]: no hay sesión que
/// emitir.
pub async fn verificar_id_token(
    settings: &OidcSettings,
    discovery: &Discovery,
    id_token: &str,
    nonce_esperado: &str,
) -> Result<IdentidadVerificada, AppError> {
    let jwks = fetch_jwks(&discovery.jwks_uri).await?;

    // El `kid` de la cabecera elige la clave del conjunto. Sin `kid`, o con uno
    // que el JWKS no publica, no hay clave con la que verificar.
    let cabecera = match decode_header(id_token) {
        Ok(cabecera) => cabecera,
        Err(error) => {
            return Err(rechazar_id_token(&format!(
                "la cabecera del ID token no es válida: {error}"
            )));
        }
    };
    let Some(kid) = cabecera.kid else {
        return Err(rechazar_id_token("el ID token no declara kid"));
    };
    let Some(jwk) = jwks.find(&kid) else {
        return Err(rechazar_id_token(
            "el ID token va firmado con una clave que el jwks_uri no publica",
        ));
    };
    let clave = match DecodingKey::from_jwk(jwk) {
        Ok(clave) => clave,
        Err(error) => {
            return Err(rechazar_id_token(&format!(
                "la clave del JWKS no es utilizable: {error}"
            )));
        }
    };

    // El algoritmo se fija a `RS256` y no se lee de la cabecera del token: es la
    // misma prevención de confusión de algoritmo que en la sesión. El `leeway`
    // por defecto —60 segundos— alargaría la vida de un token recién caducado, así
    // que se pone a cero. `iss` y `aud` son obligatorios: un token que no los
    // declare no vale.
    let mut validacion = Validation::new(Algorithm::RS256);
    validacion.leeway = 0;
    validacion.validate_aud = true;
    validacion.set_audience(&[settings.client_id.as_str()]);
    validacion.set_issuer(&[settings.issuer.as_str()]);
    validacion.set_required_spec_claims(&["exp", "iss", "aud"]);

    let datos = match jsonwebtoken::decode::<IdTokenClaims>(id_token, &clave, &validacion) {
        Ok(datos) => datos,
        Err(error) => {
            return Err(rechazar_id_token(&format!(
                "la verificación del ID token falló: {error}"
            )));
        }
    };

    // El `nonce` ata este ID token al login que lo pidió y cierra la repetición.
    // `jsonwebtoken` no lo valida, así que se compara aquí.
    if datos.claims.nonce != nonce_esperado {
        return Err(rechazar_id_token(
            "el nonce del ID token no es el emitido en el login",
        ));
    }

    // `iat` en vigor: un token emitido en el futuro no es de fiar. Tampoco lo
    // valida `jsonwebtoken`, así que se comprueba aquí. El margen es el mismo
    // cero que en `exp`: no hay dos relojes en juego.
    let ahora = jsonwebtoken::get_current_timestamp();
    if datos.claims.iat > ahora {
        return Err(rechazar_id_token("el ID token se emitió en el futuro"));
    }

    Ok(IdentidadVerificada {
        sub: datos.claims.sub,
        email: datos.claims.email,
    })
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::http::{StatusCode, header};

    use super::{Discovery, build_authorization_url};
    use crate::api::api_router;
    use crate::auth::session;
    use crate::config::OidcSettings;
    use crate::secret::SecretString;
    use crate::state::{PendingLogin, RechazoState, STATE_TTL};
    use crate::test_support::{
        EMAIL, OIDC_CLIENT_ID, OIDC_REDIRECT_URL, SESSION_SECRET, SUB, auth_oidc, callback_url,
        capturar_log, estado_oidc_con_emisor, get, header_text, login_state_y_nonce,
        parametros_de_consulta, proveedor_falso::IdToken, proveedor_falso::ProveedorFalso,
        router_oidc_con_emisor, token_de_set_cookie,
    };
    use std::time::{Duration, Instant};

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

    // ── El callback consume el `state` y verifica el ID token ─────────────

    /// Un rechazo del callback: `401` con la envoltura y sin cookie de sesión.
    ///
    /// Es la aserción que comparten los seis escenarios de rechazo, para que
    /// ninguno se olvide de la mitad que importa —que no se emite sesión—.
    async fn assert_rechazo_sin_cookie(respuesta: axum::response::Response) {
        assert_eq!(
            respuesta.status(),
            StatusCode::UNAUTHORIZED,
            "el callback debía rechazar con un error"
        );
        assert!(
            token_de_set_cookie(&respuesta).is_none(),
            "un rechazo no puede emitir cookie de sesión"
        );
    }

    #[tokio::test]
    async fn un_state_desconocido_se_rechaza_sin_emitir_cookie() {
        // Escenario 1.11. Un `state` que no se emitió no está en el mapa ni entre
        // los ya gastados: el callback lo rechaza.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let respuesta = get(
            router,
            &callback_url("state-que-nadie-emitio", "codigo-inventado"),
        )
        .await;

        assert_rechazo_sin_cookie(respuesta).await;
    }

    #[tokio::test]
    async fn un_state_caducado_se_rechaza_sin_emitir_cookie() {
        // Escenario 1.12 — **el candado de la caducidad**, el defecto de la
        // plantilla que este change corrige: guarda la marca de tiempo y nunca la
        // mira, así que su TTL es decorativo.
        //
        // La entrada se inserta con su marca de tiempo **ya retrasada** más allá
        // del TTL: determinista, sin dormir y sin inyectar un reloj. La
        // comprobación al consumir es lo que la rechaza: si el callback solo
        // mirara el mapa, esta entrada pasaría y el test caería.
        let proveedor = ProveedorFalso::servir().await;
        let estado = estado_oidc_con_emisor(&proveedor.issuer);
        let auth = estado
            .auth
            .clone()
            .expect("en modo Oidc hay estado de autenticación");
        let router = api_router(estado);

        let hace = Instant::now()
            .checked_sub(STATE_TTL + Duration::from_secs(1))
            .expect("el reloj de pruebas necesita un uptime mayor que el TTL");
        {
            let mut logins = auth.logins.lock().await;
            logins.insert(
                "state-caducado".to_owned(),
                PendingLogin {
                    nonce: "nonce-caducado".to_owned(),
                    issued_at: hace,
                },
            );
        }

        let respuesta = get(router, &callback_url("state-caducado", "codigo-x")).await;

        assert_rechazo_sin_cookie(respuesta).await;
    }

    #[tokio::test]
    async fn un_state_reutilizado_se_rechaza_sin_emitir_cookie() {
        // Escenario 1.13. El primer callback consume el `state`; el segundo, con
        // el mismo `state`, se rechaza porque ya se gastó.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let token = proveedor.firmar(&IdToken::valido(&proveedor.issuer, OIDC_CLIENT_ID, &nonce));
        proveedor.montar_token_endpoint(&token).await;

        let primera = get(router.clone(), &callback_url(&state, "codigo-1")).await;
        assert_eq!(
            primera.status(),
            StatusCode::FOUND,
            "el primer callback debe consumir el state y completar el flujo"
        );

        let segunda = get(router, &callback_url(&state, "codigo-2")).await;

        assert_rechazo_sin_cookie(segunda).await;
    }

    #[tokio::test]
    async fn el_consumo_purga_los_pendientes_caducados_y_el_rastro_de_consumidos() {
        // Tarea 3.5: el mapa de `state`/`nonce` no crece sin límite. La purga al
        // **insertar** ya tiene candado —`state::tests::la_purga_descarta_las_entradas_caducadas`—;
        // esta cubre la del **consumo** —pendientes caducados y marcas de consumo
        // viejas, el rastro que permite distinguir «reutilizado» de «desconocido»—,
        // que era la mitad sin candado. Borrar cualquiera de los dos `retain` de
        // `consumir_login` deja este test en rojo.
        let auth = auth_oidc();
        let hace = Instant::now()
            .checked_sub(STATE_TTL + Duration::from_secs(1))
            .expect("la purga necesita un uptime mayor que STATE_TTL");
        {
            let mut logins = auth.logins.lock().await;
            logins.insert(
                "pendiente-caducado".to_owned(),
                PendingLogin {
                    nonce: "nonce-caducado".to_owned(),
                    issued_at: hace,
                },
            );

            let mut consumidos = auth.consumidos.lock().await;
            consumidos.insert("consumido-viejo".to_owned(), hace);
        }

        // Un `state` que nadie emitió: pasa por el camino de rechazo de
        // `consumir_login`, que es donde vive la purga, y no inserta nada.
        let rechazo = auth
            .consumir_login("state-que-nadie-emitio")
            .await
            .expect_err("un state desconocido debe rechazarse");

        assert_eq!(rechazo, RechazoState::Desconocido);
        assert!(
            auth.logins.lock().await.is_empty(),
            "el pendiente caducado debe purgarse al consumir: el mapa no puede crecer"
        );
        assert!(
            auth.consumidos.lock().await.is_empty(),
            "la marca de consumo vieja debe purgarse al consumir: el rastro no puede crecer"
        );
    }

    #[tokio::test]
    async fn un_error_del_proveedor_no_llega_a_canjear_nada() {
        // Escenario 1.14. El `token_endpoint` se monta para que el contador tenga
        // dónde contar: si el callback intentara canjear, subiría. El requisito es
        // que no lo intente.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        proveedor
            .montar_token_endpoint("id-token-que-no-debe-usarse")
            .await;

        let respuesta = get(
            router,
            "/api/v1/auth/callback?error=access_denied&state=state-x",
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::UNAUTHORIZED,
            "un error del proveedor debe rechazarse"
        );
        assert_eq!(
            proveedor.peticiones_al_token().await,
            0,
            "un error del proveedor no puede llegar a canjear el código"
        );
    }

    #[tokio::test]
    async fn un_aud_ajeno_no_se_acepta() {
        // Escenario 1.15.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let mut claims = IdToken::valido(&proveedor.issuer, OIDC_CLIENT_ID, &nonce);
        claims.aud = "otro-cliente-distinto".to_owned();
        proveedor
            .montar_token_endpoint(&proveedor.firmar(&claims))
            .await;

        let respuesta = get(router, &callback_url(&state, "codigo")).await;

        assert_rechazo_sin_cookie(respuesta).await;
    }

    #[tokio::test]
    async fn un_token_caducado_no_se_acepta() {
        // Escenario 1.16 — **el candado del leeway cero**. Un `exp` treinta
        // segundos atrás lo rechaza el leeway cero, y lo **aceptaría** el de
        // cortesía que trae `Validation::new` por defecto —sesenta segundos—, de
        // modo que revertir esa decisión pone este test en rojo.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let ahora = jsonwebtoken::get_current_timestamp();
        let mut claims = IdToken::valido(&proveedor.issuer, OIDC_CLIENT_ID, &nonce);
        claims.iat = ahora - 60;
        claims.exp = ahora - 30;
        proveedor
            .montar_token_endpoint(&proveedor.firmar(&claims))
            .await;

        let respuesta = get(router, &callback_url(&state, "codigo")).await;

        assert_rechazo_sin_cookie(respuesta).await;
    }

    #[tokio::test]
    async fn un_id_token_sin_exp_no_se_acepta() {
        // A2 — **el candado de que `exp` es obligatorio**.
        //
        // El escenario 1.16 prueba un `exp` en el pasado, pero siempre con un
        // token que **trae** `exp`: quitara quien quitara `"exp"` de
        // `required_spec_claims`, la suite seguiría verde y un token sin vigencia
        // declarada pasaría. Aquí se omite el claim entero.
        //
        // Quien lo exige es `required_spec_claims`, que `jsonwebtoken` comprueba
        // sobre el mapa **crudo** del token y no sobre `IdTokenClaims`; ahí está
        // el rojo de esta prueba: quitando `"exp"` de esa lista, el token se
        // acepta y este test cae.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let token = proveedor.firmar_claims(&serde_json::json!({
            "iss": proveedor.issuer,
            "aud": OIDC_CLIENT_ID,
            "sub": SUB,
            "email": EMAIL,
            "nonce": nonce,
            "iat": jsonwebtoken::get_current_timestamp(),
        }));
        proveedor.montar_token_endpoint(&token).await;

        let respuesta = get(router, &callback_url(&state, "codigo")).await;

        assert_rechazo_sin_cookie(respuesta).await;
    }

    #[tokio::test]
    async fn un_id_token_emitido_en_el_futuro_no_se_acepta() {
        // A2 — **el candado de la comprobación de `iat`**. `jsonwebtoken` no
        // valida `iat`: lo mira el handler. Con esa comprobación fuera —y con ella
        // el campo del struct—, este token, con todo lo demás en regla, se
        // aceptaría y emitiría sesión. El `exp` es válido a propósito: lo que
        // tiene que rechazar la petición es el `iat` adelantado al reloj.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let ahora = jsonwebtoken::get_current_timestamp();
        let token = proveedor.firmar_claims(&serde_json::json!({
            "iss": proveedor.issuer,
            "aud": OIDC_CLIENT_ID,
            "sub": SUB,
            "email": EMAIL,
            "nonce": nonce,
            "iat": ahora + 600,
            "exp": ahora + 1800,
        }));
        proveedor.montar_token_endpoint(&token).await;

        let respuesta = get(router, &callback_url(&state, "codigo")).await;

        assert_rechazo_sin_cookie(respuesta).await;
    }

    #[tokio::test]
    async fn un_id_token_sin_iat_no_se_acepta() {
        // A2 — **el candado de que `iat` está declarado** en `IdTokenClaims`. Con
        // el campo fuera, un token sin `iat` se deserializaría y se aceptaría,
        // porque `jsonwebtoken` no lo exige; sin este test, borrar el campo dejaba
        // la suite verde y la vigencia sin comprobar.
        //
        // Y aprovecha el camino de error de **deserialización** —el único que
        // podría arrastrar al log un fragmento del payload— para comprobar que no
        // lo hace: el mensaje de `jsonwebtoken` es una etiqueta, no el token. La
        // aserción de que el rechazo se registró es lo que demuestra que la
        // captura estaba en pie y que la de abajo no pasa por vacía.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let token = proveedor.firmar_claims(&serde_json::json!({
            "iss": proveedor.issuer,
            "aud": OIDC_CLIENT_ID,
            "sub": SUB,
            "email": EMAIL,
            "nonce": nonce,
            "exp": jsonwebtoken::get_current_timestamp() + 600,
        }));
        proveedor.montar_token_endpoint(&token).await;

        let (buffer, _guard) = capturar_log();
        let respuesta = get(router, &callback_url(&state, "codigo")).await;
        let log = buffer.texto();

        assert_rechazo_sin_cookie(respuesta).await;

        assert!(
            log.contains("ID token rechazado"),
            "la captura de log no está en pie: el rechazo no quedó registrado: {log}"
        );

        // El payload del ID token, además del token entero: es el fragmento que
        // el error de deserialización podría arrastrar.
        let payload = token.split('.').nth(1).expect("un JWT tiene tres partes");

        assert!(
            !log.contains(token.as_str()),
            "el rechazo publicó el ID token en el log: {log}"
        );
        assert!(
            !log.contains(payload),
            "el rechazo publicó el payload del ID token en el log: {log}"
        );
    }

    #[tokio::test]
    async fn un_nonce_distinto_no_se_acepta() {
        // Escenario 1.17. El token lleva un `nonce` distinto del que el login
        // guardó para ese `state`: el ataque de repetición que el `nonce` cierra.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, _nonce) = login_state_y_nonce(router.clone()).await;
        let claims = IdToken::valido(&proveedor.issuer, OIDC_CLIENT_ID, "nonce-ajeno");
        proveedor
            .montar_token_endpoint(&proveedor.firmar(&claims))
            .await;

        let respuesta = get(router, &callback_url(&state, "codigo")).await;

        assert_rechazo_sin_cookie(respuesta).await;
    }

    #[tokio::test]
    async fn una_firma_fuera_del_jwks_no_se_acepta() {
        // Escenario 1.18. El token va firmado con una clave que el `jwks_uri` no
        // publica, pero con el mismo `kid` que sí publica: la verificación
        // encuentra la clave y es la **firma** la que no cuadra. Sin verificar la
        // firma, este token pasaría.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let claims = IdToken::valido(&proveedor.issuer, OIDC_CLIENT_ID, &nonce);
        proveedor
            .montar_token_endpoint(&proveedor.firmar_con_clave_ajena(&claims))
            .await;

        let respuesta = get(router, &callback_url(&state, "codigo")).await;

        assert_rechazo_sin_cookie(respuesta).await;
    }

    #[tokio::test]
    async fn un_iss_distinto_no_se_acepta() {
        // Escenario 1.19.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let mut claims = IdToken::valido(&proveedor.issuer, OIDC_CLIENT_ID, &nonce);
        claims.iss = "https://otro-emisor.test/realms/kesto".to_owned();
        proveedor
            .montar_token_endpoint(&proveedor.firmar(&claims))
            .await;

        let respuesta = get(router, &callback_url(&state, "codigo")).await;

        assert_rechazo_sin_cookie(respuesta).await;
    }

    #[tokio::test]
    async fn un_token_verificado_se_convierte_en_sesion() {
        // Escenario 1.20. El `sub` y el `email` de la sesión son los del ID token.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let claims = IdToken::valido(&proveedor.issuer, OIDC_CLIENT_ID, &nonce);
        proveedor
            .montar_token_endpoint(&proveedor.firmar(&claims))
            .await;

        let respuesta = get(router, &callback_url(&state, "codigo")).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::FOUND,
            "un token válido debe completar el flujo"
        );

        let jwt =
            token_de_set_cookie(&respuesta).expect("el callback debe emitir la cookie de sesión");
        let claims = session::verify(&SecretString::new(SESSION_SECRET.to_owned()), &jwt)
            .expect("la cookie emitida debe verificar con KESTO_SECRET");

        assert_eq!(
            claims.sub, SUB,
            "el sub de la sesión debe ser el del ID token"
        );
        assert_eq!(
            claims.email, EMAIL,
            "el email de la sesión debe ser el del ID token"
        );
    }

    #[tokio::test]
    async fn el_flujo_no_depende_de_userinfo() {
        // Escenario 1.21. El descubrimiento del proveedor falso no publica
        // `userinfo_endpoint`; el flujo se completa igual porque el ID token
        // verificado basta. Y no se pide ningún `userinfo`.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let (state, nonce) = login_state_y_nonce(router.clone()).await;
        let claims = IdToken::valido(&proveedor.issuer, OIDC_CLIENT_ID, &nonce);
        proveedor
            .montar_token_endpoint(&proveedor.firmar(&claims))
            .await;

        let respuesta = get(router, &callback_url(&state, "codigo")).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::FOUND,
            "el flujo debe completarse sin userinfo_endpoint"
        );
        assert_eq!(
            proveedor.peticiones_a_userinfo().await,
            0,
            "no se puede pedir userinfo: el ID token verificado sobra"
        );
    }
}
