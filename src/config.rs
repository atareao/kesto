//! Configuración efectiva del proceso, leída del entorno una sola vez al arrancar.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::secret::SecretString;

/// Nombre de la variable que fija el filtro de registro (§8 del SRS).
const ENV_RUST_LOG: &str = "RUST_LOG";
/// Nombre de la variable que fija la dirección de escucha (§8 del SRS).
const ENV_BIND_ADDRESS: &str = "BIND_ADDRESS";
/// Nombre de la variable que fija la cadena de base de datos (§8 del SRS).
const ENV_DATABASE_URL: &str = "DATABASE_URL";
/// Raíz bajo la que viven los directorios que un target `directory` puede copiar.
const ENV_KESTO_BACKUP_ROOT: &str = "KESTO_BACKUP_ROOT";
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
/// Permiso explícito para que el borde admita un `endpoint` de repositorio inseguro.
const ENV_KESTO_ALLOW_INSECURE_ENDPOINT: &str = "KESTO_ALLOW_INSECURE_ENDPOINT";
/// Emisor del proveedor OIDC.
const ENV_OIDC_ISSUER_URL: &str = "OIDC_ISSUER_URL";
/// Identificador público del cliente OIDC.
const ENV_OIDC_CLIENT_ID: &str = "OIDC_CLIENT_ID";
/// Secreto del cliente OIDC.
const ENV_OIDC_CLIENT_SECRET: &str = "OIDC_CLIENT_SECRET";
/// URL de retorno del proveedor OIDC.
const ENV_OIDC_REDIRECT_URL: &str = "OIDC_REDIRECT_URL";
/// Nombre de la variable que fija el socket IPC con el motor de copias.
///
/// Es la única ruta de contenedores que la API necesita conocer: el socket del
/// daemon pertenece al proceso `kesto-motor`, y la lectura de la variable
/// del daemon vive en [`crate::motor::config`], que es quien la usa.
const ENV_MOTOR_SOCKET: &str = "KESTO_MOTOR_SOCKET";

