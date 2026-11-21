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
/// Permiso explícito del modo sin autenticación.
///
/// Es la única forma de arrancar en modo `Disabled`: sin proveedor de identidad
/// y sin este permiso, la aplicación se niega a arrancar. La razón es que la
/// ausencia de las `OIDC_*` es indistinguible de un `.env` que no se cargó o de
/// un `compose` que interpoló a vacío, y esos dos accidentes dejarían el panel
/// —y el motor, que dirige el socket del daemon— abiertos en todas las
/// interfaces. El permiso convierte el modo de desarrollo en una decisión.
const ENV_KESTO_ALLOW_INSECURE: &str = "KESTO_ALLOW_INSECURE";
/// Emisor del proveedor OIDC.
const ENV_OIDC_ISSUER_URL: &str = "OIDC_ISSUER_URL";
/// Identificador público del cliente OIDC.
const ENV_OIDC_CLIENT_ID: &str = "OIDC_CLIENT_ID";
/// Secreto del cliente OIDC.
const ENV_OIDC_CLIENT_SECRET: &str = "OIDC_CLIENT_SECRET";
/// URL de retorno del proveedor OIDC.
const ENV_OIDC_REDIRECT_URL: &str = "OIDC_REDIRECT_URL";
/// Nombre de la variable que fija el socket del motor de contenedores.
///
/// No es una elección de este proyecto: es el nombre que la biblioteca de Docker
/// lee por convención, y respetarlo deja que un `compose` que ya la define para
/// Kesto funcione sin traducir nada. Y hace falta por una razón de este entorno:
/// la máquina de desarrollo no tiene Docker, tiene Podman con una API compatible
/// en otro socket, así que una ruta fija en el binario haría imposible ejercitar
/// el motor sin un Docker de verdad.
const ENV_DOCKER_HOST: &str = "DOCKER_HOST";

/// Segundos de la cabecera `Strict-Transport-Security` (`KESTO_HSTS_MAX_AGE`).
///
/// Es opcional: sin ella la cabecera queda apagada.
const ENV_KESTO_HSTS_MAX_AGE: &str = "KESTO_HSTS_MAX_AGE";
/// Socket del motor de contenedores por defecto: la ruta del §8 del SRS.
///
/// Es la ruta que la biblioteca de Docker usa cuando `DOCKER_HOST` no está
/// informada, y por eso es también la de aquí: el defecto del proyecto y el de la
/// biblioteca tienen que ser el mismo, o el motor de producción hablaría con un
/// socket que nadie le ha dicho. Una variable con valor por defecto no es
/// obligatoria: su ausencia y su vacío caen aquí, y ninguno de los dos impide
/// arrancar —una ruta vacía es una interpolación de `compose` sin variable, no una
/// intención—.
const DEFAULT_DOCKER_HOST: &str = "unix:///var/run/docker.sock";

