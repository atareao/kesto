//! El cliente HTTP de la API de Kesto.
//!
//! Es la única pieza de este crate que habla por el cable, y por eso es la única que
//! necesita un token. El token **no** vive aquí: entra en cada llamada, y quien lo elige
//! es la capa de herramientas —la cabecera de la petición manda sobre el del entorno—.
//!
//! Todas las respuestas se devuelven como el JSON que la API publica, **sin reinterpretar
//! la envoltura**: la API ya tiene su contrato (`status`, `message`, `data`) y traducirlo
//! aquí sería inventar un segundo contrato que se desincroniza del primero.

use reqwest::{RequestBuilder, StatusCode};
use serde_json::Value;

use crate::config::{Config, SecretToken};
use crate::error::ApiError;

/// El cliente de la API de Kesto.
#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    base_url: String,
    env_token: Option<SecretToken>,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient")
            .field("base_url", &self.base_url)
            .field("token_en_el_entorno", &self.env_token.is_some())
            .finish()
    }
}

impl ApiClient {
    /// Construye el cliente a partir de la configuración.
    pub fn new(config: &Config) -> Result<Self, ApiError> {
        let http = reqwest::Client::builder()
            .build()
            .map_err(|error| ApiError::Transporte {
                url: config.base_url().to_owned(),
                fuente: error.to_string(),
            })?;

        Ok(Self {
            http,
            base_url: config.base_url().to_owned(),
            env_token: config.env_token().cloned(),
        })
    }

    /// `GET` sobre una ruta de la API, con sus parámetros de consulta.
    ///
    /// `token` es el de la petición; si no hay, se usa el del entorno. Si tampoco hay,
    /// la llamada **no se hace**: sin credencial no se habla con la API.
    pub async fn get(
        &self,
        ruta: &str,
        consulta: &[(&str, String)],
        token: Option<&SecretToken>,
    ) -> Result<Value, ApiError> {
        let token = self.token(token)?;
        let url = format!("{}{ruta}", self.base_url);

        let peticion = self
            .http
            .get(&url)
            .query(consulta)
            .bearer_auth(token.expose());

        self.interpretar(peticion, &url).await
    }

    /// `GET` sobre una ruta **pública**, que no exige credencial.
    ///
    /// Es el caso de `/api/v1/health`: en modo `disabled` no hay ningún token que mandar y
    /// el servicio contesta igual, así que exigir uno dejaría la herramienta sin poder
    /// decir ni siquiera en qué modo está la API. Si el token está —de la petición o del
    /// entorno— se manda, para que la API siga pudiendo aplicar lo que tenga que aplicar.
    pub async fn get_sin_exigir_token(
        &self,
        ruta: &str,
        consulta: &[(&str, String)],
        token: Option<&SecretToken>,
    ) -> Result<Value, ApiError> {
        let url = format!("{}{ruta}", self.base_url);
        let mut peticion = self.http.get(&url).query(consulta);

        if let Some(token) = token.or(self.env_token.as_ref()) {
            peticion = peticion.bearer_auth(token.expose());
        }

        self.interpretar(peticion, &url).await
    }

    /// `POST` sobre una ruta de la API, sin cuerpo.
    pub async fn post(&self, ruta: &str, token: Option<&SecretToken>) -> Result<Value, ApiError> {
        let token = self.token(token)?;
        let url = format!("{}{ruta}", self.base_url);

        let peticion = self.http.post(&url).bearer_auth(token.expose());

        self.interpretar(peticion, &url).await
    }

    /// Elige el token: el de la petición manda, el del entorno es el defecto.
    fn token<'a>(
        &'a self,
        de_la_peticion: Option<&'a SecretToken>,
    ) -> Result<&'a SecretToken, ApiError> {
        de_la_peticion
            .or(self.env_token.as_ref())
            .ok_or(ApiError::SinToken)
    }

    /// Envía la petición y traduce su desenlace.
    async fn interpretar(&self, peticion: RequestBuilder, url: &str) -> Result<Value, ApiError> {
        let respuesta = peticion
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|error| ApiError::Transporte {
                url: url.to_owned(),
                fuente: error.to_string(),
            })?;

        let status = respuesta.status();
        let cuerpo = respuesta
            .text()
            .await
            .map_err(|error| ApiError::Transporte {
                url: url.to_owned(),
                fuente: error.to_string(),
            })?;

        if status == StatusCode::UNAUTHORIZED {
            return Err(ApiError::TokenInvalido);
        }
        if status == StatusCode::FORBIDDEN {
            return Err(ApiError::ScopeInsuficiente);
        }

        let valor: Value =
            serde_json::from_str(&cuerpo).map_err(|_| ApiError::RespuestaIlegible {
                status: status.as_u16(),
            })?;

        if !status.is_success() {
            // El `message` de la envoltura de la API es el motivo que ya escribió la API
            // —nombra el campo mal escrito, o el recurso que no está— y se reenvía tal
            // cual. Si no lo trae, no se inventa uno: se dice que no lo dijo.
            let mensaje = valor
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("la API no publicó un motivo")
                .to_owned();

            return Err(ApiError::RespuestaDeLaApi {
                status: status.as_u16(),
                mensaje,
            });
        }

        Ok(valor)
    }
}
