//! Configuración efectiva del proceso, leída del entorno una sola vez al arrancar.

use std::net::SocketAddr;

use thiserror::Error;

use crate::secret::SecretString;

/// Nombre de la variable que fija el filtro de registro (§8 del SRS).
const ENV_RUST_LOG: &str = "RUST_LOG";
/// Nombre de la variable que fija la dirección de escucha (§8 del SRS).
const ENV_BIND_ADDRESS: &str = "BIND_ADDRESS";
/// Nombre de la variable que fija la cadena de base de datos (§8 del SRS).
const ENV_DATABASE_URL: &str = "DATABASE_URL";
/// Clave maestra del cifrado en reposo: obligatoria siempre.
const ENV_KESTO_MASTER_KEY: &str = "KESTO_MASTER_KEY";
/// Clave de firma de la sesión: obligatoria solo con la autenticación encendida.
const ENV_KESTO_SECRET: &str = "KESTO_SECRET";
/// Emisor del proveedor OIDC.
const ENV_OIDC_ISSUER_URL: &str = "OIDC_ISSUER_URL";
/// Identificador público del cliente OIDC.
const ENV_OIDC_CLIENT_ID: &str = "OIDC_CLIENT_ID";
/// Secreto del cliente OIDC.
const ENV_OIDC_CLIENT_SECRET: &str = "OIDC_CLIENT_SECRET";
/// URL de retorno del proveedor OIDC.
const ENV_OIDC_REDIRECT_URL: &str = "OIDC_REDIRECT_URL";

/// Filtro de registro por defecto.
///
/// Publicada para que el arranque la use en vez de un literal propio: con dos
/// fuentes de verdad, cambiar el defecto aquí no cambiaría nada en el proceso.
pub const DEFAULT_LOG_FILTER: &str = "info";
/// Dirección de escucha por defecto: la del §8 del SRS.
const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:8080";
/// Cadena de base de datos por defecto: la del §8 del SRS.
const DEFAULT_DATABASE_URL: &str = "sqlite:///app/data/kesto.db";
/// URL de retorno por defecto: el callback local, para el desarrollo con proveedor.
const DEFAULT_OIDC_REDIRECT_URL: &str = "http://localhost:8080/api/v1/auth/callback";
/// Longitud mínima que se le exige a una credencial antes de avisar.
const MIN_CREDENTIAL_LENGTH: usize = 32;

/// Configuración del proveedor OIDC.
#[derive(Debug, Clone)]
pub struct OidcSettings {
    /// Emisor (`issuer`) del proveedor.
    pub issuer: String,
    /// Identificador público del cliente.
    pub client_id: String,
    /// Secreto del cliente: no aparece ni en `Debug` ni en el volcado de arranque.
    pub client_secret: SecretString,
    /// URL a la que el proveedor devuelve al usuario autenticado.
    pub redirect_url: String,
}

/// Modo de autenticación efectivo.
#[derive(Debug, Clone)]
pub enum AuthMode {
    /// Autenticación OIDC encendida.
    Oidc(OidcSettings),
    /// Sin autenticación: es el modo de desarrollo y se anuncia en voz alta.
    Disabled,
}

impl AuthMode {
    /// Identificador del modo tal y como lo publica la API.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Oidc(_) => "oidc",
            Self::Disabled => "disabled",
        }
    }
}

/// Configuración efectiva del proceso.
#[derive(Debug, Clone)]
pub struct Config {
    /// Dirección en la que el servidor escucha.
    pub bind_address: SocketAddr,
    /// Cadena de conexión de la base de datos (hoy, una ruta SQLite).
    pub database_url: String,
    /// Modo de autenticación efectivo.
    pub auth: AuthMode,
    /// Clave de firma de la sesión: obligatoria solo con la autenticación encendida.
    pub secret: Option<SecretString>,
    /// Clave maestra del cifrado en reposo: obligatoria siempre.
    pub master_key: SecretString,
    /// Filtro de registro con el que arranca el proceso.
    pub log_filter: String,
}

/// Fallo de configuración: la aplicación no puede arrancar.
///
/// El mensaje nombra la variable y nunca su valor.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Falta una variable obligatoria.
    #[error("falta la variable de entorno obligatoria {name}")]
    MissingVariable {
        /// Nombre de la variable ausente.
        name: &'static str,
    },

    /// Una variable presente trae un valor que no se puede usar.
    #[error("la variable de entorno {name} tiene un valor con formato inválido")]
    InvalidValue {
        /// Nombre de la variable mal formada.
        name: &'static str,
    },

    /// La configuración de OIDC está a medias: unas variables sí y otras no.
    #[error("la configuración de OIDC está incompleta: falta {name}")]
    IncompleteOidc {
        /// Nombre de la variable que falta.
        name: &'static str,
    },
}

