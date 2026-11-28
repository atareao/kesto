//! Contrato del canal entre la API y el daemon de contenedores.
//!
//! La API no tiene el socket del daemon: lo único que conoce es este socket IPC y
//! las siete operaciones de [`SolicitudMotor`], que es una lista blanca cerrada.
//! Todo lo demás —el cliente HTTP del daemon, sus credenciales y su versión de
//! API— vive en el proceso `kesto-motor`.

use std::io;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Tope del cuerpo de un mensaje: un mebibyte.
///
/// La longitud viaja en cuatro bytes, así que un par puede anunciar hasta cuatro
/// gigabytes. Sin este cotejo, una longitud hostil reservaría memoria hasta
/// agotar el proceso: se descarta antes de asignar nada.
pub const TOPE_CUERPO: usize = 1_048_576;

/// Orden tal como cruza el IPC: sin tipos de la biblioteca del daemon.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrdenIpc {
    /// Argumentos de la orden, ya troceados.
    pub cmd: Vec<String>,
    /// Variables `CLAVE=valor` que se añaden al entorno del `exec`.
    pub env: Vec<String>,
}

impl std::fmt::Debug for OrdenIpc {
    /// Publica los argumentos y los **nombres** de las variables de entorno, nunca sus valores.
    ///
    /// El entorno de un `exec` lleva `MYSQL_PWD` y `PGPASSWORD`: un `Debug`
    /// derivado filtraría la credencial en cuanto un registro hiciera
    /// `tracing::debug!(?orden)` en el motor, que es justo donde más fácil es
    /// que un diagnóstico acabe copiado a un informe.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let claves: Vec<&str> = self
            .env
            .iter()
            .map(|variable| variable.split('=').next().unwrap_or(variable))
            .collect();

        f.debug_struct("OrdenIpc")
            .field("cmd", &self.cmd)
            .field("env", &claves)
            .finish()
    }
}

/// Canal de salida de un `exec`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CanalIpc {
    /// `stdout`: el volcado.
    Salida,
    /// `stderr`: el motivo de un fallo.
    Error,
}

/// Operaciones que la API puede pedir al motor.
///
/// Siete y solo siete: cualquier otra operación sobre el daemon queda fuera del
/// alcance del proceso de la API, aunque el motor sea capaz de hacerla.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SolicitudMotor {
    /// Latido: el motor contesta si puede hablar con el daemon.
    Comprobar,
    /// Crea un contenedor con las etiquetas dadas.
    CrearContenedor {
        /// Nombre del contenedor.
        nombre: String,
        /// Imagen de la que se crea.
        imagen: String,
        /// Etiquetas, en particular la de fuente de copia.
        etiquetas: Vec<(String, String)>,
    },
    /// Arranca un contenedor creado antes.
    IniciarContenedor {
        /// Nombre del contenedor.
        nombre: String,
    },
    /// Detiene un contenedor en marcha antes de un backup.
    DetenerContenedor {
        /// Nombre del contenedor.
        nombre: String,
    },
    /// Comprueba la etiqueta de fuente y crea un `exec`.
    CrearExec {
        /// Contenedor en el que se ejecuta la orden.
        contenedor: String,
        /// Orden y entorno.
        orden: OrdenIpc,
        /// Si el `exec` va a recibir el volcado por `stdin`.
        con_entrada: bool,
    },
    /// Devuelve el código de salida de un `exec` terminado.
    InspeccionarExec {
        /// Identificador del `exec`.
        id: String,
    },
    /// Arranca un `exec`. Los mensajes sucesivos entregan trozos de `stdin`;
    /// `entrada: None` cierra la entrada y termina el drenaje.
    IniciarExec {
        /// Identificador del `exec`.
        id: String,
        /// Trozo de entrada, o `None` cuando se acabó el volcado.
        entrada: Option<Vec<u8>>,
    },
}

