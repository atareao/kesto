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
//! cosas funcionan. Por eso el arnés es `wiremock` sirviendo un proveedor de
//! verdad, con un par RSA real y el ID token firmado `RS256` de verdad cuando
//! M4 añada el `token_endpoint` y el `jwks_uri`; en M3 sirve el documento de
//! descubrimiento.
//!
//! **Ninguna prueba sale a internet**: `wiremock` levanta un servidor HTTP real
//! atado a `127.0.0.1` con un puerto efímero, y todas las peticiones se quedan
//! en la máquina.

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Ruta del documento de descubrimiento.
const RUTA_DESCUBRIMIENTO: &str = "/.well-known/openid-configuration";

/// Proveedor OIDC falso: un servidor `wiremock` que sirve el descubrimiento.
///
/// En M4 ganará los endpoints `token_endpoint` y `jwks_uri` con un par RSA real
/// y el ID token firmado `RS256`; hoy solo sirve el documento de descubrimiento.
pub(crate) struct ProveedorFalso {
    /// El servidor real, vivo mientras lo esté el proveedor.
    server: MockServer,
    /// Emisor: la base contra la que se construyen los endpoints.
    pub(crate) issuer: String,
}

impl ProveedorFalso {
    /// Levanta el proveedor y monta el documento de descubrimiento.
    ///
    /// El documento **no** publica `userinfo_endpoint`, y eso es deliberado: el
    /// flujo no puede depender de `userinfo`, así que el descubrimiento no lo
    /// exige y el proveedor falso no lo ofrece.
    pub(crate) async fn servir() -> Self {
        let server = MockServer::start().await;
        let issuer = server.uri();

        let documento = serde_json::json!({
            "authorization_endpoint": format!("{issuer}/authorize"),
            "token_endpoint": format!("{issuer}/token"),
            "jwks_uri": format!("{issuer}/jwks"),
        });

        Mock::given(method("GET"))
            .and(path(RUTA_DESCUBRIMIENTO))
            .respond_with(ResponseTemplate::new(200).set_body_json(documento))
            .mount(&server)
            .await;

        Self { server, issuer }
    }

    /// Cuántas peticiones de descubrimiento han llegado al proveedor.
    ///
    /// Es lo que hace creíble la caché: contar las peticiones es la única forma
    /// de probar que un segundo login **no** vuelve a pedir el documento.
    pub(crate) async fn peticiones_de_descubrimiento(&self) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|peticion| peticion.url.path() == RUTA_DESCUBRIMIENTO)
            .count()
    }
}