/// Segundos de la cabecera `Strict-Transport-Security` (`KESTO_HSTS_MAX_AGE`).
///
/// Es opcional: sin ella la cabecera queda apagada.
const ENV_KESTO_HSTS_MAX_AGE: &str = "KESTO_HSTS_MAX_AGE";
/// Socket IPC por defecto: la ruta del volumen compartido con el motor.
const DEFAULT_MOTOR_SOCKET: &str = "/run/kesto/motor.sock";

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
/// Raíz de las copias de directorio por defecto.
///
/// Es exactamente donde el `compose` del §8 monta los directorios a respaldar
/// (`/var/www:/backup_sources/var_www:ro`), así que el despliegue de referencia funciona sin
/// declarar nada. **No** se cablea en código —y de ahí que sea una variable— porque el e2e y el
/// modo local copian directorios temporales que no viven ahí, y porque el operador tiene que poder
/// moverla sin tocar el binario; y **no** se deriva de los montajes del propio contenedor porque en
/// el modo local no hay contenedor de Kesto que inspeccionar.
pub(crate) const DEFAULT_BACKUP_ROOT: &str = "/backup_sources";
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
    /// Raíz bajo la que el motor acepta el `host_path` de un target `directory`.
    ///
    /// Se guarda **tal como se declaró**, sin canonicalizar. La comparación del borde HTTP es
    /// **textual** por diseño —compara el `host_path` declarado contra esta raíz declarada, por
    /// componentes y sin tocar el disco—, y la canonicalización —que resuelve los enlaces
    /// simbólicos de los dos lados— es cosa del motor, que la hace por su cuenta cuando la
    /// necesita. Guardarla resuelta desajustaría el borde: una raíz declarada que es un enlace
    /// rechazaría con un `400` una ruta textual que el motor sí aceptaría.
    pub raiz_de_copias: PathBuf,
    /// Modo de autenticación efectivo.
    pub auth: AuthMode,
    /// Clave de firma de la sesión: obligatoria solo con la autenticación encendida.
    pub secret: Option<SecretString>,
    /// Clave maestra del cifrado en reposo: obligatoria siempre.
    pub master_key: SecretString,
    /// Filtro de registro con el que arranca el proceso.
    pub log_filter: String,
    /// Socket IPC con el motor de copias (`KESTO_MOTOR_SOCKET`).
    ///
    /// Lo lee la API; el daemon con privilegios lee la misma variable desde su
    /// propia configuración. El defecto apunta al volumen compartido.
    pub motor_socket: String,

    /// Antigüedad (segundos) de la cabecera `Strict-Transport-Security` (`KESTO_HSTS_MAX_AGE`).
    ///
    /// `None` —variable ausente— deja la cabecera apagada: en el despliegue estándar
    /// el TLS lo termina un proxy, y la HSTS es suya. Con un valor presente, toda
    /// respuesta lleva `Strict-Transport-Security: max-age=<n>`.
    pub hsts_max_age: Option<u64>,

    /// Si el borde admite un `endpoint` de repositorio inseguro (`KESTO_ALLOW_INSECURE_ENDPOINT`).
    ///
    /// El permiso rebaja **dos** cosas a la vez —el esquema `http` y el loopback, con su nombre
    /// `localhost`—, porque son el mismo caso: el destino está en la propia máquina y en claro. Lo
    /// que **no** rebaja nunca es la dirección de enlace local, la de metadatos, la no especificada
    /// ni las credenciales embebidas. El defecto es el permiso **negado**, que es lo que hace que un
    /// `compose` que perdió su variable no abra el agujero en silencio.
    pub allow_insecure_endpoint: bool,
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

    /// La URL de retorno de OIDC no va por `https` en un modo que sí lo exige.
    ///
    /// La cookie de sesión viaja con `Secure` incondicional, así que un retorno
    /// por `http` no podría establecer la sesión nunca: es un error de arranque,
    /// no un modo degradado.
    #[error("la variable de entorno {name} debe ser una URL `https` en modo OIDC")]
    InsecureRedirect {
        /// Nombre de la variable mal formada.
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

    /// La raíz de las copias de directorio está presente pero no sirve.
    ///
    /// Nombra la variable y nunca su valor: el valor es una ruta del despliegue, y este es un
    /// error de arranque que un operador va a leer en un registro.
    #[error("la variable de entorno {name} debe ser una ruta absoluta")]
    InvalidBackupRoot {
        /// Nombre de la variable mal formada.
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

/// ¿El esquema de esta URL es `https`?
///
/// La variable es una URL de configuración y lo único que decide aquí es el
/// esquema, que es lo que precede a `://`; no hace falta un analizador entero.
fn es_https(url: &str) -> bool {
    url.split_once("://")
        .is_some_and(|(esquema, _)| esquema.eq_ignore_ascii_case("https"))
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

        // La raíz de las copias de directorio. Se lee **aquí**, entre dos variables que ya se
        // leían, sin mover ninguna: el orden de la lectura es lo que fija qué error se ve primero
        // cuando hay varios problemas, y reordenarlo cambiaría mensajes ya probados. La variable
        // es opcional y su ausencia cae al defecto; una raíz presente que no sea absoluta —vacía
        // incluida— es un error de arranque, como `RUST_LOG=""` y `BIND_ADDRESS=""`: un valor
        // presente pero inutilizable no se degrada en silencio.
        //
        // El valor no se recorta: una raíz que empiece por un espacio no es absoluta y falla, que
        // es justo lo que hay que hacer con una variable pegada con un espacio de más.
        //
        // La raíz se guarda **tal como se declaró**, sin canonicalizar. El borde HTTP compara el
        // `host_path` declarado contra esta raíz declarada, y la comparación es textual a
        // propósito; el motor canonicaliza **los dos lados** por su cuenta cuando la necesita. Si
        // aquí se guardara resuelta, una raíz declarada que es un enlace dejaría de casar con las
        // rutas textuales que el borde acepta, y el alta rechazaría con un `400` desconcertante una
        // ruta que el motor sí admitiría.
        let raiz_de_copias = match lookup.get(ENV_KESTO_BACKUP_ROOT) {
            None => PathBuf::from(DEFAULT_BACKUP_ROOT),
            Some(raw) => {
                let declarada = Path::new(&raw);

                if !declarada.is_absolute() {
                    return Err(ConfigError::InvalidBackupRoot {
                        name: ENV_KESTO_BACKUP_ROOT,
                    });
                }

                declarada.to_path_buf()
            }
        };

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

        // El socket IPC del motor. `DOCKER_HOST` **no** se lee aquí: pertenece
        // al proceso privilegiado (`src/motor/config.rs`). La API solo conoce
        // este canal, y su defecto apunta al volumen compartido del compose.
        let motor_socket = lookup
            .get(ENV_MOTOR_SOCKET)
            .map(|valor| valor.trim().to_owned())
            .filter(|valor| !valor.is_empty())
            .unwrap_or_else(|| DEFAULT_MOTOR_SOCKET.to_owned());

        // El permiso del `endpoint` se lee **siempre**, también con la autenticación encendida: el
        // `endpoint` de un repositorio se valida en los dos modos, así que una variable que solo se
        // leyera en el modo de desarrollo dejaría el permiso concedido sin declararlo. Va al final
        // a propósito: el orden de lectura fija qué error se ve primero cuando hay varios
        // problemas, y meterlo antes cambiaría mensajes ya probados.
        let allow_insecure_endpoint = match lookup.get(ENV_KESTO_ALLOW_INSECURE_ENDPOINT) {
            None => false,
            Some(raw) => match raw.trim() {
                "1" => true,
                "0" => false,
                _ => {
                    return Err(ConfigError::InvalidValue {
                        name: ENV_KESTO_ALLOW_INSECURE_ENDPOINT,
                    });
                }
            },
        };

        Ok(Self {
            bind_address,
            database_url,
            raiz_de_copias,
            auth,
            secret,
            master_key: SecretString::new(master_key),
            log_filter,
            motor_socket,
            hsts_max_age,
            allow_insecure_endpoint,
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
                // El retorno no tiene defecto: la cookie de sesión viaja con
                // `Secure` incondicional, así que un retorno por `http` no podría
                // establecer la sesión nunca. Ausente o no `https` es error de
                // arranque, y el mensaje nombra la variable.
                let redirect_url = redirect_url.ok_or(ConfigError::MissingVariable {
                    name: ENV_OIDC_REDIRECT_URL,
                })?;

                if !es_https(&redirect_url) {
                    return Err(ConfigError::InsecureRedirect {
                        name: ENV_OIDC_REDIRECT_URL,
                    });
                }

                Ok(AuthMode::Oidc(OidcSettings {
                    issuer,
                    client_id,
                    client_secret: SecretString::new(client_secret),
                    redirect_url,
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
            "motor_socket": self.motor_socket,
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
    use std::path::PathBuf;

    use super::{AuthMode, Config, ConfigError, DEFAULT_BACKUP_ROOT, DEFAULT_BIND_ADDRESS};
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
    /// URL de retorno por `https`: la única admisible en modo `Oidc`.
    const OIDC_REDIRECT_HTTPS: &str = "https://panel.example.test/api/v1/auth/callback";
    /// URL de retorno por `http`: el caso que deja de ser admisible.
    const OIDC_REDIRECT_HTTP: &str = "http://localhost:8080/api/v1/auth/callback";
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
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTPS),
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
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTPS),
        ]);

        let config = Config::from_lookup(&lookup).expect("las tres variables OIDC son válidas");

        match &config.auth {
            AuthMode::Oidc(settings) => {
                assert_eq!(settings.issuer, OIDC_ISSUER);
                assert_eq!(settings.client_id, OIDC_CLIENT_ID);
                assert_eq!(
                    settings.redirect_url, OIDC_REDIRECT_HTTPS,
                    "el retorno tiene que ser el declarado, no un valor por defecto"
                );
            }
            AuthMode::Disabled => panic!("las tres variables OIDC deben encender la autenticación"),
        }
    }

    #[test]
    fn con_oidc_la_url_de_retorno_es_obligatoria() {
        // El retorno no tiene valor por defecto: en modo OIDC un retorno que no
        // sea `https` no tiene uso legítimo, así que un defecto no podría ser
        // válido nunca. Se exige declarado y se nombra la variable que falta.
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ]);

        let error = Config::from_lookup(&lookup)
            .expect_err("sin OIDC_REDIRECT_URL no se puede arrancar en modo Oidc");

        assert!(
            error.to_string().contains("OIDC_REDIRECT_URL"),
            "el mensaje tiene que nombrar la variable: {error}"
        );
    }

    #[test]
    fn con_oidc_un_retorno_por_http_no_arranca() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTP),
        ]);

        let error = Config::from_lookup(&lookup)
            .expect_err("un retorno por http no puede arrancar en modo Oidc");

        assert!(
            error.to_string().contains("OIDC_REDIRECT_URL"),
            "el mensaje tiene que nombrar la variable: {error}"
        );
    }

    #[test]
    fn sin_oidc_la_url_de_retorno_se_ignora() {
        // Sin las tres `OIDC_*` no hay modo OIDC al que aplicarle la regla, así
        // que la variable se ignora en silencio, aunque venga por `http`.
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTP),
        ]);

        let config = Config::from_lookup(&lookup).expect("sin OIDC la configuración es válida");

        assert!(
            matches!(config.auth, AuthMode::Disabled),
            "sin las tres OIDC_* el modo tiene que ser Disabled"
        );
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
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTPS),
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

    /// SEC-06 — la API no conoce el socket del daemon.
    ///
    /// `DOCKER_HOST` lo lee únicamente el proceso privilegiado. Aquí se comprueba
    /// en el volcado —lo que el arranque registra— que la configuración de la
    /// API no lo conserva ni lo publica, aunque el entorno lo traiga informado.
    #[test]
    fn la_api_ignora_docker_host_y_solo_expone_la_ipc_del_motor() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("DOCKER_HOST", "unix:///run/user/1000/podman/podman.sock"),
            ("KESTO_MOTOR_SOCKET", "/run/kesto/otro.sock"),
        ]);

        let config = Config::from_lookup(&lookup).expect("la configuración es válida");
        let volcado = config.describe();

        assert!(
            volcado.get("docker_host").is_none(),
            "la API publicó el socket del daemon en su volcado de configuración"
        );
        assert_eq!(
            volcado["motor_socket"], "/run/kesto/otro.sock",
            "la API solo debe exponer el socket IPC del motor"
        );
    }

    /// SEC-06 — el defecto del canal IPC no depende del daemon.
    ///
    /// Dos configuraciones con un `DOCKER_HOST` distinto —o sin él— ven el
    /// mismo socket IPC por defecto: ese es el único extremo que la API conoce.
    #[test]
    fn el_defecto_de_la_ipc_no_depende_de_docker_host() {
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

        for volcado in [una.describe(), otra.describe()] {
            assert_eq!(
                volcado["motor_socket"], "/run/kesto/motor.sock",
                "el defecto del canal IPC es el volumen compartido del compose"
            );
            assert!(volcado.get("docker_host").is_none());
        }
    }

    #[test]
    fn el_volcado_no_publica_credenciales() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTPS),
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
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTPS),
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

    /// 5.6 — Un permiso del `endpoint` que no es `1` ni `0` es error de formato.
    ///
    /// El permiso del `endpoint` es un booleano de entorno como el permiso del modo sin
    /// autenticación, y se lee **siempre** —también con la autenticación encendida, porque el
    /// `endpoint` de un repositorio se valida en los dos modos—: la tabla de sinónimos no existe, y
    /// un valor que no sea `1` ni `0` tiene que impedir el arranque nombrando la variable y sin
    /// publicar el valor.
    #[test]
    fn un_permiso_del_endpoint_inservible_es_error_de_formato() {
        for valor in ["", "true", "2", "si"] {
            let lookup = lookup_from(&[
                ("KESTO_MASTER_KEY", MASTER_KEY),
                ("KESTO_ALLOW_INSECURE", "1"),
                ("BIND_ADDRESS", "127.0.0.1:8080"),
                ("KESTO_ALLOW_INSECURE_ENDPOINT", valor),
            ]);

            let error = Config::from_lookup(&lookup)
                .err()
                .unwrap_or_else(|| panic!("`{valor}` no es un valor de permiso válido"));
            let mensaje = error.to_string();

            assert!(
                mensaje.contains("KESTO_ALLOW_INSECURE_ENDPOINT"),
                "el mensaje no nombra la variable: {mensaje}"
            );
            assert!(
                !mensaje.contains(valor) || valor.is_empty(),
                "el mensaje publica el valor del permiso: {mensaje}"
            );
        }
    }

    /// 5.6 (contrapeso) — `1` concede el permiso del `endpoint`.
    #[test]
    fn el_permiso_del_endpoint_se_concede_con_uno() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("BIND_ADDRESS", "127.0.0.1:8080"),
            ("KESTO_ALLOW_INSECURE_ENDPOINT", "1"),
        ]);

        let config = Config::from_lookup(&lookup).expect("`1` concede el permiso");

        assert!(
            config.allow_insecure_endpoint,
            "`1` tiene que conceder el permiso del endpoint"
        );
    }

    /// 5.6 (contrapeso) — La ausencia de la variable niega el permiso.
    ///
    /// Es el defecto, y el que importa: un despliegue que nunca declaró la variable no puede
    /// encontrarse el agujero abierto. `0` significa lo mismo, y se comprueba aquí para que la
    /// equivalencia quede escrita.
    #[test]
    fn sin_la_variable_el_permiso_del_endpoint_esta_negado() {
        for valor in [None, Some("0")] {
            let mut pares = vec![
                ("KESTO_MASTER_KEY", MASTER_KEY),
                ("KESTO_ALLOW_INSECURE", "1"),
                ("BIND_ADDRESS", "127.0.0.1:8080"),
            ];

            if let Some(valor) = valor {
                pares.push(("KESTO_ALLOW_INSECURE_ENDPOINT", valor));
            }

            let config = Config::from_lookup(&lookup_from(&pares))
                .unwrap_or_else(|_| panic!("`{valor:?}` no impide el arranque"));

            assert!(
                !config.allow_insecure_endpoint,
                "`{valor:?}` tiene que negar el permiso del endpoint"
            );
        }
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
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTPS),
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
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTPS),
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
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTPS),
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
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTPS),
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
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_HTTPS),
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

    /// 1.1 — Sin `KESTO_BACKUP_ROOT`, la raíz de copias es la del `compose` del §8.
    #[test]
    fn sin_la_raiz_de_copias_gana_el_defecto() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
        ]);

        let config =
            Config::from_lookup(&lookup).expect("sin la raíz de copias la configuración es válida");

        assert_eq!(
            config.raiz_de_copias,
            PathBuf::from("/backup_sources"),
            "sin la variable, la raíz tiene que ser la que monta el §8"
        );
    }

    /// 1.2 — Una raíz de copias relativa no se acepta.
    #[test]
    fn una_raiz_de_copias_relativa_es_error_de_arranque() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("KESTO_BACKUP_ROOT", "backup_sources"),
        ]);

        let error =
            Config::from_lookup(&lookup).expect_err("una raíz de copias relativa no arranca");
        let mensaje = error.to_string();

        assert!(
            matches!(&error, ConfigError::InvalidBackupRoot { name } if *name == "KESTO_BACKUP_ROOT"),
            "el error no señala KESTO_BACKUP_ROOT: {mensaje}"
        );
        assert!(
            mensaje.contains("KESTO_BACKUP_ROOT"),
            "el mensaje no nombra KESTO_BACKUP_ROOT: {mensaje}"
        );
        // El mensaje nombra la variable, nunca el valor de ninguna otra.
        assert!(
            !mensaje.contains(MASTER_KEY),
            "el mensaje publica el valor de KESTO_MASTER_KEY: {mensaje}"
        );
    }

    /// 1.3 — Una raíz de copias vacía no cae al defecto.
    #[test]
    fn una_raiz_de_copias_vacia_no_cae_al_defecto() {
        // El artefacto de un `compose` que interpola una variable que no definiste es la cadena
        // vacía, y una raíz vacía es relativa: el proceso no puede arrancar con una raíz que
        // nadie pidió, ni puede caer en silencio a `/backup_sources`.
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("KESTO_BACKUP_ROOT", ""),
        ]);

        let error = Config::from_lookup(&lookup).expect_err("una raíz de copias vacía no arranca");
        let mensaje = error.to_string();

        assert!(
            matches!(&error, ConfigError::InvalidBackupRoot { name } if *name == "KESTO_BACKUP_ROOT"),
            "el error no señala KESTO_BACKUP_ROOT: {mensaje}"
        );
        assert!(
            !mensaje.contains(DEFAULT_BACKUP_ROOT),
            "el mensaje deja entrever el defecto: {mensaje}"
        );
    }

    /// 1.4 — Una raíz de copias absoluta se lee.
    #[test]
    fn una_raiz_de_copias_absoluta_se_lee() {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("KESTO_BACKUP_ROOT", "/tmp/kesto-copias-de-prueba"),
        ]);

        let config = Config::from_lookup(&lookup)
            .expect("una raíz de copias absoluta es una configuración válida");

        assert_eq!(
            config.raiz_de_copias,
            PathBuf::from("/tmp/kesto-copias-de-prueba"),
            "una raíz que no existe se guarda tal cual: el motor la canonicaliza cuando la use"
        );
    }

    /// 1.4b — Una raíz de copias que existe se guarda **tal cual**, sin canonicalizar.
    ///
    /// La raíz se declara a través de un enlace para que la afirmación signifique algo: si se
    /// guardara resuelta, el valor diferiría del declarado y el borde HTTP —que compara por texto—
    /// rechazaría rutas que el motor aceptaría. La canonicalización es cosa del motor.
    #[test]
    fn una_raiz_de_copias_existente_se_guarda_tal_cual() {
        let directorio = crate::test_support::directorio_de_prueba();
        let real = directorio.ruta().join("real");
        std::fs::create_dir(&real).expect("el directorio real de la prueba se crea");
        let enlace = directorio.ruta().join("enlace");
        std::os::unix::fs::symlink(&real, &enlace).expect("el enlace de la prueba se crea");

        let declarada = enlace.to_str().expect("las rutas del arnés son UTF-8");

        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
            ("KESTO_BACKUP_ROOT", declarada),
        ]);

        let config = Config::from_lookup(&lookup).expect("una raíz existente es válida");

        assert_eq!(
            config.raiz_de_copias,
            PathBuf::from(declarada),
            "la raíz se guarda tal como se declaró: la comparación del borde es textual y la \
             canonicalización es cosa del motor"
        );
        assert_ne!(
            config.raiz_de_copias,
            std::fs::canonicalize(&enlace).expect("el enlace de la prueba existe"),
            "guardarla resuelta desajustaría la comparación textual del borde"
        );
    }
}
