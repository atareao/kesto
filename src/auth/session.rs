//! Sesión del operador: el JWT que viaja en la cookie y los atributos de esta.
//!
//! La sesión no tiene estado en servidor: es un JWT firmado con `KESTO_SECRET`
//! que sobrevive a un reinicio y no depende de que exista la base de datos.

use axum_extra::extract::cookie::{Cookie, SameSite};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode, get_current_timestamp,
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::secret::SecretString;

/// Nombre de la cookie de sesión.
///
/// Es una constante y no un literal repetido: el `logout` tiene que borrar
/// exactamente esta cookie.
pub const SESSION_COOKIE_NAME: &str = "kesto_session";

/// Vida de la sesión: 8 horas.
///
/// Es una constante del módulo, no una variable de entorno nueva: el panel es
/// de un solo operador y una sesión que caduque en mitad de una copia obliga a
/// rehacer el flujo de entrada.
pub const SESSION_LIFETIME: std::time::Duration = std::time::Duration::from_secs(8 * 60 * 60);

/// Identidad de la sesión: exactamente `sub`, `email`, `iat` y `exp`.
///
/// La estructura es la que sella la superficie: con esos cuatro campos y
/// `deny_unknown_fields`, un token que traiga cualquier claim de más —un rol,
/// por ejemplo— no se puede deserializar y por tanto no se acepta. No hay que
/// acordarse de rechazarlo: no cabe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claims {
    /// Identificador del usuario en el proveedor de identidad.
    pub sub: String,
    /// Correo del usuario, tomado del ID token ya verificado.
    pub email: String,
    /// Momento de emisión, en segundos desde la época.
    pub iat: u64,
    /// Momento de caducidad, en segundos desde la época.
    pub exp: u64,
}

/// Emite el JWT de sesión firmado con `HS256` y `secret`.
///
/// Devuelve el token, no la cookie: quien lo llame decide si además lo envuelve
/// en una cabecera `Set-Cookie`.
pub fn issue(secret: &SecretString, sub: String, email: String) -> Result<String, AppError> {
    let emitido = get_current_timestamp();
    let claims = Claims {
        sub,
        email,
        iat: emitido,
        exp: emitido + SESSION_LIFETIME.as_secs(),
    };

    let cabecera = Header::new(Algorithm::HS256);
    // La clave entra por el tipo que redacta y solo sale por `expose`, en el
    // único sitio donde hace falta: firmar.
    let clave = EncodingKey::from_secret(secret.expose().as_bytes());

    encode(&cabecera, &claims, &clave).map_err(|error| {
        tracing::error!(%error, "no se pudo firmar el JWT de sesión");
        AppError::Other("no se pudo emitir la sesión".to_owned())
    })
}

/// Verifica la firma, el algoritmo y la vigencia de un JWT de sesión.
///
/// Rechaza con [`AppError::Unauthorized`] todo token mal firmado, firmado con
/// otra clave, caducado o manipulado. La cookie **ausente** no llega hasta aquí:
/// la rechaza el *gate* de la superficie protegida antes de llamar a esta
/// función, que trabaja sobre un token ya extraído.
pub fn verify(secret: &SecretString, token: &str) -> Result<Claims, AppError> {
    // El algoritmo se fija aquí y no se lee de la cabecera del token: aceptar el
    // que traiga el propio token es la confusión de algoritmo de manual.
    let mut validacion = Validation::new(Algorithm::HS256);
    // La sesión no declara `aud` —el panel es su único destinatario— y hoy el
    // defecto de `Validation` tampoco lo rechazaría por ausencia: sin `aud` en la
    // validación, la rama que lo comprueba no se dispara. Se desactiva para que
    // el día que la sesión lleve un `aud` no quede a medio comprobar en silencio:
    // o se valida de verdad, con su valor esperado, o no se valida.
    validacion.validate_aud = false;
    // Sin margen de cortesía sobre `exp`: «`exp` en vigor» es literal. El
    // `leeway` que trae `Validation` por defecto —60 segundos— no cubre ningún
    // desfase entre relojes, porque no hay dos: el navegador no valida el JWT,
    // solo guarda la cookie, y el único reloj que interviene es el de este
    // proceso, que firma y verifica con el mismo. Lo único que haría ese minuto
    // es **alargar la vida de una sesión ya caducada**.
    validacion.leeway = 0;

    let clave = DecodingKey::from_secret(secret.expose().as_bytes());

    decode::<Claims>(token, &clave, &validacion)
        .map(|datos| datos.claims)
        .map_err(|error| {
            // El detalle se queda en el registro, que es donde el operador lo
            // necesita; el cliente recibe el 401 con su motivo.
            tracing::debug!(%error, "cookie de sesión rechazada");
            AppError::Unauthorized("la cookie de sesión no es válida".to_owned())
        })
}

