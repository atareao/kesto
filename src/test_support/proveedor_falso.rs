//! Proveedor OIDC falso local para las pruebas.
//!
//! **Por qué este arnés y no dobles de las funciones de red.** Un doble de
//! `reqwest` —o de la propia capa de descubrimiento— devolvería lo que el test
//! le pidiera devolver, y con eso el flujo pasaría sin tocar la red. Eso dejaría
//! sin probar precisamente el requisito central de `auth-capability`: que el ID
//! token se **verifica** —firma `RS256` contra el `jwks_uri`, `iss`, `aud`,
//! `exp`/`iat` y `nonce`—, que es uno de los dos defectos de la plantilla que
//! este change corrige. Un doble de la red hace que la verificación de la firma
//! nunca se ejercite: se prueba que el código llama a las cosas, no que las
//! cosas funcionan.
//!
//! Por eso el arnés es `wiremock` sirviendo un proveedor de verdad: un par RSA
//! **real** (con `rsa`), el ID token firmado **`RS256` de verdad** con ese par,
//! y la clave pública publicada como JWK en el `jwks_uri`. La firma que se
//! publica y la que se verifica son la misma matemática que en producción.
//!
//! **Ninguna prueba sale a internet**: `wiremock` levanta un servidor HTTP real
//! atado a `127.0.0.1` con un puerto efímero, y todas las peticiones se quedan
//! en la máquina.

use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use rsa::RsaPrivateKey;
use rsa::pkcs1::EncodeRsaPrivateKey;
use serde::{Deserialize, Serialize};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::test_support::{EMAIL, SUB};

/// Ruta del documento de descubrimiento.
const RUTA_DESCUBRIMIENTO: &str = "/.well-known/openid-configuration";
/// Ruta del canje del código.
const RUTA_TOKEN: &str = "/token";
/// Ruta del conjunto de claves públicas.
const RUTA_JWKS: &str = "/jwks";

/// `kid` con el que el proveedor falso etiqueta su clave pública y sus tokens.
///
/// Es coherente entre la cabecera del JWT y el JWK a propósito: la verificación
/// tiene que encontrar la clave por su `kid` y comprobar la firma contra ella,
/// no tropezar antes con una clave ausente.
pub(crate) const KID: &str = "kesto-clave-de-prueba";

/// Token de acceso del proveedor: distinguible para que buscarlo en el log o en
/// la cookie sea concluyente.
pub(crate) const ACCESS_TOKEN: &str = "access-token-del-proveedor-distinguible";

/// Claims del ID token que el proveedor falso firma.
///
/// Los valores por defecto de [`IdToken::valido`] son distinguibles: `sub` y
/// `email` son los de la sesión de pruebas, y `nonce` viaja tal cual, de modo
/// que los tests localizan la identidad literalmente en la cookie.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IdToken {
    /// Emisor del token (`iss`).
    pub iss: String,
    /// Audiencia (`aud`): el `client_id` con el que se pidió la sesión.
    pub aud: String,
    /// Usuario (`sub`).
    pub sub: String,
    /// Correo (`email`).
    pub email: String,
    /// `nonce` devuelto: se compara con el emitido en el login.
    pub nonce: String,
    /// Caducidad, en segundos desde la época (`exp`).
    pub exp: u64,
    /// Emisión, en segundos desde la época (`iat`).
    pub iat: u64,
}

impl IdToken {
    /// Un ID token válido para el emisor, el `client_id` y el `nonce` dados.
    ///
    /// Caduca a diez minutos vista y se emite ahora: dentro de la ventana de
    /// vigencia y sin adelantarse al reloj. Los tests que quieren un token que
    /// falle **un** campo parten de aquí y lo tuercen.
    pub(crate) fn valido(issuer: &str, client_id: &str, nonce: &str) -> Self {
        let ahora = jsonwebtoken::get_current_timestamp();

        Self {
            iss: issuer.to_owned(),
            aud: client_id.to_owned(),
            sub: SUB.to_owned(),
            email: EMAIL.to_owned(),
            nonce: nonce.to_owned(),
            exp: ahora + 600,
            iat: ahora,
        }
    }
}

