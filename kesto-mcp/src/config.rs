//! La configuración del servidor, leída del entorno.
//!
//! Dos cosas y ninguna más: dónde está la API —`KESTO_URL`, **obligatoria**— y con qué
//! token hablarle por defecto —`KESTO_API_TOKEN`, opcional—. Se lee del entorno en el
//! arranque y nunca se escribe en el código ni en un fichero versionado.
//!
//! [`Config::from_lookup`] recibe la lectura del entorno como parámetro en vez de llamar a
//! `std::env::var` por dentro porque las pruebas tienen que inyectar valores sin tocar el
//! entorno del proceso: el entorno es global y una prueba que lo escribe deja de ser
//! reproducible.

use std::fmt;

use zeroize::Zeroize;

/// La variable que declara la URL base de la API de Kesto.
pub const KESTO_URL: &str = "KESTO_URL";

/// La variable con el token de API por defecto, cuando la petición no trae uno.
pub const KESTO_API_TOKEN: &str = "KESTO_API_TOKEN";

/// La variable con la dirección de escucha del transporte HTTP.
pub const KESTO_MCP_BIND_ADDRESS: &str = "KESTO_MCP_BIND_ADDRESS";

/// La dirección de escucha del transporte HTTP cuando no se configura ninguna.
///
/// Es `127.0.0.1` y no `0.0.0.0` a propósito: un servidor de herramientas que se expone
/// por defecto a toda la red es una puerta abierta sin querer, y quien quiera publicarlo
/// lo dice explícitamente.
pub const BIND_ADDRESS_POR_DEFECTO: &str = "127.0.0.1:8931";

/// La variable con la lista de hosts admitidos por el transporte HTTP, separados por comas.
///
/// No lleva defecto: sin ella valen los que trae el SDK —las direcciones de bucle local—,
/// que es lo correcto para un uso local y lo que impide que un nombre de dominio ajeno se
/// haga pasar por el servidor. Un despliegue con nombre propio la pone.
pub const KESTO_MCP_ALLOWED_HOSTS: &str = "KESTO_MCP_ALLOWED_HOSTS";

/// Un token de API que no se puede imprimir.
///
/// Es un `String` con dos propiedades: su [`Debug`] no publica el valor —así que un
/// `#[derive(Debug)]` en cualquier estructura que lo contenga tampoco lo publica— y se
/// borra de la memoria al soltarlo. El valor solo sale por [`SecretToken::expose`], que es
/// el punto por el que pasa la cabecera `Authorization`.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretToken(String);

impl SecretToken {
    /// Envuelve un valor ya recortado.
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// El valor en claro. El único sitio que lo usa es quien compone la cabecera.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretToken(<oculto>)")
    }
}

impl Drop for SecretToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Lo que impide arrancar.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Falta una variable obligatoria. Nombra la variable, que es lo único útil que puede
    /// decir: qué hacer para arreglarlo.
    #[error(
        "falta la variable de entorno `{variable}`: es obligatoria; por ejemplo \
         `{variable}=http://127.0.0.1:8080`"
    )]
    FaltaVariable {
        /// El nombre de la variable que falta.
        variable: &'static str,
    },
    /// La variable está puesta pero vacía. Es error de arranque y **no** un valor por
    /// defecto: una variable vacía es casi siempre una interpolación que no se resolvió en
    /// el `compose`, y caer al defecto en silencio esconde el fallo.
    #[error("la variable de entorno `{variable}` está puesta pero vacía: quítala o dale un valor")]
    VariableVacia {
        /// El nombre de la variable vacía.
        variable: &'static str,
    },
}

/// La configuración del servidor.
#[derive(Clone)]
pub struct Config {
    base_url: String,
    token: Option<SecretToken>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("base_url", &self.base_url)
            .field("token", &self.token.is_some())
            .finish()
    }
}