/// Construye la cookie de sesión con el JWT dado.
///
/// `redirect_url` decide el atributo `Secure`: se añade solo cuando su esquema
/// es `https`, porque en desarrollo se entra por `http://localhost` y una
/// cookie `Secure` no viajaría, así que la sesión no se establecería nunca.
pub fn session_cookie(token: String, redirect_url: &str) -> Result<Cookie<'static>, AppError> {
    // El crate `cookie` espera una duración del crate `time`, que es una
    // dependencia transitiva y no se puede nombrar desde aquí. La conversión
    // desde `std::time::Duration` deja que el compilador infiera el tipo del
    // parámetro de `max_age`; con 8 horas no puede fallar, pero el error se
    // propaga tipado en vez de recurrir a un `unwrap`.
    let max_age = SESSION_LIFETIME.try_into().map_err(|_| {
        AppError::Other("la vida de la sesión no cabe en el Max-Age de la cookie".to_owned())
    })?;

    Ok(Cookie::build((SESSION_COOKIE_NAME, token))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/")
        .secure(requires_secure(redirect_url))
        .max_age(max_age)
        .build())
}

/// ¿El esquema de la URL de retorno obliga a marcar la cookie como `Secure`?
///
/// Una URL que no se pueda leer como `esquema://…` no se considera segura: el
/// defecto útil es el del desarrollo, donde el retorno es `http` sobre
/// `localhost`.
fn requires_secure(redirect_url: &str) -> bool {
    redirect_url
        .split_once("://")
        .is_some_and(|(esquema, _)| esquema.eq_ignore_ascii_case("https"))
}

#[cfg(test)]
mod tests {
    use axum_extra::extract::cookie::SameSite;

    use super::{Claims, SESSION_COOKIE_NAME, SESSION_LIFETIME, issue, session_cookie, verify};
    use crate::config::{AuthMode, Config};
    use crate::error::AppError;
    use crate::secret::SecretString;
    use crate::test_support::lookup_from;

    /// Clave maestra larga: que ningún aviso se cuele en la respuesta.
    const MASTER_KEY: &str = "clave-maestra-distinguible-000000000000000000000000000000000000";
    /// Clave de firma de la sesión, con la longitud de sobra.
    const SECRET: &str = "clave-de-firma-distinguible-000000000000000000000000000000000000";
    /// Otra clave de firma, distinguible de la anterior: es la que no vale.
    const OTHER_SECRET: &str = "otra-clave-de-firma-000000000000000000000000000000000000000000";
    /// Emisor del proveedor de pruebas.
    const OIDC_ISSUER: &str = "https://idp.example.test/realms/kesto";
    /// Identificador del cliente de pruebas.
    const OIDC_CLIENT_ID: &str = "kesto-panel";
    /// Secreto de cliente OIDC: solo tiene que estar presente.
    const OIDC_CLIENT_SECRET: &str = "secreto-de-cliente-oidc-distinguible";
    /// URL de retorno con esquema `http`: el caso del desarrollo.
    const REDIRECT_HTTP: &str = "http://localhost:8080/api/v1/auth/callback";
    /// URL de retorno con esquema `https`: el caso del despliegue.
    const REDIRECT_HTTPS: &str = "https://kesto.example.test/api/v1/auth/callback";
    /// Usuario distinguible de la sesión.
    const SUB: &str = "usuario-distinguible-1";
    /// Correo distinguible de la sesión.
    const EMAIL: &str = "operador-distinguible@example.test";
    /// ID token del proveedor, con forma de JWT ajeno: no puede pasar por sesión.
    const ID_TOKEN_DEL_PROVEEDOR: &str =
        "eyJhbGciOiJSUzI1NiJ9.ID-TOKEN-DEL-PROVEEDOR.firma-del-proveedor";

    /// Clave de firma, envuelta en el tipo que redacta.
    fn secret() -> SecretString {
        SecretString::new(SECRET.to_owned())
    }

    /// La otra clave de firma, la que no debe validar la sesión.
    fn otro_secreto() -> SecretString {
        SecretString::new(OTHER_SECRET.to_owned())
    }

    /// Emite una sesión con los valores distinguibles de estas pruebas.
    fn sesion() -> String {
        issue(&secret(), SUB.to_owned(), EMAIL.to_owned()).expect("la emisión no puede fallar")
    }