/// Proveedor OIDC falso: un servidor `wiremock` que sirve el descubrimiento, el
/// `token_endpoint` y el `jwks_uri`, con un par RSA real.
pub(crate) struct ProveedorFalso {
    /// El servidor real, vivo mientras lo esté el proveedor.
    server: MockServer,
    /// Emisor: la base contra la que se construyen los endpoints.
    pub(crate) issuer: String,
    /// Clave con la que se firman los ID tokens **válidos**.
    clave: EncodingKey,
    /// Clave que el `jwks_uri` **no** publica: firma los tokens que deben
    /// rechazarse por una firma que no cuadra. Es lo que convierte el escenario
    /// 1.18 en una prueba de la verificación criptográfica y no de una búsqueda.
    clave_no_publicada: EncodingKey,
}

impl ProveedorFalso {
    /// Levanta el proveedor y monta el descubrimiento y el JWKS.
    ///
    /// El documento de descubrimiento **no** publica `userinfo_endpoint`, y eso
    /// es deliberado: el flujo no puede depender de `userinfo`, así que el
    /// descubrimiento no lo exige, el proveedor falso no lo ofrece y el
    /// escenario 1.21 lo comprueba.
    pub(crate) async fn servir() -> Self {
        let server = MockServer::start().await;
        let issuer = server.uri();

        // El par de claves es real: la firma que el callback verifica es la
        // misma matemática que en producción. La segunda clave nunca se publica.
        let clave = generar_clave();
        let clave_no_publicada = generar_clave();

        let documento = serde_json::json!({
            "authorization_endpoint": format!("{issuer}/authorize"),
            "token_endpoint": format!("{issuer}{RUTA_TOKEN}"),
            "jwks_uri": format!("{issuer}{RUTA_JWKS}"),
        });

        Mock::given(method("GET"))
            .and(path(RUTA_DESCUBRIMIENTO))
            .respond_with(ResponseTemplate::new(200).set_body_json(documento))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(RUTA_JWKS))
            .respond_with(ResponseTemplate::new(200).set_body_json(jwks(&clave)))
            .mount(&server)
            .await;