/// Lectura de variables de entorno.
///
/// La lectura se inyecta para que las pruebas no toquen el entorno del proceso:
/// en `edition = "2024"`, mutarlo no es seguro y no sería reproducible en paralelo.
pub trait Lookup {
    /// Valor de `name`, si está definido.
    fn get(&self, name: &str) -> Option<String>;
}

impl<F> Lookup for F
where
    F: Fn(&str) -> Option<String>,
{
    fn get(&self, name: &str) -> Option<String> {
        self(name)
    }
}

/// Lectura de las variables del entorno real del proceso.
fn env_lookup(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

impl Config {
    /// Carga la configuración desde `lookup`.
    ///
    /// Nunca consulta el entorno del proceso: la lectura entra por parámetro.
    pub fn from_lookup(lookup: &impl Lookup) -> Result<Self, ConfigError> {
        // `KESTO_MASTER_KEY` es obligatoria siempre: un despliegue que arranca
        // sin clave maestra es el que guardará credenciales en claro mañana.
        let master_key = lookup
            .get(ENV_KESTO_MASTER_KEY)
            .ok_or(ConfigError::MissingVariable {
                name: ENV_KESTO_MASTER_KEY,
            })?;

        // Un valor presente pero inutilizable es un error de arranque, no un
        // valor degradado en silencio.
        let bind_address = lookup
            .get(ENV_BIND_ADDRESS)
            .unwrap_or_else(|| DEFAULT_BIND_ADDRESS.to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| ConfigError::InvalidValue {
                name: ENV_BIND_ADDRESS,
            })?;

        let database_url = lookup
            .get(ENV_DATABASE_URL)
            .unwrap_or_else(|| DEFAULT_DATABASE_URL.to_owned());

        // Una `RUST_LOG` presente pero en blanco es un valor presente e
        // inutilizable: `EnvFilter` interpreta la cadena vacía como «todo
        // apagado», así que el proceso arrancaría sin una sola línea de registro
        // y en silencio. Es el artefacto típico de un `compose` que interpola
        // una variable que no definiste, y degradar a «sin registro» convierte
        // un error de configuración en un servicio ciego: se falla en alto, como
        // con `BIND_ADDRESS=` vacío. Solo el blanco puro falla: sobre cualquier
        // valor con contenido manda el recorte.
        let log_filter = match lookup.get(ENV_RUST_LOG) {
            None => DEFAULT_LOG_FILTER.to_owned(),
            Some(raw) if raw.trim().is_empty() => {
                return Err(ConfigError::InvalidValue { name: ENV_RUST_LOG });
            }
            // El recorte no es cosmética: `EnvFilter` interpreta una palabra
            // suelta sin `=` como el **nombre de un destino**, no como un nivel,
            // así que `" info "` se lee como el destino `" info "` —que no
            // existe— y el filtro resultante no casa nada. `EnvFilter::try_new`
            // devuelve `Ok` y el proceso arranca mudo, que es justo el desenlace
            // que la validación de arriba quiere evitar. Recortado, `" info "` se
            // convierte en `"info"`, que sí es el nivel `INFO`. El operador que
            // pega una variable con espacios —lo que hace medio mundo al copiar
            // de un `docker-compose.yml`— no se queda sin registros.
            Some(raw) => raw.trim().to_owned(),
        };

        let auth = Self::resolve_auth(
            lookup.get(ENV_OIDC_ISSUER_URL),
            lookup.get(ENV_OIDC_CLIENT_ID),
            lookup.get(ENV_OIDC_CLIENT_SECRET),
            lookup.get(ENV_OIDC_REDIRECT_URL),
        )?;

        // Sin autenticación no hay sesión que firmar: `KESTO_SECRET` solo es
        // obligatoria cuando el modo es `Oidc`.
        let secret = match &auth {
            AuthMode::Oidc(_) => {
                let valor = lookup
                    .get(ENV_KESTO_SECRET)
                    .ok_or(ConfigError::MissingVariable {
                        name: ENV_KESTO_SECRET,
                    })?;
                Some(SecretString::new(valor))
            }
            // En desarrollo no hace falta, pero si está puesta se conserva: es
            // la que firmará las sesiones el día que se encienda el proveedor.
            AuthMode::Disabled => lookup.get(ENV_KESTO_SECRET).map(SecretString::new),
        };

        Ok(Self {
            bind_address,
            database_url,
            auth,
            secret,
            master_key: SecretString::new(master_key),
            log_filter,
        })
    }

    /// Deduce el modo de autenticación de la presencia de las variables OIDC.
    ///
    /// Los desenlaces son exactamente tres: las tres presentes dan `Oidc`,
    /// ninguna da `Disabled` y una configuración a medias no arranca. Degradar
    /// a `Disabled` dejaría el panel abierto sin que nadie se entere.
    fn resolve_auth(
        issuer: Option<String>,
        client_id: Option<String>,
        client_secret: Option<String>,
        redirect_url: Option<String>,
    ) -> Result<AuthMode, ConfigError> {
        match (issuer, client_id, client_secret) {
            (Some(issuer), Some(client_id), Some(client_secret)) => {
                Ok(AuthMode::Oidc(OidcSettings {
                    issuer,
                    client_id,
                    client_secret: SecretString::new(client_secret),
                    redirect_url: redirect_url
                        .unwrap_or_else(|| DEFAULT_OIDC_REDIRECT_URL.to_owned()),
                }))
            }
            (None, None, None) => Ok(AuthMode::Disabled),
            (issuer, client_id, _) => {
                // El mensaje nombra la variable que falta, nunca su valor.
                let name = if issuer.is_none() {
                    ENV_OIDC_ISSUER_URL
                } else if client_id.is_none() {
                    ENV_OIDC_CLIENT_ID
                } else {
                    ENV_OIDC_CLIENT_SECRET
                };

                Err(ConfigError::IncompleteOidc { name })
            }
        }
    }

    /// Carga la configuración desde el entorno real del proceso.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(&env_lookup)
    }

    /// Volcado de la configuración efectiva, con las credenciales redactadas.
    ///
    /// La redacción no es un cuidado de este método: la lleva [`SecretString`],
    /// que es quien impide que el valor salga por `Display`.
    pub fn describe(&self) -> serde_json::Value {
        let mut volcado = serde_json::json!({
            "bind_address": self.bind_address.to_string(),
            "database_url": self.database_url,
            "log_filter": self.log_filter,
            "auth_mode": self.auth.as_str(),
            "master_key": self.master_key.to_string(),
            "secret": self.secret.as_ref().map(ToString::to_string),
        });

        if let AuthMode::Oidc(settings) = &self.auth {
            volcado["oidc"] = serde_json::json!({
                "issuer": settings.issuer,
                "client_id": settings.client_id,
                "client_secret": settings.client_secret.to_string(),
                "redirect_url": settings.redirect_url,
            });
        }

        volcado
    }

    /// Avisos sobre la configuración efectiva: credenciales débiles y modo desarrollo.
    ///
    /// Es una función pura: devuelve los avisos, no los registra. Una credencial
    /// **ausente** no avisa — la ausencia la decide el modo de autenticación, y
    /// en desarrollo es legítima.
    pub fn warnings(&self) -> Vec<String> {
        let mut avisos = Vec::new();

        if matches!(self.auth, AuthMode::Disabled) {
            avisos.push(
                "la autenticación está apagada: modo de desarrollo, sin sesión que verificar"
                    .to_owned(),
            );
        }

        if self.master_key.expose().len() < MIN_CREDENTIAL_LENGTH {
            avisos.push(format!(
                "{ENV_KESTO_MASTER_KEY} mide menos de {MIN_CREDENTIAL_LENGTH} caracteres"
            ));
        }

        if let Some(secret) = &self.secret
            && secret.expose().len() < MIN_CREDENTIAL_LENGTH
        {
            avisos.push(format!(
                "{ENV_KESTO_SECRET} mide menos de {MIN_CREDENTIAL_LENGTH} caracteres"
            ));
        }

        avisos
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthMode, Config, ConfigError};
    use crate::test_support::lookup_from;

    /// Clave maestra distinguible, con la longitud de sobra.
    const MASTER_KEY: &str = "clave-maestra-distinguible-000000000000000000000000000000000000";
    /// Clave de firma distinguible, con la longitud de sobra.
    const SESSION_SECRET: &str = "clave-de-firma-distinguible-000000000000000000000000000000000000";
    /// Secreto de cliente OIDC distinguible.
    const OIDC_CLIENT_SECRET: &str = "secreto-de-cliente-oidc-distinguible";

    /// Emisor del proveedor de pruebas.
    const OIDC_ISSUER: &str = "https://idp.example.test/realms/kesto";
    /// Identificador del cliente de pruebas.
    const OIDC_CLIENT_ID: &str = "kesto-panel";
    /// Dirección de escucha propia, para detectarla en el volcado.
    const BIND: &str = "127.0.0.1:19090";
    /// Cadena de base de datos propia, para detectarla en el volcado.
    const DATABASE: &str = "sqlite:///tmp/kesto-distinguible.db";

    #[test]
    fn el_compose_del_srs_se_lee_sin_traduccion() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("BIND_ADDRESS", "0.0.0.0:9090"),
            ("DATABASE_URL", "sqlite:///tmp/x.db"),
        ]);

        let config = Config::from_lookup(&lookup).expect("el compose del SRS es válido");

        assert_eq!(config.bind_address.to_string(), "0.0.0.0:9090");
        assert_eq!(config.database_url, "sqlite:///tmp/x.db");
    }

    #[test]
    fn sin_variables_ganan_los_defectos_del_srs() {
        // `KESTO_MASTER_KEY` es obligatoria siempre, así que la lectura "vacía"
        // solo puede dejar sin declarar las variables que tienen valor por defecto.
        let lookup = lookup_from(&[("KESTO_MASTER_KEY", MASTER_KEY)]);

        let config =
            Config::from_lookup(&lookup).expect("sin más variables, arranca con los del SRS");

        assert_eq!(config.bind_address.to_string(), "0.0.0.0:8080");
        assert_eq!(config.database_url, "sqlite:///app/data/kesto.db");
        assert_eq!(config.log_filter, "info");
    }

    #[test]
    fn sin_clave_maestra_no_se_arranca() {
        let lookup = lookup_from(&[("BIND_ADDRESS", BIND), ("DATABASE_URL", DATABASE)]);

        let error = Config::from_lookup(&lookup).expect_err("sin KESTO_MASTER_KEY no se arranca");
        let mensaje = error.to_string();

        assert!(
            matches!(&error, ConfigError::MissingVariable { name } if *name == "KESTO_MASTER_KEY"),
            "el error no señala la variable ausente: {mensaje}"
        );
        assert!(
            mensaje.contains("KESTO_MASTER_KEY"),
            "el mensaje no nombra KESTO_MASTER_KEY: {mensaje}"
        );
        assert!(
            !mensaje.contains(BIND),
            "el mensaje publica el valor de BIND_ADDRESS: {mensaje}"
        );
        assert!(
            !mensaje.contains(DATABASE),
            "el mensaje publica el valor de DATABASE_URL: {mensaje}"
        );
    }

    #[test]
    fn una_direccion_sin_puerto_falla_en_alto() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("BIND_ADDRESS", "0.0.0.0"),
        ]);

        let error =
            Config::from_lookup(&lookup).expect_err("una dirección sin puerto no se acepta");
        let mensaje = error.to_string();

        assert!(
            matches!(&error, ConfigError::InvalidValue { name } if *name == "BIND_ADDRESS"),
            "el error no señala BIND_ADDRESS: {mensaje}"
        );
        assert!(
            mensaje.contains("BIND_ADDRESS"),
            "el mensaje no nombra BIND_ADDRESS: {mensaje}"
        );
        // El valor sí está en la mano en este camino, así que es aquí donde hay
        // que comprobar que el mensaje no lo publica.
        assert!(
            !mensaje.contains("0.0.0.0"),
            "el mensaje publica el valor de la variable: {mensaje}"
        );
    }

    #[test]
    fn un_filtro_de_registro_en_blanco_falla_en_alto() {
        // Una `RUST_LOG` presente y en blanco es un valor presente pero
        // inutilizable: `EnvFilter` lo lee como «todo apagado» y el proceso
        // arrancaría sin una sola línea de registro, sin que nadie se entere.
        // Los dos casos en blanco fallan igual.
        for en_blanco in ["", "   "] {
            let lookup = lookup_from(&[
                ("KESTO_MASTER_KEY", MASTER_KEY),
                ("BIND_ADDRESS", BIND),
                ("RUST_LOG", en_blanco),
            ]);

            let error = Config::from_lookup(&lookup)
                .expect_err("un filtro de registro en blanco no puede arrancar");
            let mensaje = error.to_string();

            assert!(
                matches!(&error, ConfigError::InvalidValue { name } if *name == "RUST_LOG"),
                "el error no señala RUST_LOG para {en_blanco:?}: {mensaje}"
            );
            assert!(
                mensaje.contains("RUST_LOG"),
                "el mensaje no nombra RUST_LOG para {en_blanco:?}: {mensaje}"
            );
            // El mensaje nombra la variable, nunca el valor de ninguna otra.
            assert!(
                !mensaje.contains(MASTER_KEY),
                "el mensaje publica el valor de KESTO_MASTER_KEY: {mensaje}"
            );
            assert!(
                !mensaje.contains(BIND),
                "el mensaje publica el valor de BIND_ADDRESS: {mensaje}"
            );
        }
    }

    #[test]
    fn un_filtro_con_espacios_alrededor_se_recorta() {
        // Este test nació fijando lo contrario —que `" info "` se usaba tal
        // cual— y consagraba un defecto: `EnvFilter` lee una palabra sin `=`
        // como nombre de destino, así que el valor sin recortar dejaba el
        // proceso arrancado y mudo. Se corrige la aserción; no se relaja para
        // que pase el código. No lo «restaures» pensando que era intencional.
        let lookup = lookup_from(&[("KESTO_MASTER_KEY", MASTER_KEY), ("RUST_LOG", " info ")]);

        let config =
            Config::from_lookup(&lookup).expect("un filtro con contenido no es un caso de error");

        assert_eq!(
            config.log_filter, "info",
            "los espacios sobrantes deben recortarse, no colarse en el filtro"
        );
        // Y el valor recortado tiene que ser de verdad utilizable: es lo que
        // distingue un filtro que registra de uno que arranca mudo.
        assert!(
            tracing_subscriber::EnvFilter::try_new(&config.log_filter).is_ok(),
            "el filtro recortado no lo acepta EnvFilter: {:?}",
            config.log_filter
        );
    }

    #[test]
    fn las_tres_variables_encienden_la_autenticacion() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let config = Config::from_lookup(&lookup).expect("las tres variables OIDC son válidas");

        match &config.auth {
            AuthMode::Oidc(settings) => {
                assert_eq!(settings.issuer, OIDC_ISSUER);
                assert_eq!(settings.client_id, OIDC_CLIENT_ID);
                assert_eq!(
                    settings.redirect_url, "http://localhost:8080/api/v1/auth/callback",
                    "sin OIDC_REDIRECT_URL, el retorno debe ser el callback local"
                );
            }
            AuthMode::Disabled => panic!("las tres variables OIDC deben encender la autenticación"),
        }
    }

    #[test]
    fn sin_oidc_la_autenticacion_queda_apagada_y_lo_anuncia() {
        let lookup = lookup_from(&[("KESTO_MASTER_KEY", MASTER_KEY)]);

        let config = Config::from_lookup(&lookup).expect("sin OIDC la configuración es válida");

        assert!(
            matches!(&config.auth, AuthMode::Disabled),
            "sin OIDC el modo es Disabled"
        );

        // El otro modo, también por valor exacto: el volcado tiene que publicar
        // el modo efectivo, y en este caso es `disabled`.
        assert_eq!(
            config.describe()["auth_mode"],
            "disabled",
            "el volcado no muestra el modo de autenticación efectivo"
        );

        let avisos = config.warnings().join(" | ").to_lowercase();
        assert!(
            avisos.contains("desarrollo"),
            "el arranque debe anunciar el modo de desarrollo: {avisos}"
        );
    }

    #[test]
    fn una_configuracion_oidc_a_medias_es_un_error() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
        ]);

        let error = Config::from_lookup(&lookup)
            .expect_err("una configuración OIDC a medias no puede arrancar");
        let mensaje = error.to_string();

        assert!(
            matches!(&error, ConfigError::IncompleteOidc { name } if *name == "OIDC_CLIENT_SECRET"),
            "el error no señala OIDC_CLIENT_SECRET: {mensaje}"
        );
        assert!(
            mensaje.contains("OIDC_CLIENT_SECRET"),
            "el mensaje no nombra OIDC_CLIENT_SECRET: {mensaje}"
        );
    }

    #[test]
    fn la_clave_de_firma_solo_es_obligatoria_con_oidc() {
        let con_oidc = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let error = Config::from_lookup(&con_oidc).expect_err("con OIDC falta KESTO_SECRET");

        assert!(
            matches!(&error, ConfigError::MissingVariable { name } if *name == "KESTO_SECRET"),
            "el error no señala KESTO_SECRET: {error}"
        );
        assert!(
            error.to_string().contains("KESTO_SECRET"),
            "el mensaje no nombra KESTO_SECRET: {error}"
        );

        let sin_oidc = lookup_from(&[("KESTO_MASTER_KEY", MASTER_KEY)]);

        let config =
            Config::from_lookup(&sin_oidc).expect("sin OIDC, la clave de firma no hace falta");

        assert!(matches!(&config.auth, AuthMode::Disabled));
        assert!(config.secret.is_none(), "sin OIDC no hay sesión que firmar");
    }

    #[test]
    fn los_valores_salen_de_la_lectura_inyectada() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", "clave-maestra-inyectada"),
            ("BIND_ADDRESS", "127.0.0.1:1"),
        ]);

        let config = Config::from_lookup(&lookup).expect("la lectura inyectada es válida");

        assert_eq!(config.bind_address.to_string(), "127.0.0.1:1");
        assert_eq!(config.master_key.expose(), "clave-maestra-inyectada");
    }

    #[test]
    fn el_volcado_no_publica_credenciales() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
            ("BIND_ADDRESS", BIND),
            ("DATABASE_URL", DATABASE),
        ]);

        let config = Config::from_lookup(&lookup).expect("la configuración es válida");

        let volcado_json = config.describe();
        let volcado = volcado_json.to_string();
        // El vector de `debug`/`trace` no es este volcado, sino el `Debug` de
        // `Config`: basta un `tracing::debug!(?config)` futuro para registrarlo.
        // La garantía la sostiene la redacción de `SecretString`, y es aquí
        // donde se comprueba que sigue en pie.
        let en_debug = format!("{config:?}");

        for (variable, valor) in [
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ] {
            assert!(
                !volcado.contains(valor),
                "el volcado publica {variable}: {volcado}"
            );
            assert!(
                !en_debug.contains(valor),
                "el Debug de Config publica {variable}: {en_debug}"
            );
        }
        assert!(
            volcado.contains(BIND),
            "el volcado no muestra la dirección de escucha: {volcado}"
        );
        // El positivo sobre el `Debug` es lo que hace creíble el negativo: si
        // el volcado no mostrara nada, la suite pasaría con un `Debug` vacío.
        assert!(
            en_debug.contains(BIND),
            "el Debug de Config no muestra la dirección de escucha: {en_debug}"
        );
        // Por valor, no por la palabra `oidc`: el volcado lleva una clave
        // anidada con ese nombre, así que un `contains("oidc")` pasaría aunque
        // `auth_mode` faltara.
        assert_eq!(
            volcado_json["auth_mode"], "oidc",
            "el volcado no muestra el modo de autenticación efectivo: {volcado}"
        );
    }

    #[test]
    fn una_credencial_debil_avisa_y_una_larga_no() {
        let debil = "c".repeat(12);
        let lookup = lookup_from(&[("KESTO_MASTER_KEY", &debil)]);

        let config = Config::from_lookup(&lookup)
            .expect("una credencial débil avisa, pero no impide arrancar");
        let avisos = config.warnings().join(" | ");

        assert!(
            avisos.contains("KESTO_MASTER_KEY"),
            "el aviso no nombra la variable: {avisos}"
        );
        assert!(
            avisos.contains("32"),
            "el aviso no nombra el mínimo: {avisos}"
        );

        let larga = "c".repeat(64);
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", &larga),
            ("KESTO_SECRET", &larga),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let config = Config::from_lookup(&lookup).expect("la configuración es válida");
        let avisos = config.warnings();

        assert!(
            !avisos
                .iter()
                .any(|aviso| aviso.contains("KESTO_MASTER_KEY")),
            "avisó de una clave maestra de 64 caracteres: {avisos:?}"
        );
        assert!(
            !avisos.iter().any(|aviso| aviso.contains("KESTO_SECRET")),
            "avisó de una clave de firma de 64 caracteres: {avisos:?}"
        );

        let firma_corta = "c".repeat(12);
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", &larga),
            ("KESTO_SECRET", &firma_corta),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let config =
            Config::from_lookup(&lookup).expect("una clave de firma débil no impide arrancar");
        let avisos = config.warnings().join(" | ");

        assert!(
            avisos.contains("KESTO_SECRET"),
            "el aviso no nombra la clave de firma: {avisos}"
        );
    }
}
