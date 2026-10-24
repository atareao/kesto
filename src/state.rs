//! Estado compartido de la aplicación.

use std::sync::Arc;

use crate::config::{AuthMode, Config, ConfigError};
use crate::secret::SecretString;

/// Estado de la autenticación.
///
/// Existe **sólo** cuando el modo es `Oidc`: en `Disabled` no hay capa ninguna
/// que montar, y guardar un secreto que nadie va a usar sería guardar un secreto
/// de más. Por eso `AppState::auth` es un `Option` y no un campo vacío: el modo
/// se lee en el tipo, y quien tenga un `AuthState` en la mano sabe que hay
/// autenticación sin consultar la configuración.
#[derive(Debug, Clone)]
pub struct AuthState {
    /// Clave de firma de la sesión (`KESTO_SECRET`), ya resuelta por `Config`.
    ///
    /// Es un [`SecretString`] y no un `String` para que no pueda salir por
    /// `Debug` ni acabar en una línea de registro por descuido.
    pub secret: SecretString,
    // M3 añadirá aquí el descubrimiento cacheado y el mapa de `state`/`nonce`
    // con su marca de tiempo. Sus campos no se inventan hoy.
}

/// Estado que comparten los manejadores de la API.
#[derive(Clone)]
pub struct AppState {
    /// Configuración efectiva, cargada una sola vez al arrancar.
    pub config: Arc<Config>,
    /// Estado de la autenticación: `None` cuando no se monta ninguna capa.
    pub auth: Option<AuthState>,
}

impl AppState {
    /// Construye el estado a partir de la configuración efectiva.
    ///
    /// Falla —en vez de degradar a superficie pública en silencio— si el modo
    /// es `Oidc` y no hay clave de firma. `Config` ya garantiza que en `Oidc` la
    /// clave está, así que ese camino es inalcanzable por construcción; y
    /// precisamente por eso no puede quedarse en un `None`, que aquí significaría
    /// «panel sin *gate*»: un fallo de invariante no puede abrir la superficie
    /// sin que nadie se entere.
    pub fn new(config: Config) -> Result<Self, ConfigError> {
        let auth = match &config.auth {
            AuthMode::Oidc(_) => match &config.secret {
                Some(secret) => Some(AuthState {
                    secret: secret.clone(),
                }),
                None => {
                    return Err(ConfigError::MissingVariable {
                        name: "KESTO_SECRET",
                    });
                }
            },
            AuthMode::Disabled => None,
        };

        Ok(Self {
            config: Arc::new(config),
            auth,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::config::Config;
    use crate::test_support::{
        MASTER_KEY, OIDC_CLIENT_ID, OIDC_CLIENT_SECRET, OIDC_ISSUER_URL, SESSION_SECRET,
        lookup_from,
    };

    #[test]
    fn en_desarrollo_no_hay_estado_de_autenticacion() {
        let config = Config::from_lookup(&lookup_from(&[("KESTO_MASTER_KEY", MASTER_KEY)]))
            .expect("la configuración de desarrollo es válida");

        let estado = AppState::new(config).expect("el estado se construye");

        assert!(
            estado.auth.is_none(),
            "sin proveedor no hay capa que montar ni secreto que guardar"
        );
    }

    #[test]
    fn con_oidc_el_estado_lleva_la_clave_y_no_la_publica() {
        let config = Config::from_lookup(&lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER_URL),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]))
        .expect("la configuración OIDC es válida");

        let estado = AppState::new(config).expect("el estado se construye");
        let auth = estado
            .auth
            .as_ref()
            .expect("con Oidc hay capa de autenticación");

        assert_eq!(auth.secret.expose(), SESSION_SECRET);

        // El estado acaba en un `Debug` en cuanto alguien registre `?state`, y
        // la clave de firma no puede salir por ahí.
        let en_debug = format!("{auth:?}");

        assert!(
            !en_debug.contains(SESSION_SECRET),
            "el Debug del estado publica la clave de firma: {en_debug}"
        );
    }
}