        Self {
            server,
            issuer,
            clave,
            clave_no_publicada,
        }
    }

    /// Firma un ID token `RS256` con la clave que el JWKS **sí** publica.
    pub(crate) fn firmar(&self, claims: &IdToken) -> String {
        firmar_con(&self.clave, claims)
    }

    /// Firma un ID token `RS256` a partir de claims crudos.
    ///
    /// Es lo que permite fabricar tokens que **omiten** un claim obligatorio:
    /// [`IdToken`] declara `exp` e `iat` y no sabe representar su ausencia, y los
    /// candados de A2 necesitan un token sin `exp` y otro sin `iat` para probar
    /// que se rechazan.
    pub(crate) fn firmar_claims(&self, claims: &serde_json::Value) -> String {
        let mut cabecera = Header::new(Algorithm::RS256);
        cabecera.kid = Some(KID.to_owned());

        encode(&cabecera, claims, &self.clave).expect("el ID token de prueba debe firmarse")
    }

    /// Firma un ID token `RS256` con la clave que el JWKS **no** publica.
    ///
    /// La cabecera lleva el mismo `kid` que el JWK publicado: así la
    /// verificación encuentra la clave y el rechazo sale de que la firma no
    /// cuadra, que es lo que el escenario 1.18 quiere cazar. Con un `kid` ajeno,
    /// el token se rechazaría por no encontrar la clave y la comprobación
    /// criptográfica nunca se ejercitaría.
    pub(crate) fn firmar_con_clave_ajena(&self, claims: &IdToken) -> String {
        firmar_con(&self.clave_no_publicada, claims)
    }

    /// Monta el `token_endpoint` para que devuelva el ID token dado.
    ///
    /// Devuelve también un `access_token` distinguible: el flujo no lo usa —con
    /// el ID token verificado sobra— y el escenario 1.38 comprobará que no acaba
    /// ni en el log ni en la cookie.
    pub(crate) async fn montar_token_endpoint(&self, id_token: &str) {
        let cuerpo = serde_json::json!({
            "id_token": id_token,
            "access_token": ACCESS_TOKEN,
            "token_type": "Bearer",
            "expires_in": 600,
        });

        Mock::given(method("POST"))
            .and(path(RUTA_TOKEN))
            .respond_with(ResponseTemplate::new(200).set_body_json(cuerpo))
            .mount(&self.server)
            .await;
    }

    /// Cuántas peticiones de descubrimiento han llegado al proveedor.
    ///
    /// Es lo que hace creíble la caché: contar las peticiones es la única forma
    /// de probar que un segundo login **no** vuelve a pedir el documento.
    pub(crate) async fn peticiones_de_descubrimiento(&self) -> usize {
        self.peticiones_a(RUTA_DESCUBRIMIENTO).await
    }

    /// Cuántas veces se ha llamado al `token_endpoint`.
    ///
    /// Es lo que prueba el escenario 1.14: un `error` del proveedor rechaza sin
    /// llegar a canjear nada, así que este contador tiene que quedarse en cero.
    pub(crate) async fn peticiones_al_token(&self) -> usize {
        self.peticiones_a(RUTA_TOKEN).await
    }

    /// Cuántas peticiones han ido a un endpoint `userinfo`, si lo hubiera.
    ///
    /// El proveedor falso **no** ofrece `userinfo`: si el flujo lo pidiera, la
    /// petición llegaría igual al servidor —sin mock que la atienda— y este
    /// contador lo delataría. Es lo que hace asertable el escenario 1.21.
    pub(crate) async fn peticiones_a_userinfo(&self) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|peticion| peticion.url.path().contains("userinfo"))
            .count()
    }

    /// Cuenta las peticiones recibidas cuya ruta coincide con `ruta`.
    async fn peticiones_a(&self, ruta: &str) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|peticion| peticion.url.path() == ruta)
            .count()
    }
}

/// Genera un par RSA real y devuelve la clave privada como clave de firma.
///
/// A 2048 bits, que es el mínimo que la verificación `RS256` de `jsonwebtoken`
/// espera. La clave se exporta en PKCS#1 DER porque es lo que acepta
/// `EncodingKey::from_rsa_der` y lo que `Jwk::from_encoding_key` sabe leer.
fn generar_clave() -> EncodingKey {
    let mut rng = rand::thread_rng();

    let privada = RsaPrivateKey::new(&mut rng, 2048)
        .expect("la generación de un par RSA de prueba no puede fallar");
    let der = privada
        .to_pkcs1_der()
        .expect("la clave privada de prueba debe codificarse en PKCS#1")
        .as_bytes()
        .to_vec();

    EncodingKey::from_rsa_der(&der)
}

/// Publica la clave pública de `clave` como JWKS, con su `kid`.
///
/// Se usa `Jwk::from_encoding_key`, que extrae el módulo y el exponente y los
/// codifica en base64url sin relleno —como exige el RFC 7518—, de modo que la
/// misma biblioteca que firma y verifica construye la clave que publica. Nada
/// se codifica a mano.
fn jwks(clave: &EncodingKey) -> serde_json::Value {
    let mut jwk = Jwk::from_encoding_key(clave, Algorithm::RS256)
        .expect("la clave pública de prueba debe poder publicarse como JWK");
    jwk.common.key_id = Some(KID.to_owned());

    serde_json::to_value(JwkSet { keys: vec![jwk] }).expect("el JWKS de prueba debe serializarse")
}

/// Firma `claims` con `clave` y el `kid` del proveedor.
fn firmar_con(clave: &EncodingKey, claims: &IdToken) -> String {
    let mut cabecera = Header::new(Algorithm::RS256);
    cabecera.kid = Some(KID.to_owned());

    encode(&cabecera, claims, clave).expect("el ID token de prueba debe firmarse")
}