    /// URL de retorno efectiva de una configuración con la autenticación encendida.
    ///
    /// La configuración entra por lectura inyectada, como en `bootstrap-core`:
    /// nunca se muta el entorno del proceso.
    fn redirect_configurado(redirect_url: &str) -> String {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
            ("OIDC_REDIRECT_URL", redirect_url),
        ]);

        let config = Config::from_lookup(&lookup).expect("la configuración OIDC es válida");

        match &config.auth {
            AuthMode::Oidc(settings) => settings.redirect_url.clone(),
            AuthMode::Disabled => panic!("las tres variables OIDC deben encender la autenticación"),
        }
    }

    /// Cancela la firma de un JWT alterando un carácter de su contenido.
    ///
    /// Es una manipulación real: el token sigue teniendo tres partes y sigue
    /// siendo base64url, pero ya no lo respalda la firma original.
    fn manipular(token: &str) -> String {
        let mut caracteres: Vec<char> = token.chars().collect();
        let posicion = caracteres.len() / 2;

        if let Some(caracter) = caracteres.get_mut(posicion) {
            *caracter = if *caracter == 'a' { 'b' } else { 'a' };
        }

        caracteres.into_iter().collect()
    }

    /// Firma un JWT de sesión con el `exp` indicado, saltándose `issue`.
    ///
    /// Es lo que permite fabricar un token caducado: la emisión real siempre
    /// pone `exp` a ocho horas vista.
    fn firmar_con_caducidad(exp: u64) -> String {
        let claims = Claims {
            sub: SUB.to_owned(),
            email: EMAIL.to_owned(),
            iat: exp - SESSION_LIFETIME.as_secs(),
            exp,
        };

        jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .expect("el token de prueba debe poder firmarse")
    }

    /// Claves del payload de un JWT, ordenadas y sin verificar la firma.
    fn claves_del_payload(token: &str) -> Vec<String> {
        let claims: serde_json::Value = jsonwebtoken::dangerous::insecure_decode_claims(token)
            .expect("el payload debe poder leerse como JSON");

        let mut claves: Vec<String> = claims
            .as_object()
            .expect("los claims deben ser un objeto JSON")
            .keys()
            .cloned()
            .collect();
        claves.sort();

        claves
    }

    #[test]
    fn los_atributos_de_la_cookie() {
        let cookie = session_cookie(sesion(), REDIRECT_HTTP).expect("la cookie se construye");

        assert_eq!(
            cookie.name(),
            SESSION_COOKIE_NAME,
            "el logout tendrá que borrar exactamente esta cookie"
        );
        assert_eq!(
            cookie.http_only(),
            Some(true),
            "la cookie debe ser HttpOnly"
        );
        assert_eq!(
            cookie.same_site(),
            Some(SameSite::Strict),
            "la cookie debe ser SameSite=Strict"
        );
        assert_eq!(
            cookie.path(),
            Some("/"),
            "la cookie debe valer para todo el panel"
        );
        assert!(
            cookie.max_age().is_some(),
            "la cookie debe llevar Max-Age, o el navegador la trataría de sesión"
        );

        // Y el renderizado, que es lo que viaja de verdad en la cabecera.
        let cabecera = cookie.to_string();

        for atributo in ["HttpOnly", "SameSite=Strict", "Path=/", "Max-Age="] {
            assert!(
                cabecera.contains(atributo),
                "la Set-Cookie no trae {atributo}: {cabecera}"
            );
        }
    }

    #[test]
    fn el_secure_depende_del_esquema_de_retorno() {
        let con_http = session_cookie(sesion(), &redirect_configurado(REDIRECT_HTTP))
            .expect("la cookie se construye");

        assert!(
            !con_http.to_string().contains("Secure"),
            "con retorno http la cookie no puede ir Secure, o no viajaría: {con_http}"
        );

        let con_https = session_cookie(sesion(), &redirect_configurado(REDIRECT_HTTPS))
            .expect("la cookie se construye");

        assert_eq!(
            con_https.secure(),
            Some(true),
            "con retorno https la cookie debe ir Secure"
        );
        assert!(
            con_https.to_string().contains("Secure"),
            "con retorno https la Set-Cookie debe declarar Secure: {con_https}"
        );
    }

    #[test]
    fn los_claims_de_la_sesion_son_los_cuatro_sin_roles() {
        let token = sesion();

        let claves = claves_del_payload(&token);

        assert_eq!(
            claves,
            ["email", "exp", "iat", "sub"],
            "los claims de la sesión deben ser exactamente los cuatro: {token}"
        );

        // El SRS no define ni un rol, así que la superficie no se inventa.
        for rol in ["role", "roles", "rol"] {
            assert!(
                !claves.iter().any(|clave| clave == rol),
                "la sesión lleva el claim de rol {rol}: {token}"
            );
        }
    }

    #[test]
    fn la_vida_de_la_sesion_es_una_constante_de_ocho_horas() {
        assert_eq!(
            SESSION_LIFETIME.as_secs(),
            8 * 60 * 60,
            "la vida de la sesión debe ser de 8 horas"
        );

        let token = sesion();
        let claims = verify(&secret(), &token).expect("el JWT emitido debe verificarse");
        let cookie = session_cookie(token, REDIRECT_HTTP).expect("la cookie se construye");

        let max_age = cookie
            .max_age()
            .map(|duracion| duracion.whole_seconds())
            .expect("la cookie debe llevar Max-Age");

        assert_eq!(
            max_age,
            SESSION_LIFETIME.as_secs() as i64,
            "el Max-Age de la cookie y la constante de vida deben coincidir"
        );
        assert_eq!(
            claims.exp - claims.iat,
            SESSION_LIFETIME.as_secs(),
            "el exp del JWT y la constante de vida deben coincidir"
        );
    }

    #[test]
    fn una_cookie_firmada_con_otra_clave_no_se_acepta() {
        let error = verify(&otro_secreto(), &sesion())
            .expect_err("una clave distinta de KESTO_SECRET no puede validar la sesión");

        assert!(
            matches!(error, AppError::Unauthorized(_)),
            "el rechazo debe ser un 401 tipado: {error}"
        );
    }

    #[test]
    fn una_cookie_caducada_no_se_acepta() {
        let error = verify(
            &secret(),
            &firmar_con_caducidad(jsonwebtoken::get_current_timestamp() - 60 * 60),
        )
        .expect_err("una sesión caducada no se acepta");

        assert!(
            matches!(error, AppError::Unauthorized(_)),
            "el rechazo debe ser un 401 tipado: {error}"
        );
    }

    #[test]
    fn una_cookie_con_el_exp_apenas_pasado_no_se_acepta() {
        // Este es el candado de `validacion.leeway = 0`, y el único test que lo
        // sostiene: `exp` treinta segundos atrás lo rechaza el leeway cero y lo
        // **aceptaría** el de cortesía que trae `Validation::new` por defecto
        // —sesenta segundos—, de modo que revertir esa decisión pone este test
        // en rojo. No es «otro caso de caducado»: es el que distingue las dos
        // situaciones. Y el margen no cubriría ningún desfase entre relojes,
        // porque no hay dos: el navegador no valida el JWT, solo guarda la
        // cookie, y el único reloj que interviene es el de este proceso, que
        // firma y verifica con el mismo. Lo único que haría es alargar la vida
        // de una sesión ya caducada.
        let error = verify(
            &secret(),
            &firmar_con_caducidad(jsonwebtoken::get_current_timestamp() - 30),
        )
        .expect_err("una sesión caducada hace 30 segundos no se acepta");

        assert!(
            matches!(error, AppError::Unauthorized(_)),
            "el rechazo debe ser un 401 tipado: {error}"
        );
    }

    #[test]
    fn una_cookie_manipulada_no_se_acepta() {
        let error = verify(&secret(), &manipular(&sesion()))
            .expect_err("alterar el contenido sin volver a firmarlo no puede colar");

        assert!(
            matches!(error, AppError::Unauthorized(_)),
            "el rechazo debe ser un 401 tipado: {error}"
        );
    }

    #[test]
    fn el_payload_de_la_cookie_son_exactamente_los_cuatro_claims() {
        // Es un candado del **esquema del payload**, no una prueba de fuga por
        // marcador: en M1 `issue` no tiene por dónde recibir un ID token, así que
        // no hay camino en el que acabe en la cookie. La prueba de fuga de verdad
        // —credenciales distinguibles buscadas literalmente en el log de un flujo
        // completo— es el escenario 1.38, que vive en `src/auth/mod.rs` y llega
        // con M2.
        let token = sesion();
        let cookie = session_cookie(token.clone(), REDIRECT_HTTP).expect("la cookie se construye");

        assert_eq!(
            cookie.value(),
            token,
            "la cookie debe llevar únicamente el JWT de sesión"
        );

        // La superficie del JWT es lo que lo garantiza: con exactamente cuatro
        // claims no hay dónde meter el ID token ni el token de acceso, y
        // cualquier claim de más rompe esta igualdad.
        assert_eq!(
            claves_del_payload(&token),
            ["email", "exp", "iat", "sub"],
            "la sesión no puede llevar claims del proveedor: {token}"
        );

        // Y si el proveedor hubiera dejado su ID token en la cookie, tampoco
        // valdría como sesión: aquí solo firma Kesto, con KESTO_SECRET, así que
        // un token `RS256` del proveedor no pasa la comprobación de algoritmo.
        assert!(
            verify(&secret(), ID_TOKEN_DEL_PROVEEDOR).is_err(),
            "un token del proveedor no puede pasar por sesión de Kesto"
        );
    }
}