/// Respuesta del motor a una solicitud, o evento del `exec` en curso.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RespuestaMotor {
    /// La operación sin resultado terminó bien.
    Aceptado,
    /// Un `exec` acaba de crearse.
    ExecCreado {
        /// Identificador con el que se arrancará.
        id: String,
    },
    /// Un trozo de la salida del `exec`, aún en marcha.
    Trozo {
        /// Canal del que salió.
        canal: CanalIpc,
        /// Bytes tal cual, sin recortar.
        bytes: Vec<u8>,
    },
    /// El `exec` terminó y su código de salida es este.
    Terminado {
        /// Código de salida; negativo si el daemon no supo decirlo.
        codigo: i32,
    },
    /// La operación falló.
    Fallo {
        /// Familia del fallo, para reconstruir el error tipado en la API.
        clase: ClaseFallo,
        /// Motivo legible.
        mensaje: String,
        /// Código HTTP del daemon, si lo hubo.
        status_code: Option<u16>,
    },
}

/// Familia de un fallo del motor.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClaseFallo {
    /// El socket del daemon no está.
    Socket,
    /// No se pudo hablar con el daemon.
    Motor,
    /// Fallo referido a un contenedor.
    Contenedor,
    /// El contenedor no lleva la etiqueta de fuente de copia.
    Etiqueta,
    /// El `exec` dejó de producir salida.
    Inactividad,
    /// El volcado no llegó entero al `stdin`.
    Entrada,
    /// No se pudo obtener un código de salida.
    Codigo,
    /// El par envió algo fuera del contrato.
    Protocolo,
}

/// Error del canal IPC, tanto en la API como en el motor.
#[derive(Debug, Error)]
pub enum Error {
    /// La ruta del socket no existe.
    #[error("el socket del motor no existe: {socket}")]
    SocketInexistente {
        /// Ruta pedida.
        socket: String,
    },
    /// No se pudo hablar con el motor.
    ///
    /// El `Display` redacta la ruta: este error puede acabar en `error_message`, y la ruta
    /// del socket IPC es información del despliegue, no del volcado. El campo se conserva
    /// para diagnósticos internos.
    #[error("no se pudo hablar con el motor: {causa}")]
    NoDisponible {
        /// Socket al que se intentó conectar.
        socket: String,
        /// Causa de entrada/salida.
        #[source]
        causa: io::Error,
    },
    /// La longitud anunciada no es válida o supera el tope.
    #[error("longitud IPC inválida: {longitud}")]
    LongitudInvalida {
        /// Valor anunciado por el par.
        longitud: u32,
    },
    /// El cuerpo no era un mensaje del contrato.
    #[error("mensaje IPC inválido: {causa}")]
    MensajeInvalido {
        /// Causa de la serialización.
        #[source]
        causa: serde_json::Error,
    },
    /// El motor contestó con un fallo.
    #[error("{clase:?}: {mensaje}")]
    DelMotor {
        /// Familia del fallo.
        clase: ClaseFallo,
        /// Motivo dado por el motor.
        mensaje: String,
        /// Código HTTP del daemon.
        status_code: Option<u16>,
    },
    /// El plazo de inactividad venció esperando un trozo.
    #[error("el motor dejó de enviar salida")]
    Inactividad,
    /// El par cerró el canal antes del final.
    #[error("el motor cerró el canal antes de terminar")]
    CanalCerrado,
}

/// Escribe un mensaje con longitud `u32` big-endian delante del cuerpo.
///
/// # Errors
///
/// Devuelve [`Error::MensajeInvalido`] si el tipo no se puede serializar y un
/// error de entrada/salida si el socket se cierra o falla la escritura.
pub async fn escribir<W>(escritor: &mut W, mensaje: &[u8]) -> Result<(), Error>
where
    W: AsyncWrite + Unpin,
{
    let longitud =
        u32::try_from(mensaje.len()).map_err(|_| Error::LongitudInvalida { longitud: u32::MAX })?;
    if mensaje.is_empty() || longitud as usize > TOPE_CUERPO {
        return Err(Error::LongitudInvalida { longitud });
    }

    escritor
        .write_all(&longitud.to_be_bytes())
        .await
        .map_err(|causa| Error::NoDisponible {
            socket: String::from("ipc"),
            causa,
        })?;
    escritor
        .write_all(mensaje)
        .await
        .map_err(|causa| Error::NoDisponible {
            socket: String::from("ipc"),
            causa,
        })?;
    escritor.flush().await.map_err(|causa| Error::NoDisponible {
        socket: String::from("ipc"),
        causa,
    })
}

