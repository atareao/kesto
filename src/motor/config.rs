//! Configuración del proceso privilegiado: el único que conoce el socket del
//! daemon de contenedores y el UID del proceso de la API que puede hablarle.

use std::path::PathBuf;

use thiserror::Error;

/// Variable que fija el socket del daemon.
pub const ENV_DOCKER_HOST: &str = "DOCKER_HOST";
/// Variable que fija el socket IPC compartido con la API.
pub const ENV_SOCKET_MOTOR: &str = "KESTO_MOTOR_SOCKET";
/// Variable que identifica al par IPC.
pub const ENV_UID_API: &str = "KESTO_API_UID";
/// Variable que fija la raíz de copias que el motor vuelve a confinar.
pub const ENV_BACKUP_ROOT: &str = "KESTO_BACKUP_ROOT";

/// Socket del daemon por defecto, el del §8 del SRS.
pub const DOCKER_POR_DEFECTO: &str = "unix:///var/run/docker.sock";
/// Socket IPC por defecto, dentro del volumen compartido.
pub const SOCKET_POR_DEFECTO: &str = "/run/kesto/motor.sock";
/// Raíz de copias por defecto, la misma que la de la API (§8).
pub const RAIZ_POR_DEFECTO: &str = "/backup_sources";

/// Fallos de la configuración del motor.
#[derive(Debug, Error)]
pub enum Error {
    /// Falta el UID de la API: sin él no se puede verificar al par.
    #[error(
        "falta {ENV_UID_API}: el motor tiene que conocer el UID de la API para verificar al par IPC"
    )]
    FaltaUidApi,
    /// El UID de la API no es un número.
    #[error("{ENV_UID_API} no es un UID válido: {valor}")]
    UidInvalido {
        /// Valor leído.
        valor: String,
    },
    /// La raíz de copias declarada no es una ruta absoluta no vacía.
    #[error("{ENV_BACKUP_ROOT} tiene que ser una ruta absoluta y no vacía: {valor}")]
    RaizDeCopiasInvalida {
        /// Valor leído.
        valor: String,
    },
}

/// Configuración efectiva del motor.
#[derive(Debug, Clone)]
pub struct AjustesMotor {
    /// Socket del daemon de contenedores.
    pub docker_host: String,
    /// Ruta del socket IPC.
    pub socket_ipc: PathBuf,
    /// UID del proceso de la API, único admitido en el otro extremo.
    pub uid_api: u32,
    /// Raíz de copias: el motor confina contra ella de nuevo antes del `exec`.
    pub raiz_de_copias: PathBuf,
}

impl AjustesMotor {
    /// Lee la configuración de un `lookup` inyectado.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::FaltaUidApi`], [`Error::UidInvalido`] o
    /// [`Error::RaizDeCopiasInvalida`] si no se puede identificar al par IPC o
    /// la raíz de copias declarada no es absoluta. Ni el socket del daemon ni
    /// el del IPC son obligatorios: ambos tienen defecto.
    pub fn desde_lookup<F>(lookup: F) -> Result<Self, Error>
    where
        F: Fn(&str) -> Option<String>,
    {
        let docker_host = lookup(ENV_DOCKER_HOST)
            .map(|valor| valor.trim().to_owned())
            .filter(|valor| !valor.is_empty())
            .unwrap_or_else(|| DOCKER_POR_DEFECTO.to_owned());

        let socket_ipc = lookup(ENV_SOCKET_MOTOR)
            .map(|valor| valor.trim().to_owned())
            .filter(|valor| !valor.is_empty())
            .unwrap_or_else(|| SOCKET_POR_DEFECTO.to_owned());

        let valor_uid = lookup(ENV_UID_API).ok_or(Error::FaltaUidApi)?;
        let uid_api = valor_uid
            .trim()
            .parse::<u32>()
            .map_err(|_| Error::UidInvalido { valor: valor_uid })?;

        // Una raíz relativa no puede compararse por prefijo con las rutas
        // absolutas de los `exec`: `confinamiento_valido` saltaría la
        // canonicalización y el motor dejaría de revalidar en silencio. Por
        // eso una declaración relativa o vacía es error de arranque; solo
        // ausente toma el defecto.
        let raiz_de_copias = match lookup(ENV_BACKUP_ROOT) {
            None => RAIZ_POR_DEFECTO.to_owned(),
            Some(valor) => {
                let valor = valor.trim().to_owned();
                if !valor.starts_with('/') {
                    return Err(Error::RaizDeCopiasInvalida { valor });
                }
                valor
            }
        };

        Ok(Self {
            docker_host,
            socket_ipc: PathBuf::from(socket_ipc),
            uid_api,
            raiz_de_copias: PathBuf::from(raiz_de_copias),
        })
    }

    /// Lee la configuración del entorno del proceso.
    ///
    /// # Errors
    ///
    /// Los mismos que [`Self::desde_lookup`].
    pub fn desde_env() -> Result<Self, Error> {
        Self::desde_lookup(|nombre| std::env::var(nombre).ok())
    }
}

#[cfg(test)]
mod pruebas {
    use std::path::PathBuf;

    use super::{AjustesMotor, ENV_BACKUP_ROOT, ENV_UID_API, Error, RAIZ_POR_DEFECTO};

    fn lookup<'a>(pares: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |nombre| {
            pares
                .iter()
                .find(|(clave, _)| *clave == nombre)
                .map(|(_, valor)| (*valor).to_owned())
        }
    }

    #[test]
    fn la_raiz_de_copias_exige_una_ruta_absoluta_y_toma_el_defecto_si_falta() {
        for (valor, valido) in [
            ("backup_sources", false),
            ("", false),
            ("   ", false),
            ("/copias", true),
        ] {
            let pares = [(ENV_UID_API, "10001"), (ENV_BACKUP_ROOT, valor)];
            let resultado = AjustesMotor::desde_lookup(lookup(&pares));
            if valido {
                assert_eq!(
                    resultado
                        .expect("una ruta absoluta es aceptada")
                        .raiz_de_copias,
                    PathBuf::from(valor)
                );
            } else {
                let error = resultado.expect_err("una raíz relativa o vacía no cae al defecto");
                assert!(
                    matches!(&error, Error::RaizDeCopiasInvalida { valor: leido } if leido == valor.trim()),
                    "el error no señala el valor declarado: {error:?}"
                );
                assert!(
                    error.to_string().contains(ENV_BACKUP_ROOT),
                    "el error no nombra la variable: {error}"
                );
            }
        }

        let sin_raiz = [(ENV_UID_API, "10001")];
        let ajustes =
            AjustesMotor::desde_lookup(lookup(&sin_raiz)).expect("ausente usa el defecto");
        assert_eq!(ajustes.raiz_de_copias, PathBuf::from(RAIZ_POR_DEFECTO));
    }
}
