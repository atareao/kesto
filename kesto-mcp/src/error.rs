//! Los fallos del cliente de la API, con el mensaje que hay que leer.
//!
//! El servidor no contesta un `401` crudo ni una lista vacía: distingue **de quién es el
//! problema** —falta el token, el token no vale, el scope no llega— y lo dice, porque un
//! agente que recibe «no autorizado» no puede hacer nada con eso y uno que recibe «define
//! `KESTO_API_TOKEN`» sí.
//!
//! Ninguna variante lleva el token: ni en su texto, ni en su `Debug` —el token es un
//! [`SecretToken`](crate::config::SecretToken), que no se imprime—.

/// Un fallo al hablar con la API de Kesto.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// No hay token por ningún lado: ni en la petición, ni en el entorno.
    #[error(
        "no hay token de API: define la variable `{}` en el entorno, o manda la cabecera \
         `Authorization: Bearer <token>` en la petición HTTP",
        crate::config::KESTO_API_TOKEN
    )]
    SinToken,

    /// La API contestó `401`: el token no vale.
    #[error(
        "el token de la API no es válido o está revocado: emite uno nuevo y actualiza \
         `{}` (o el token que manda la petición)",
        crate::config::KESTO_API_TOKEN
    )]
    TokenInvalido,

    /// La API contestó `403`: el token vale, pero no autoriza esto.
    #[error(
        "el token de la API no tiene el scope `run`, que es el que exige esta operación: \
         emite un token con scope `run`"
    )]
    ScopeInsuficiente,

    /// La API contestó un error con su propia explicación.
    #[error("la API de Kesto contestó {status}: {mensaje}")]
    RespuestaDeLaApi {
        /// El código HTTP.
        status: u16,
        /// El mensaje de la API, tal cual.
        mensaje: String,
    },

    /// No se pudo ni empezar a hablar con la API.
    #[error("no se pudo hablar con la API de Kesto en {url}: {fuente}")]
    Transporte {
        /// La URL a la que se intentó llamar.
        url: String,
        /// La causa tal y como la cuenta el cliente HTTP. Nunca lleva el token.
        fuente: String,
    },

    /// La API contestó algo que no es JSON.
    #[error("la respuesta de la API de Kesto no se pudo interpretar como JSON (código {status})")]
    RespuestaIlegible {
        /// El código HTTP de esa respuesta.
        status: u16,
    },
}

impl ApiError {
    /// La clase del fallo, en un identificador estable para el agente.
    ///
    /// Es lo que viaja en el `error` del resultado estructurado, para que un agente pueda
    /// decidir sin analizar el texto: `sin_token` se arregla configurando, `token_invalido`
    /// emitiendo uno nuevo, `scope_insuficiente` pidiendo el scope `run`.
    pub fn clase(&self) -> &'static str {
        match self {
            Self::SinToken => "sin_token",
            Self::TokenInvalido => "token_invalido",
            Self::ScopeInsuficiente => "scope_insuficiente",
            Self::RespuestaDeLaApi { .. } => "respuesta_de_la_api",
            Self::Transporte { .. } => "transporte",
            Self::RespuestaIlegible { .. } => "respuesta_ilegible",
        }
    }

    /// El código HTTP de la API, cuando hubo uno.
    pub fn status_http(&self) -> Option<u16> {
        match self {
            Self::TokenInvalido => Some(401),
            Self::ScopeInsuficiente => Some(403),
            Self::RespuestaDeLaApi { status, .. } | Self::RespuestaIlegible { status } => {
                Some(*status)
            }
            Self::SinToken | Self::Transporte { .. } => None,
        }
    }
}