/// Lee un mensaje: cuatro bytes de longitud big-endian y luego el cuerpo.
///
/// La longitud se coteja **antes** de reservar memoria. Un cero o un valor por
/// encima de un mebibyte se descarta con [`Error::LongitudInvalida`], sin
/// entrar en pánico ni asignar el tamaño que diga el par.
///
/// # Errors
///
/// Devuelve [`Error::LongitudInvalida`] para longitudes hostiles, y un error de
/// entrada/salida si el canal se cierra a medias.
pub async fn leer<R>(lector: &mut R) -> Result<Vec<u8>, Error>
where
    R: AsyncRead + Unpin,
{
    let mut cabecera = [0_u8; 4];
    lector
        .read_exact(&mut cabecera)
        .await
        .map_err(|causa| Error::NoDisponible {
            socket: String::from("ipc"),
            causa,
        })?;

    let longitud = u32::from_be_bytes(cabecera);
    let tamaño = longitud as usize;
    if tamaño == 0 || tamaño > TOPE_CUERPO {
        return Err(Error::LongitudInvalida { longitud });
    }

    let mut cuerpo = vec![0_u8; tamaño];
    lector
        .read_exact(&mut cuerpo)
        .await
        .map_err(|causa| Error::NoDisponible {
            socket: String::from("ipc"),
            causa,
        })?;
    Ok(cuerpo)
}

/// Envía una solicitud serializada por el canal.
///
/// # Errors
///
/// Los mismos que [`escribir`], más un fallo de serialización.
pub async fn enviar<W>(escritor: &mut W, solicitud: &SolicitudMotor) -> Result<(), Error>
where
    W: AsyncWrite + Unpin,
{
    let cuerpo = serde_json::to_vec(solicitud).map_err(|causa| Error::MensajeInvalido { causa })?;
    escribir(escritor, &cuerpo).await
}

/// Recibe y decodifica una respuesta del motor.
///
/// # Errors
///
/// Los mismos que [`leer`], más un cuerpo que no es del contrato.
pub async fn recibir<R>(lector: &mut R) -> Result<RespuestaMotor, Error>
where
    R: AsyncRead + Unpin,
{
    let cuerpo = leer(lector).await?;
    serde_json::from_slice(&cuerpo).map_err(|causa| Error::MensajeInvalido { causa })
}

/// Recibe y decodifica una solicitud de la API.
///
/// # Errors
///
/// Los mismos que [`recibir`], con un cuerpo que no es del contrato.
pub async fn recibir_solicitud<R>(lector: &mut R) -> Result<SolicitudMotor, Error>
where
    R: AsyncRead + Unpin,
{
    let cuerpo = leer(lector).await?;
    serde_json::from_slice(&cuerpo).map_err(|causa| Error::MensajeInvalido { causa })
}

/// Abre el socket IPC; acepta `unix://<ruta>` y la ruta pelada.
///
/// # Errors
///
/// Devuelve [`Error::SocketInexistente`] si la ruta no existe y
/// [`Error::NoDisponible`] si la conexión falla.
pub async fn conectar(ruta: &str) -> Result<tokio::net::UnixStream, Error> {
    let pelada = ruta.strip_prefix("unix://").unwrap_or(ruta);
    if !std::path::Path::new(pelada).exists() {
        return Err(Error::SocketInexistente {
            socket: ruta.to_owned(),
        });
    }
    tokio::net::UnixStream::connect(pelada)
        .await
        .map_err(|causa| Error::NoDisponible {
            socket: ruta.to_owned(),
            causa,
        })
}

#[cfg(test)]
mod pruebas {
    use super::Error;

    #[test]
    fn no_disponible_no_publica_la_ruta_en_el_display() {
        let error = Error::NoDisponible {
            socket: String::from("/run/secreto/motor.sock?token=CLAVE"),
            causa: std::io::Error::other("conexión rechazada"),
        };

        let motivo = error.to_string();
        assert!(
            !motivo.contains("/run/secreto") && !motivo.contains("CLAVE"),
            "el Display publicó la ruta o la credencial: {motivo}"
        );
        assert!(
            motivo.contains("conexión rechazada"),
            "se perdió la causa: {motivo}"
        );
    }
}
