//! Estado compartido de la aplicación.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, RwLock};

use crate::auth::oidc::Discovery;
use crate::config::{AuthMode, Config, ConfigError, OidcSettings};
use crate::secret::SecretString;

/// Caducidad de un `state` emitido en el login: 5 minutos.
///
/// Un login que no se completa en ese plazo es un intento abandonado. La
/// comprobación de verdad al consumir es M4 —es el defecto de la plantilla que
/// este change corrige—, pero el dato de la caducidad vive aquí desde ya.
pub const STATE_TTL: Duration = Duration::from_secs(5 * 60);

/// Un login en curso: el `nonce` emitido y el instante en que se emitió.
///
/// El `nonce` se guarda **junto** al `state` y no en un mapa aparte: M4 tendrá
/// que compararlo con el del ID token al verificar, y con la asociación hecha
/// —`state → (nonce, instante)`— esa comparación no depende de nada más.
#[derive(Debug, Clone)]
pub struct PendingLogin {
    /// `nonce` que viajó en la petición de autorización.
    pub nonce: String,
    /// Instante de emisión, para aplicar el TTL.
    pub issued_at: Instant,
}

impl PendingLogin {
    /// ¿Ha caducado esta entrada en `ahora`?
    ///
    /// `saturating_duration_since` y no `duration_since`: un instante anterior
    /// al de emisión —imposible con un reloj monótono, pero gratis de cubrir—
    /// no puede entrar en pánico.
    pub fn expirado_en(&self, ahora: Instant) -> bool {
        ahora.saturating_duration_since(self.issued_at) >= STATE_TTL
    }
}

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
    /// Configuración del proveedor: emisor, `client_id`, secreto y retorno.
    ///
    /// Va tras un [`Arc`] porque el estado se clona en cada petición: clonar el
    /// `Arc` no vuelve a copiar las cadenas.
    pub(crate) settings: Arc<OidcSettings>,
    /// Descubrimiento cacheado del proveedor: `None` hasta que conteste.
    ///
    /// Va tras un [`RwLock`] y no un `Mutex`: el servidor es concurrente, y
    /// varios logins pueden leer la caché a la vez sin estorbarse. Se escribe
    /// solo cuando llega el primer resultado.
    pub(crate) discovery: Arc<RwLock<Option<Discovery>>>,
    /// Logins en curso: `state → (nonce, instante de emisión)`.
    ///
    /// Tras un [`Mutex`] porque insertar y purgar son mutaciones: el mapa no
    /// puede crecer sin límite con cada intento de entrada que nadie completa.
    pub(crate) logins: Arc<Mutex<HashMap<String, PendingLogin>>>,
}

impl AuthState {
    /// Construye el estado de autenticación en modo `Oidc`.
    pub(crate) fn new(settings: OidcSettings, secret: SecretString) -> Self {
        Self {
            secret,
            settings: Arc::new(settings),
            discovery: Arc::new(RwLock::new(None)),
            logins: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Registra un `state` emitido junto a su `nonce` y su marca de tiempo.
    ///
    /// Purga las entradas caducadas **al insertar**: sin esto el mapa crecería
    /// con cada login que nadie termina —un `state` que caduca sin consumirse— y
    /// el proceso acabaría guardando la memoria de intentos abandonados.
    pub async fn registrar_login(&self, state: &str, nonce: &str) {
        let ahora = Instant::now();
        let mut logins = self.logins.lock().await;

        logins.retain(|_, pendiente| !pendiente.expirado_en(ahora));
        logins.insert(
            state.to_owned(),
            PendingLogin {
                nonce: nonce.to_owned(),
                issued_at: ahora,
            },
        );
    }
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
            AuthMode::Oidc(settings) => match &config.secret {
                Some(secret) => Some(AuthState::new(settings.clone(), secret.clone())),
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