/// Filtro de registro por defecto.
///
/// Publicada para que el arranque la use en vez de un literal propio: con dos
/// fuentes de verdad, cambiar el defecto aquí no cambiaría nada en el proceso.
pub const DEFAULT_LOG_FILTER: &str = "info";
/// Dirección de escucha por defecto.
///
/// Es de **loopback**, y no el `0.0.0.0:8080` del §8: escuchar en todas las
/// interfaces es una decisión explícita del operador —el `compose` del §8 la
/// declara tal cual, y con proveedor de identidad— y nunca el resultado de no
/// haber dicho nada. Con el defecto en loopback, un despliegue que pierde su
/// `BIND_ADDRESS` deja de ser alcanzable desde fuera en vez de quedar expuesto.
const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8080";
/// Cadena de base de datos por defecto: la del §8 del SRS.
const DEFAULT_DATABASE_URL: &str = "sqlite:///app/data/kesto.db";
/// URL de retorno por defecto: el callback local, para el desarrollo con proveedor.
const DEFAULT_OIDC_REDIRECT_URL: &str = "http://localhost:8080/api/v1/auth/callback";
/// Longitud mínima, medida en caracteres, que se le exige a una credencial para
/// dejar arrancar.
///
/// Una credencial más corta —vacía incluida— es un error de arranque, no un
/// aviso: `KESTO_MASTER_KEY` es la que cifra el resto de los secretos, y una
/// vacía hace que HKDF derive con sal `None` una clave AES determinista y
/// públicamente calculable. `KESTO_SECRET`, en modo `Oidc`, firma las sesiones:
/// una corta permite forjar un JWT y saltarse la autenticación.
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
    /// Socket con el que el motor habla con Docker (`DOCKER_HOST`).
    ///
    /// Es una ruta y no una dirección de escucha: el socket es del motor de
    /// contenedores, no de este proceso. Y viaja **ya resuelta** —nunca `None`—
    /// porque el defecto lo aplica la carga de la configuración: quien lea este
    /// campo se conecta o falla, pero no tiene que volver a decidir cuál es la
    /// ruta por defecto. La lectura sale del `Lookup` inyectado, como el resto:
    /// el motor no lee el entorno del proceso por su cuenta, y eso es lo que deja
    /// que dos pruebas quieran dos sockets distintos a la vez.
    pub docker_host: String,

    /// Antigüedad (segundos) de la cabecera `Strict-Transport-Security` (`KESTO_HSTS_MAX_AGE`).
    ///
    /// `None` —variable ausente— deja la cabecera apagada: en el despliegue estándar
    /// el TLS lo termina un proxy, y la HSTS es suya. Con un valor presente, toda
    /// respuesta lleva `Strict-Transport-Security: max-age=<n>`.
    pub hsts_max_age: Option<u64>,
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

    /// Se pidió el modo sin autenticación sin el permiso explícito.
    #[error(
        "el modo sin autenticación exige el permiso explícito {name}=1: sin las variables OIDC la aplicación no arranca abierta"
    )]
    InsecureModeNotAllowed {
        /// Nombre de la variable que concede el permiso.
        name: &'static str,
    },

    /// El modo sin autenticación escucharía fuera de loopback.
    #[error("sin autenticación, {name} debe ser una dirección de loopback")]
    InsecureBindAddress {
        /// Nombre de la variable de la dirección de escucha.
        name: &'static str,
    },

    /// Una credencial obligatoria mide menos de lo exigido.
    ///
    /// El mensaje nombra la variable y el mínimo, y nunca el valor.
    #[error("la variable de entorno {name} debe medir al menos {min} caracteres")]
    WeakCredential {
        /// Nombre de la credencial débil.
        name: &'static str,
        /// Longitud mínima exigida.
        min: usize,
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

/// Exige a una credencial la longitud mínima y nombra la variable si no la alcanza.
///
/// La longitud se mide en **caracteres**, no en bytes: es la unidad que anuncia
/// el mensaje de [`ConfigError::WeakCredential`] y la que espera el operador. Con
/// bytes, una credencial multibyte de 16 caracteres —32 bytes— pasaría el filtro
/// creyendo medir 32 caracteres.
///
/// El mensaje lleva el nombre y el mínimo, nunca el valor: es un error de
/// arranque que un operador va a leer en un registro, y el valor no tiene por qué
/// acabar ahí.
fn exigir_longitud_minima(name: &'static str, valor: &str) -> Result<(), ConfigError> {
    if valor.chars().count() < MIN_CREDENTIAL_LENGTH {
        return Err(ConfigError::WeakCredential {
            name,
            min: MIN_CREDENTIAL_LENGTH,
        });
    }

    Ok(())
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

        // Y una clave presente pero débil es un error de arranque, no un aviso:
        // con la clave vacía, HKDF deriva con sal `None` y la etiqueta constante
        // una clave AES-256 determinista y públicamente calculable, así que
        // cualquiera que lea la base descifra las credenciales de los
        // repositorios. Un aviso que arranca igual deja el sistema en producción
        // con los secretos al alcance. Se mide el valor tal cual, sin recortar:
        // una clave con espacios es la clave.
        exigir_longitud_minima(ENV_KESTO_MASTER_KEY, &master_key)?;

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

        // La HSTS es opcional, pero una variable presente e inservible —vacía o
        // no numérica— es un error de arranque, como `RUST_LOG` o `BIND_ADDRESS`:
        // un operador que cree haber encendido la HSTS y arranca sin ella no lo
        // notaría.
        let hsts_max_age = match lookup.get(ENV_KESTO_HSTS_MAX_AGE) {
            None => None,
            Some(raw) => {
                let recortado = raw.trim();
                if recortado.is_empty() {
                    return Err(ConfigError::InvalidValue {
                        name: ENV_KESTO_HSTS_MAX_AGE,
                    });
                }
                Some(
                    recortado
                        .parse::<u64>()
                        .map_err(|_| ConfigError::InvalidValue {
                            name: ENV_KESTO_HSTS_MAX_AGE,
                        })?,
                )
            }
        };

        let auth = Self::resolve_auth(
            lookup.get(ENV_OIDC_ISSUER_URL),
            lookup.get(ENV_OIDC_CLIENT_ID),
            lookup.get(ENV_OIDC_CLIENT_SECRET),
            lookup.get(ENV_OIDC_REDIRECT_URL),
        )?;

        // La regla cruzada necesita las dos piezas —el modo y la dirección—, así
        // que va **después** de resolver el modo y sin reordenar el resto de la
        // lectura: el orden actual fija qué error se ve primero cuando hay varios
        // problemas, y cambiarlo alteraría mensajes ya probados. En modo `Oidc` el
        // permiso no se consulta siquiera: la capa de sesión se monta igual y un
        // valor inservible ahí no relaja nada.
        if matches!(auth, AuthMode::Disabled) {
            // El permiso es lo que separa un modo de desarrollo deliberado de un
            // despliegue que perdió sus `OIDC_*`. `1` permite, `0` niega y
            // cualquier otro valor —vacío incluido— es un error de formato: un
            // booleano de entorno no admite una tabla de sinónimos.
            let permitido = match lookup.get(ENV_KESTO_ALLOW_INSECURE) {
                None => false,
                Some(raw) => match raw.trim() {
                    "1" => true,
                    "0" => false,
                    _ => {
                        return Err(ConfigError::InvalidValue {
                            name: ENV_KESTO_ALLOW_INSECURE,
                        });
                    }
                },
            };

            if !permitido {
                return Err(ConfigError::InsecureModeNotAllowed {
                    name: ENV_KESTO_ALLOW_INSECURE,
                });
            }

            // Sin capa de autenticación, escuchar fuera de loopback deja la
            // superficie completa —el motor incluido, que dirige el socket del
            // daemon— alcanzable desde otra interfaz. El permiso explícito no
            // levanta esta segunda condición.
            if !bind_address.ip().is_loopback() {
                return Err(ConfigError::InsecureBindAddress {
                    name: ENV_BIND_ADDRESS,
                });
            }
        }

        // Sin autenticación no hay sesión que firmar: `KESTO_SECRET` solo se lee
        // y se valida cuando el modo es `Oidc`. En desarrollo no se toca —una
        // variable que no se usa no puede impedir el arranque— y el campo queda
        // vacío, porque no hay ninguna sesión que firmar.
        let secret = match &auth {
            AuthMode::Oidc(_) => {
                let valor = lookup
                    .get(ENV_KESTO_SECRET)
                    .ok_or(ConfigError::MissingVariable {
                        name: ENV_KESTO_SECRET,
                    })?;
                exigir_longitud_minima(ENV_KESTO_SECRET, &valor)?;
                Some(SecretString::new(valor))
            }
            AuthMode::Disabled => None,
        };

        // El socket del motor de contenedores. A diferencia de `RUST_LOG`, un
        // valor inservible **no** falla: cae al defecto. La diferencia no es
        // capricho —aquí el valor inservible no degrada nada en silencio, porque
        // el defecto es una ruta usable y el motor avisa si no contesta al
        // arrancar—. Y es el desenlace que de verdad ocurre: un `compose` que
        // interpola una variable que no definiste deja la cadena vacía, y
        // negarse a arrancar por eso convertiría un error de configuración en
        // ausencia de copias.
        //
        // El recorte está por eso mismo, y cubre lo que el razonamiento del vacío
        // **no** cubre: el vacío de `compose` es `""`, pero una variable que solo
        // lleva espacios no es ni el vacío ni una ruta, y sin recortar pasaría el
        // filtro y viajaría como socket —contra el que nadie conecta, con el aviso
        // genérico del motor como único rastro, que no distingue «aquí no hay
        // motor» de «la ruta es basura»—. Se recorta y se filtra sobre el valor
        // ya recortado, igual que `RUST_LOG`, para que las dos variables del
        // entorno se lean igual. Recortar una ruta no la estropea: un camino con
        // espacios deliberados al principio o al final no existe en la práctica
        // —habría que crear a propósito un directorio que acabe en espacio—,
        // mientras que el espacio pegado al copiar una variable es lo corriente, y
        // el recorte no toca los espacios de en medio.
        let docker_host = lookup
            .get(ENV_DOCKER_HOST)
            .map(|valor| valor.trim().to_owned())
            .filter(|valor| !valor.is_empty())
            .unwrap_or_else(|| DEFAULT_DOCKER_HOST.to_owned());

        Ok(Self {
            bind_address,
            database_url,
            auth,
            secret,
            master_key: SecretString::new(master_key),
            log_filter,
            docker_host,
            hsts_max_age,
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
            "docker_host": self.docker_host,
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

    /// Avisos sobre la configuración efectiva: el modo sin autenticación.
    ///
    /// Es una función pura: devuelve los avisos, no los registra. Las credenciales
    /// ya **no** avisan: una débil impide el arranque
    /// ([`ConfigError::WeakCredential`]), así que aquí solo queda lo que nada
    /// impide y el operador debe leer igualmente —que la autenticación está
    /// apagada—. Una credencial ausente tampoco avisa: la ausencia la decide el
    /// modo de autenticación, y en desarrollo es legítima.
    pub fn warnings(&self) -> Vec<String> {
        let mut avisos = Vec::new();

        if matches!(self.auth, AuthMode::Disabled) {
            avisos.push(
                "la autenticación está apagada: modo de desarrollo, sin sesión que verificar"
                    .to_owned(),
            );
        }

        avisos
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthMode, Config, ConfigError, DEFAULT_BIND_ADDRESS};
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
        // El §8 declara las tres `OIDC_*` y sirve en todas las interfaces: sin
        // proveedor de identidad, ese mismo `0.0.0.0:9090` no sería admisible,
        // así que el escenario solo se sostiene con el modo `Oidc` encendido.
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("BIND_ADDRESS", "0.0.0.0:9090"),
            ("DATABASE_URL", "sqlite:///tmp/x.db"),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let config = Config::from_lookup(&lookup).expect("el compose del SRS es válido");

        assert_eq!(config.bind_address.to_string(), "0.0.0.0:9090");
        assert_eq!(config.database_url, "sqlite:///tmp/x.db");
    }

    #[test]
    fn sin_variables_ganan_los_defectos_del_srs() {
        // `KESTO_MASTER_KEY` es obligatoria siempre, así que la lectura "vacía"
        // solo puede dejar sin declarar las variables que tienen valor por defecto.
        // El permiso explícito sí se declara: sin `BIND_ADDRESS` el defecto es de
        // loopback, que es la única dirección que el modo sin autenticación admite.
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
        ]);

        let config =
            Config::from_lookup(&lookup).expect("sin más variables, arranca con los del SRS");

        assert_eq!(config.bind_address.to_string(), "127.0.0.1:8080");
        assert_eq!(config.database_url, "sqlite:///app/data/kesto.db");
        assert_eq!(config.log_filter, "info");
    }

    #[test]
    fn sin_hsts_la_cabecera_queda_desactivada() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
        ]);

        let config = Config::from_lookup(&lookup).expect("sin HSTS la configuración es válida");

        assert_eq!(config.hsts_max_age, None);
    }

    #[test]
    fn la_hsts_configurada_se_lee() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("KESTO_HSTS_MAX_AGE", "31536000"),
        ]);

        let config = Config::from_lookup(&lookup).expect("una HSTS válida se acepta");

        assert_eq!(config.hsts_max_age, Some(31_536_000));
    }

    #[test]
    fn una_hsts_vacia_es_error_de_arranque() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("KESTO_HSTS_MAX_AGE", ""),
        ]);

        let error = Config::from_lookup(&lookup).expect_err("una HSTS vacía no arranca");

        assert!(
            matches!(error, ConfigError::InvalidValue { name } if name == "KESTO_HSTS_MAX_AGE"),
            "se esperaba InvalidValue para KESTO_HSTS_MAX_AGE, llegó {error:?}"
        );
    }

    #[test]
    fn una_hsts_no_numerica_es_error_de_arranque() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("KESTO_HSTS_MAX_AGE", "treinta"),
        ]);

        let error = Config::from_lookup(&lookup).expect_err("una HSTS no numérica no arranca");

        assert!(
            matches!(error, ConfigError::InvalidValue { name } if name == "KESTO_HSTS_MAX_AGE"),
            "se esperaba InvalidValue para KESTO_HSTS_MAX_AGE, llegó {error:?}"
        );
    }

    #[test]
    fn sin_clave_maestra_no_se_arranca() {
        let lookup = lookup_from(&[
            ("KESTO_ALLOW_INSECURE", "1"),
            ("BIND_ADDRESS", BIND),
            ("DATABASE_URL", DATABASE),
        ]);

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
            ("KESTO_ALLOW_INSECURE", "1"),
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
    fn una_direccion_vacia_no_cae_al_defecto() {
        // Una `BIND_ADDRESS` presente pero vacía —el artefacto de un `compose`
        // que interpola una variable que no definiste— no parsea: es un error de
        // arranque, no el `127.0.0.1:8080` por defecto. Si cayera al defecto, el
        // proceso arrancaría escuchando en una dirección que nadie pidió.
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("BIND_ADDRESS", ""),
        ]);

        let error =
            Config::from_lookup(&lookup).expect_err("una dirección vacía no puede caer al defecto");
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
        // que comprobar que el mensaje no lo publica ni deja entrever el defecto.
        assert!(
            !mensaje.contains(DEFAULT_BIND_ADDRESS),
            "el mensaje publica el valor por defecto: {mensaje}"
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
                ("KESTO_ALLOW_INSECURE", "1"),
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
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("RUST_LOG", " info "),
        ]);

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
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
        ]);

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

        let sin_oidc = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
        ]);

        let config =
            Config::from_lookup(&sin_oidc).expect("sin OIDC, la clave de firma no hace falta");

        assert!(matches!(&config.auth, AuthMode::Disabled));
        assert!(config.secret.is_none(), "sin OIDC no hay sesión que firmar");
    }

    #[test]
    fn los_valores_salen_de_la_lectura_inyectada() {
        // La clave inyectada mide los 32 caracteres que se le exigen a toda
        // credencial: el valor es distinguible, pero no débil.
        let lookup = lookup_from(&[
            (
                "KESTO_MASTER_KEY",
                "clave-maestra-inyectada-0000000000000000",
            ),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("BIND_ADDRESS", "127.0.0.1:1"),
        ]);

        let config = Config::from_lookup(&lookup).expect("la lectura inyectada es válida");

        assert_eq!(config.bind_address.to_string(), "127.0.0.1:1");
        assert_eq!(
            config.master_key.expose(),
            "clave-maestra-inyectada-0000000000000000"
        );
    }

    /// 1.32 — `DOCKER_HOST` se respeta, su ausencia cae al defecto y su blanco también.
    ///
    /// Cubre `config`, requisito 1, los escenarios del socket: la variable
    /// informada manda, sin ella el socket es la ruta por defecto de la
    /// plataforma, y una variable informada pero **inservible** —una
    /// interpolación de `compose` sin variable, o un valor que solo lleva
    /// espacios— también cae al defecto. El blanco se comprueba aquí, con el
    /// vacío, porque los dos desenlaces son el mismo por la misma razón: un valor
    /// recortado que queda en nada no es una ruta, así que no hay a dónde
    /// conectarse y el defecto es lo único que puede funcionar. Y un valor con
    /// espacios **alrededor** de una ruta de verdad se recorta y se usa: ése es
    /// el caso que separa el recorte del descarte.
    ///
    /// La prueba mira el **volcado** (`describe`) y no un campo del `Config`: el
    /// volcado es lo que el arranque registra y lo que un operador lee, así que
    /// es el camino que de verdad importa; y mirar el campo obligaría a que
    /// existiera para compilar, lo que enmascararía el rojo de las demás pruebas.
    #[test]
    fn docker_host_se_respeta_y_su_ausencia_y_su_vacio_caen_al_defecto() {
        // El escenario de la variable informada: manda tal cual, sin traducirla.
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("DOCKER_HOST", "unix:///run/user/1000/podman/podman.sock"),
        ]);

        let config = Config::from_lookup(&lookup).expect("la configuración es válida");

        assert_eq!(
            config.describe()["docker_host"],
            "unix:///run/user/1000/podman/podman.sock",
            "el volcado no muestra el socket de Docker informado"
        );

        // El escenario de la ausencia: la ruta por defecto, y sin fallar.
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
        ]);

        let config = Config::from_lookup(&lookup).expect("sin DOCKER_HOST el arranque no falla");

        assert_eq!(
            config.describe()["docker_host"],
            "unix:///var/run/docker.sock",
            "sin DOCKER_HOST el volcado no muestra la ruta por defecto"
        );

        // El escenario del vacío: también la ruta por defecto, y sin fallar.
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("DOCKER_HOST", ""),
        ]);

        let config =
            Config::from_lookup(&lookup).expect("una DOCKER_HOST vacía no impide arrancar");

        assert_eq!(
            config.describe()["docker_host"],
            "unix:///var/run/docker.sock",
            "una DOCKER_HOST vacía no cae a la ruta por defecto"
        );

        // El escenario del blanco: tampoco es una ruta, así que el desenlace es
        // el mismo que el del vacío. Sin el recorte, `" "` pasaría el filtro y
        // viajaría como ruta de socket: nadie conecta contra ella y el único
        // rastro sería el aviso del motor al no contestar el socket, que no
        // distingue «aquí no hay motor» de «la ruta es basura».
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("DOCKER_HOST", " "),
        ]);

        let config =
            Config::from_lookup(&lookup).expect("una DOCKER_HOST en blanco no impide arrancar");

        assert_eq!(
            config.describe()["docker_host"],
            "unix:///var/run/docker.sock",
            "una DOCKER_HOST en blanco no cae a la ruta por defecto"
        );

        // Y el otro lado del recorte: los espacios **alrededor** de una ruta de
        // verdad no la invalidan. Es el caso de pegar una variable desde un
        // `compose`, y el motivo de recortar en vez de limitarse a descartar el
        // vacío.
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("DOCKER_HOST", " unix:///run/user/1000/podman/podman.sock "),
        ]);

        let config = Config::from_lookup(&lookup).expect("una DOCKER_HOST con espacios es válida");

        assert_eq!(
            config.describe()["docker_host"],
            "unix:///run/user/1000/podman/podman.sock",
            "el blanco alrededor de una DOCKER_HOST de verdad no se recorta"
        );
    }

    /// 1.33 — Dos configuraciones con sockets distintos conviven.
    ///
    /// Cubre `config`, requisito 2: la lectura se inyecta como la del resto de la
    /// configuración, así que dos pruebas pueden querer dos sockets distintos a
    /// la vez. Cada volcado ve **el suyo**, y ninguno ve el del otro.
    ///
    /// Y la variable del proceso no se toca: eso no se comprueba con una lectura
    /// —este módulo no puede consultarla sin volver a la lectura del entorno, que
    /// es justo lo que la regla prohíbe— sino por la forma del código, que
    /// recibe el `Lookup` y nunca consulta el proceso.
    #[test]
    fn dos_configuraciones_con_sockets_distintos_conviven() {
        let una = Config::from_lookup(&lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("DOCKER_HOST", "unix:///run/user/1000/podman/podman.sock"),
        ]))
        .expect("la primera configuración es válida");

        let otra = Config::from_lookup(&lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("DOCKER_HOST", "unix:///no/existe/kesto-prueba.sock"),
        ]))
        .expect("la segunda configuración es válida");

        assert_eq!(
            una.describe()["docker_host"],
            "unix:///run/user/1000/podman/podman.sock",
            "la primera configuración no ve su socket"
        );
        assert_eq!(
            otra.describe()["docker_host"],
            "unix:///no/existe/kesto-prueba.sock",
            "la segunda configuración no ve su socket"
        );
        assert_ne!(
            una.describe()["docker_host"],
            otra.describe()["docker_host"],
            "las dos configuraciones no pueden ver el mismo socket"
        );
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
    fn una_credencial_debil_impide_el_arranque() {
        // La clave maestra corta ya no avisa: impide arrancar. El mensaje nombra
        // la variable y el mínimo, y jamás el valor.
        let debil = "c".repeat(12);
        let lookup = lookup_from(&[("KESTO_MASTER_KEY", &debil), ("KESTO_ALLOW_INSECURE", "1")]);

        let error =
            Config::from_lookup(&lookup).expect_err("una clave maestra débil no puede arrancar");
        let mensaje = error.to_string();

        assert!(
            mensaje.contains("KESTO_MASTER_KEY"),
            "el error no nombra la variable: {mensaje}"
        );
        assert!(
            mensaje.contains("32"),
            "el error no nombra el mínimo: {mensaje}"
        );

        // La clave de firma sigue el mismo contrato, pero solo con OIDC: es
        // cuando se usa y cuando es obligatoria.
        let larga = "c".repeat(64);
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", &larga),
            ("KESTO_SECRET", &debil),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let error = Config::from_lookup(&lookup)
            .expect_err("con OIDC una clave de firma débil no puede arrancar");
        let mensaje = error.to_string();

        assert!(
            mensaje.contains("KESTO_SECRET"),
            "el error no nombra la clave de firma: {mensaje}"
        );
        assert!(
            mensaje.contains("32"),
            "el error no nombra el mínimo: {mensaje}"
        );
    }

    /// 1.1 — Sin proveedor y sin permiso, el modo de desarrollo no arranca.
    #[test]
    fn sin_permiso_explicito_el_modo_sin_autenticacion_no_arranca() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("BIND_ADDRESS", "127.0.0.1:8080"),
        ]);

        let error = Config::from_lookup(&lookup)
            .expect_err("sin KESTO_ALLOW_INSECURE el modo sin autenticación no puede arrancar");
        let mensaje = error.to_string();

        assert!(
            mensaje.contains("KESTO_ALLOW_INSECURE"),
            "el mensaje no nombra el permiso que falta: {mensaje}"
        );
        assert!(
            !mensaje.contains("127.0.0.1"),
            "el mensaje publica el valor de BIND_ADDRESS: {mensaje}"
        );
    }

    /// 1.2 — Con el permiso explícito y un bind de loopback, el modo arranca.
    #[test]
    fn con_permiso_explicito_el_modo_de_desarrollo_arranca() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("BIND_ADDRESS", "127.0.0.1:8080"),
        ]);

        let config =
            Config::from_lookup(&lookup).expect("el modo de desarrollo con permiso arranca");

        assert!(
            matches!(&config.auth, AuthMode::Disabled),
            "sin OIDC el modo es Disabled"
        );
        assert_eq!(config.bind_address.to_string(), "127.0.0.1:8080");
    }

    /// 1.3 — Un permiso que no es `1` ni `0` es un error de formato.
    #[test]
    fn un_permiso_que_no_es_uno_ni_cero_es_error_de_formato() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "true"),
            ("BIND_ADDRESS", "127.0.0.1:8080"),
        ]);

        let error =
            Config::from_lookup(&lookup).expect_err("`true` no es un valor de permiso válido");
        let mensaje = error.to_string();

        assert!(
            mensaje.contains("KESTO_ALLOW_INSECURE"),
            "el mensaje no nombra la variable: {mensaje}"
        );
        assert!(
            !mensaje.contains("true"),
            "el mensaje publica el valor del permiso: {mensaje}"
        );
    }

    /// 1.4 — `0` y la ausencia significan lo mismo: permiso negado.
    #[test]
    fn el_permiso_negado_no_arranca_como_su_ausencia() {
        let ausente = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("BIND_ADDRESS", "127.0.0.1:8080"),
        ]);
        let negado = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "0"),
            ("BIND_ADDRESS", "127.0.0.1:8080"),
        ]);

        let error_ausente =
            Config::from_lookup(&ausente).expect_err("sin permiso el modo no puede arrancar");
        let error_negado =
            Config::from_lookup(&negado).expect_err("con el permiso negado tampoco arranca");

        assert_eq!(
            error_ausente.to_string(),
            error_negado.to_string(),
            "el permiso negado debe fallar igual que su ausencia"
        );
        assert!(
            error_negado.to_string().contains("KESTO_ALLOW_INSECURE"),
            "el mensaje no nombra el permiso: {error_negado}"
        );
    }

    /// 1.5 — Con proveedor de identidad, el permiso no hace falta ni cambia nada.
    #[test]
    fn con_oidc_el_permiso_no_hace_falta() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let config =
            Config::from_lookup(&lookup).expect("con OIDC no se exige KESTO_ALLOW_INSECURE");

        assert!(
            matches!(&config.auth, AuthMode::Oidc(_)),
            "las tres OIDC encienden la autenticación sin permiso adicional"
        );
    }

    /// 1.6 — Sin autenticación, escuchar fuera de loopback no arranca.
    #[test]
    fn sin_autenticacion_y_en_todas_las_interfaces_no_arranca() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("BIND_ADDRESS", "0.0.0.0:8080"),
        ]);

        let error = Config::from_lookup(&lookup)
            .expect_err("sin autenticación no se puede escuchar en todas las interfaces");
        let mensaje = error.to_string();

        assert!(
            mensaje.contains("BIND_ADDRESS"),
            "el mensaje no nombra la dirección: {mensaje}"
        );
        assert!(
            !mensaje.contains("0.0.0.0"),
            "el mensaje publica el valor de BIND_ADDRESS: {mensaje}"
        );
    }

    /// 1.7 — Con autenticación, escuchar en todas las interfaces es legítimo.
    #[test]
    fn con_autenticacion_todas_las_interfaces_son_legitimas() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("BIND_ADDRESS", "0.0.0.0:8080"),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let config = Config::from_lookup(&lookup)
            .expect("con autenticación, 0.0.0.0:8080 es una decisión legítima");

        assert_eq!(config.bind_address.to_string(), "0.0.0.0:8080");
    }

    /// 2.1 — Una clave maestra presente y vacía no arranca.
    #[test]
    fn una_clave_maestra_vacia_impide_el_arranque() {
        let lookup = lookup_from(&[("KESTO_MASTER_KEY", ""), ("KESTO_ALLOW_INSECURE", "1")]);

        let error =
            Config::from_lookup(&lookup).expect_err("una clave maestra vacía no puede arrancar");

        assert!(
            error.to_string().contains("KESTO_MASTER_KEY"),
            "el mensaje no nombra la variable: {error}"
        );
    }

    /// 2.2 — Una clave maestra corta no arranca y el mensaje nombra el mínimo.
    #[test]
    fn una_clave_maestra_corta_impide_el_arranque() {
        let corta = "c".repeat(12);
        let lookup = lookup_from(&[("KESTO_MASTER_KEY", &corta), ("KESTO_ALLOW_INSECURE", "1")]);

        let error =
            Config::from_lookup(&lookup).expect_err("una clave maestra corta no puede arrancar");
        let mensaje = error.to_string();

        assert!(
            mensaje.contains("KESTO_MASTER_KEY"),
            "el mensaje no nombra la variable: {mensaje}"
        );
        assert!(
            mensaje.contains("32"),
            "el mensaje no nombra el mínimo: {mensaje}"
        );
    }

    /// 2.2.b — La longitud se mide en caracteres, no en bytes.
    #[test]
    fn una_credencial_multibyte_corta_impide_el_arranque() {
        // 16 caracteres acentuados son 32 bytes: con la medición en bytes este
        // valor pasaría el filtro creyendo medir 32 caracteres. El mensaje
        // promete caracteres, así que el código debe medir caracteres.
        let multibyte = "á".repeat(16);
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", &multibyte),
            ("KESTO_ALLOW_INSECURE", "1"),
        ]);

        let error = Config::from_lookup(&lookup)
            .expect_err("16 caracteres no alcanzan el mínimo de 32, aunque ocupen 32 bytes");

        assert!(
            matches!(&error, ConfigError::WeakCredential { name, .. } if *name == "KESTO_MASTER_KEY"),
            "el error no señala una credencial débil: {error}"
        );
    }

    /// 2.3 — En modo OIDC una clave de firma vacía no arranca.
    #[test]
    fn en_modo_oidc_una_clave_de_firma_vacia_impide_el_arranque() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", ""),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let error = Config::from_lookup(&lookup)
            .expect_err("con OIDC una clave de firma vacía no puede arrancar");

        assert!(
            error.to_string().contains("KESTO_SECRET"),
            "el mensaje no nombra la clave de firma: {error}"
        );
    }

    /// 2.4 — En modo OIDC una clave de firma corta no arranca.
    #[test]
    fn en_modo_oidc_una_clave_de_firma_corta_impide_el_arranque() {
        let corta = "c".repeat(12);
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", &corta),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let error = Config::from_lookup(&lookup)
            .expect_err("con OIDC una clave de firma corta no puede arrancar");
        let mensaje = error.to_string();

        assert!(
            mensaje.contains("KESTO_SECRET"),
            "el mensaje no nombra la clave de firma: {mensaje}"
        );
        assert!(
            mensaje.contains("32"),
            "el mensaje no nombra el mínimo: {mensaje}"
        );
    }

    /// 2.5 — Dos credenciales largas arrancan y no emiten ningún aviso.
    #[test]
    fn dos_credenciales_largas_arrancan_sin_aviso() {
        let larga = "c".repeat(64);
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", &larga),
            ("KESTO_SECRET", &larga),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let config = Config::from_lookup(&lookup).expect("dos credenciales largas son válidas");
        let avisos = config.warnings();

        // En modo `Oidc` no hay ningún aviso legítimo que emitir: la lista debe
        // estar vacía. Comprobar solo que no nombran estas dos variables pasaría
        // aunque se redactara un aviso con cualquier otro texto.
        assert!(avisos.is_empty(), "avisos inesperados: {avisos:?}");
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
    }

    /// 2.6 — El error de credencial débil no publica el valor.
    #[test]
    fn el_error_de_credencial_debil_no_publica_el_valor() {
        // Un valor reconocible, para que un `contains` no dé un falso positivo.
        let valor = "valor-secreto";
        let lookup = lookup_from(&[("KESTO_MASTER_KEY", valor), ("KESTO_ALLOW_INSECURE", "1")]);

        let error =
            Config::from_lookup(&lookup).expect_err("una credencial débil no puede arrancar");

        assert!(
            !error.to_string().contains(valor),
            "el mensaje publica el valor de la credencial: {error}"
        );
    }

    /// 2.7 — En desarrollo, la ausencia de clave de firma no es debilidad.
    #[test]
    fn en_desarrollo_la_ausencia_de_clave_de_firma_no_es_debilidad() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
        ]);

        let config = Config::from_lookup(&lookup)
            .expect("sin KESTO_SECRET el modo de desarrollo arranca igual");

        assert!(matches!(&config.auth, AuthMode::Disabled));
    }
}