impl Config {
    /// Lee la configuración del entorno del proceso.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(&|nombre| std::env::var(nombre).ok())
    }

    /// Lee la configuración de una lectura inyectada.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let base_url = match lookup(KESTO_URL) {
            Some(valor) if !valor.trim().is_empty() => {
                valor.trim().trim_end_matches('/').to_owned()
            }
            Some(_) => {
                return Err(ConfigError::VariableVacia {
                    variable: KESTO_URL,
                });
            }
            None => {
                return Err(ConfigError::FaltaVariable {
                    variable: KESTO_URL,
                });
            }
        };

        // Un `KESTO_API_TOKEN` vacío se trata como ausente y no como error: en `stdio` es
        // el caso normal —se usa la configuración del cliente MCP, no una cabecera— y no
        // hay ninguna decisión equivocada que esconder.
        let token = lookup(KESTO_API_TOKEN)
            .filter(|valor| !valor.trim().is_empty())
            .map(|valor| SecretToken::new(valor.trim().to_owned()));

        Ok(Self { base_url, token })
    }

    /// La URL base de la API, sin la barra final.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// El token del entorno, si lo hay.
    pub fn env_token(&self) -> Option<&SecretToken> {
        self.token.as_ref()
    }

    /// La dirección de escucha del transporte HTTP: la del entorno o el defecto.
    pub fn bind_address(lookup: &dyn Fn(&str) -> Option<String>) -> String {
        lookup(KESTO_MCP_BIND_ADDRESS)
            .filter(|valor| !valor.trim().is_empty())
            .unwrap_or_else(|| BIND_ADDRESS_POR_DEFECTO.to_owned())
    }

    /// Los hosts admitidos por el transporte HTTP.
    ///
    /// Vacío significa «los del SDK», no «ninguno»: es la diferencia entre no configurar
    /// nada —y quedarse con la lista blanca de bucle local del SDK— y, algún día, querer
    /// desactivar la validación, que no es lo mismo y no se hace por omisión.
    pub fn allowed_hosts(lookup: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
        lookup(KESTO_MCP_ALLOWED_HOSTS)
            .map(|valor| {
                valor
                    .split(',')
                    .map(str::trim)
                    .filter(|host| !host.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{BIND_ADDRESS_POR_DEFECTO, Config, ConfigError, KESTO_URL, SecretToken};

    /// Una lectura del entorno a partir de pares sueltos.
    fn lookup(pares: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let pares: Vec<(String, String)> = pares
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();

        move |nombre: &str| {
            pares
                .iter()
                .find(|(clave, _)| clave == nombre)
                .map(|(_, valor)| valor.clone())
        }
    }

    #[test]
    fn sin_url_no_hay_configuracion_y_el_error_nombra_la_variable() {
        let error = Config::from_lookup(&lookup(&[])).expect_err("sin KESTO_URL no arranca");

        assert!(matches!(error, ConfigError::FaltaVariable { .. }));
        assert!(
            error.to_string().contains(KESTO_URL),
            "el error tiene que nombrar la variable que falta: {error}"
        );
    }

    #[test]
    fn una_url_vacia_es_error_y_no_un_defecto() {
        let error =
            Config::from_lookup(&lookup(&[(KESTO_URL, "  ")])).expect_err("una URL vacía no vale");

        assert!(error.to_string().contains(KESTO_URL));
    }

    #[test]
    fn la_url_se_recorta_y_pierde_la_barra_final() {
        let config = Config::from_lookup(&lookup(&[(KESTO_URL, "  http://127.0.0.1:8080/  ")]))
            .expect("la configuración es válida");

        assert_eq!(config.base_url(), "http://127.0.0.1:8080");
    }

    #[test]
    fn el_token_del_entorno_es_opcional() {
        let config = Config::from_lookup(&lookup(&[(KESTO_URL, "http://a")]))
            .expect("sin token la configuración es válida");

        assert!(config.env_token().is_none());

        let config = Config::from_lookup(&lookup(&[
            (KESTO_URL, "http://a"),
            ("KESTO_API_TOKEN", " "),
        ]))
        .expect("un token en blanco se trata como ausente");

        assert!(config.env_token().is_none());
    }

    #[test]
    fn el_token_nunca_sale_en_el_debug() {
        let config = Config::from_lookup(&lookup(&[
            (KESTO_URL, "http://a"),
            ("KESTO_API_TOKEN", "secreto-que-no-puede-salir"),
        ]))
        .expect("la configuración es válida");

        let impreso = format!("{config:?}");

        assert!(!impreso.contains("secreto-que-no-puede-salir"), "{impreso}");

        let token = SecretToken::new("secreto-que-no-puede-salir".to_owned());

        assert!(!format!("{token:?}").contains("secreto-que-no-puede-salir"));
        assert_eq!(token.expose(), "secreto-que-no-puede-salir");
    }

    #[test]
    fn la_direccion_de_escucha_tiene_defecto() {
        assert_eq!(Config::bind_address(&lookup(&[])), BIND_ADDRESS_POR_DEFECTO);
        assert_eq!(
            Config::bind_address(&lookup(&[("KESTO_MCP_BIND_ADDRESS", "0.0.0.0:9000")])),
            "0.0.0.0:9000"
        );
        assert_eq!(
            Config::bind_address(&lookup(&[("KESTO_MCP_BIND_ADDRESS", "  ")])),
            BIND_ADDRESS_POR_DEFECTO
        );
    }
}
