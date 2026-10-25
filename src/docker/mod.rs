//! El transporte de la API de Docker: ejecutar una orden dentro de un contenedor.
//!
//! Es la pieza que hace posible el §4.A sin que un solo byte del volcado pase por el
//! disco: dentro del contenedor de la base se crea un `exec`, su `stdout` se lee **según
//! llega** y se entrega en trozos a quien lo haya pedido, y al final se pregunta por el
//! código de salida del proceso. Este módulo no sabe nada de volcados, de bases ni de
//! repositorios: recibe una orden, entrega bytes y devuelve un código.
//!
//! [`Cliente`] es toda la superficie: se construye una vez con el socket del motor y se
//! comparte. Los tipos de la biblioteca —`bollard`, y por debajo `bytes`— **no** salen
//! por aquí, y eso es deliberado: lo que cruza esta frontera es contrato del crate, y
//! atar ese contrato a la representación interna de una dependencia convierte una subida
//! de versión en un cambio de contrato. Por eso una orden se describe con `Vec<String>` y
//! un trozo llega como `Vec<u8>`.
//!
//! El detalle que este módulo existe para no olvidar: **el runtime que conduce el `exec`
//! tiene que seguir vivo mientras se drena su salida**. Está medido y contado en
//! [`Cliente::ejecutar`], y hay una prueba marcada `#[ignore]` que lo fija.

mod cliente;

use thiserror::Error;

pub use cliente::Cliente;

/// Una orden a ejecutar dentro de un contenedor.
///
/// `cmd` es la línea de comandos **como lista de argumentos**, no como cadena: la API de
/// Docker no pasa por un intérprete, así que quien la construya decide si quiere un
/// `sh -c` o no.
///
/// `env` es **aditivo**: las variables que van aquí se suman al entorno que el contenedor
/// ya tiene, no lo sustituyen —medido: `PATH` conserva su valor—. Es lo que permite
/// entregar la contraseña de la base por el entorno del `exec` (`MYSQL_PWD`,
/// `PGPASSWORD`) en vez de por su línea de comandos, que es lo que fija el §4.A y lo que
/// deja la credencial a la vista en el listado de procesos del contenedor.
#[derive(Clone, PartialEq, Eq)]
pub struct Orden {
    /// La orden, ya troceada en argumentos.
    pub cmd: Vec<String>,
    /// Variables de entorno que se **añaden** a las del contenedor, en la forma
    /// `CLAVE=valor`.
    pub env: Vec<String>,
}

impl std::fmt::Debug for Orden {
    /// Publica el comando y los **nombres** de las variables de entorno, nunca sus valores.
    ///
    /// No es una preferencia de estilo, es la diferencia entre un registro útil y una
    /// credencial filtrada: el entorno de una orden es donde va la contraseña de la base
    /// —`MYSQL_PWD`, `PGPASSWORD`—, así que un `#[derive(Debug)]` convertiría un
    /// `tracing::debug!(?orden)` de una línea en esa contraseña escrita en el registro del
    /// contenedor, que es justo de donde se copia a un informe de fallo.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let claves: Vec<&str> = self
            .env
            .iter()
            .map(|variable| variable.split('=').next().unwrap_or(variable))
            .collect();

        f.debug_struct("Orden")
            .field("cmd", &self.cmd)
            .field("env", &claves)
            .finish()
    }
}

/// De qué canal del proceso viene un trozo de su salida.
///
/// La distinción es la que permite que un fallo de extracción no se confunda con una base
/// vacía: el volcado sale por [`Canal::Salida`] y el motivo de un fallo, por
/// [`Canal::Error`]. Un `stdout` vacío con el canal de error lleno es una extracción que
/// no se hizo, no una base sin datos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Canal {
    /// `stdout` del proceso: la salida del comando, que es el volcado.
    Salida,
    /// `stderr` del proceso: el motivo de un fallo, que **no** es volcado.
    Error,
}

/// Un trozo de la salida de un `exec`, ya decodificado.
///
/// Los bytes son un `Vec<u8>` propio y **no** el `Bytes` de la biblioteca: este tipo es
/// contrato del crate, y con él entraría en la superficie pública la representación
/// interna de una dependencia. La copia por trozo es despreciable frente al cifrado en
/// reposo del repositorio y frente a la red, y a cambio el volcado deja de compartir
/// búfer con la biblioteca que lo leyó.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trozo {
    /// De qué canal salió.
    pub canal: Canal,
    /// Los bytes tal y como llegaron: sin recortar, sin normalizar y con sus saltos de
    /// línea dentro.
    pub bytes: Vec<u8>,
}

/// Error del transporte de la API de Docker.
///
/// Cada variante conserva lo que hace falta para el diagnóstico y **el motivo se escribe
/// aquí**: el `Display` de la biblioteca describe sus fallos con su formato, que es suyo y
/// puede cambiar en cualquier versión menor, así que no viaja tal cual. Los dos campos que
/// sí son de la API —`status_code` y `message`— conservan su nombre original.
#[derive(Debug, Error)]
pub enum Error {
    /// La ruta del socket no existe en el sistema de ficheros.
    ///
    /// El motivo nombra **la ruta que se pidió**, no la que la biblioteca haya
    /// normalizado: lo que el operador tiene que poder reconocer es lo que escribió en la
    /// configuración del despliegue.
    #[error("el socket del motor de contenedores no existe: {socket}")]
    SocketInexistente {
        /// Socket tal y como se pidió.
        socket: String,
    },

    /// No se pudo hablar con el motor por el socket indicado.
    #[error("no se pudo hablar con el motor de contenedores por {socket}: {causa}")]
    MotorNoDisponible {
        /// Socket con el que se intentó.
        socket: String,
        /// Causa, tal y como la informa la biblioteca.
        #[source]
        causa: bollard::errors::Error,
    },

    /// El motor contestó con un error de su propia API al operar con un contenedor.
    ///
    /// El motivo nombra el contenedor: hoy el mensaje del motor ya lo lleva dentro
    /// —medido: `no container with name or ID "mi-contenedor" found: no such container`—,
    /// pero eso es texto suyo, así que el nombre se pone aquí y no se delega en él.
    #[error(
        "el motor de contenedores contestó {status_code} sobre el contenedor \
         `{contenedor}`: {message}"
    )]
    MotorContesta {
        /// Contenedor al que se refería la operación.
        contenedor: String,
        /// Código con el que contestó el motor.
        status_code: u16,
        /// Mensaje del motor, tal cual.
        message: String,
    },

    /// La comunicación con el motor falló al operar con un contenedor.
    #[error(
        "no se pudo hablar con el motor de contenedores sobre el contenedor \
         `{contenedor}`: {causa}"
    )]
    MotorNoContesta {
        /// Contenedor al que se refería la operación.
        contenedor: String,
        /// Causa, tal y como la informa la biblioteca.
        #[source]
        causa: bollard::errors::Error,
    },

    /// El `exec` no dejó ningún flujo del que leer.
    ///
    /// No puede pasar —la ejecución pide `detach: false`— pero existe para que el caso
    /// imposible no se convierta en un cero: sin flujo no hay salida, y dar por buena una
    /// orden que no se pudo leer es exactamente lo que este módulo no hace.
    #[error("el motor de contenedores no dejó leer la salida de la orden en `{contenedor}`")]
    SinSalida {
        /// Contenedor en el que se ejecutaba la orden.
        contenedor: String,
    },

    /// El motor no informó un código de salida interpretable.
    ///
    /// `None` significa «el motor no lo informó», y darlo por `0` convertiría un fallo en
    /// un éxito. Un valor que no cabe en un `i32` tampoco es un código de salida: los dos
    /// desenlaces son el mismo —no se puede decir cómo acabó— y por eso comparten variante.
    #[error(
        "el motor de contenedores no informó un código de salida interpretable para la \
         orden en `{contenedor}`"
    )]
    SinCodigoDeSalida {
        /// Contenedor en el que se ejecutaba la orden.
        contenedor: String,
    },
}

impl Error {
    /// Traduce un fallo del motor que se refiere a un contenedor concreto.
    ///
    /// Un fallo de respuesta del servidor se convierte en [`Error::MotorContesta`]
    /// conservando su `status_code` y su `message`; cualquier otro se conserva como causa
    /// de [`Error::MotorNoContesta`]. Se distinguen porque no se arreglan igual: lo
    /// primero es el motor diciendo que no a algo concreto, lo segundo es no haber podido
    /// preguntarle.
    fn del_contenedor(contenedor: &str, causa: bollard::errors::Error) -> Self {
        match causa {
            bollard::errors::Error::DockerResponseServerError {
                status_code,
                message,
            } => Self::MotorContesta {
                contenedor: contenedor.to_owned(),
                status_code,
                message,
            },
            causa => Self::MotorNoContesta {
                contenedor: contenedor.to_owned(),
                causa,
            },
        }
    }

    /// Traduce un fallo del motor que **no** se refiere a ningún contenedor: la conexión
    /// inicial y la comprobación de que el motor contesta.
    fn del_socket(socket: &str, causa: bollard::errors::Error) -> Self {
        match causa {
            // La biblioteca comprueba que la ruta existe **antes** de abrir nada, así que
            // este fallo es «no hay socket ahí» y no un problema de transporte: se
            // distingue porque el diagnóstico es otro —mirar la ruta, no el motor—.
            bollard::errors::Error::SocketNotFoundError(_) => Self::SocketInexistente {
                socket: socket.to_owned(),
            },
            causa => Self::MotorNoDisponible {
                socket: socket.to_owned(),
                causa,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use bollard::errors::Error as ErrorDelMotor;

    use super::{Canal, Error, Orden};

    /// El mensaje que el motor da cuando el contenedor no existe, **medido** contra el
    /// Podman de esta máquina: el nombre del contenedor viaja dentro, entrecomillado.
    const MENSAJE_DEL_404: &str =
        "no container with name or ID \"mi-contenedor\" found: no such container";

    /// El mensaje que el motor da cuando el `exec` se pide sin ningún `attach`, también
    /// medido: es un `500` del motor, no un fallo del comando.
    const MENSAJE_DEL_500: &str = "must provide at least one stream to attach to: invalid argument";

    /// Un 404 del motor se traduce a nuestro error y **el motivo nombra el contenedor**.
    ///
    /// Es el requisito que impide que un contenedor mal escrito se lea como un fallo de
    /// volcado: el motivo tiene que llevar el nombre, y no se delega en el texto del motor
    /// —que hoy lo lleva— porque ese texto es suyo.
    #[test]
    fn un_404_del_motor_nombra_el_contenedor() {
        let medida = ErrorDelMotor::DockerResponseServerError {
            status_code: 404,
            message: MENSAJE_DEL_404.to_owned(),
        };
        let display_de_la_biblioteca = format!("{medida}");

        let error = Error::del_contenedor("mi-contenedor", medida);
        let motivo = error.to_string();
        let debug = format!("{error:?}");

        assert!(
            motivo.contains("mi-contenedor"),
            "el motivo no nombra el contenedor: {motivo}"
        );
        assert!(
            motivo.contains("404"),
            "el motivo no lleva el código: {motivo}"
        );
        assert!(
            motivo.contains(MENSAJE_DEL_404),
            "el motivo no conserva el mensaje del motor: {motivo}"
        );
        // El `Display` de la biblioteca no sale por aquí: el formato del motivo es nuestro.
        assert_ne!(
            motivo, display_de_la_biblioteca,
            "el motivo es el `Display` de bollard tal cual"
        );
        // Y el `Debug` tampoco lo publica: es el nuestro, con los campos que se conservan.
        assert!(
            debug.contains("mi-contenedor") && debug.contains("404"),
            "el Debug del error no lleva lo que hace falta para diagnosticarlo: {debug}"
        );
    }

    /// El motivo nombra el contenedor **aunque el motor no lo haga**.
    ///
    /// Es la mitad que de verdad se prueba: con el mensaje medido, un código que se limitara
    /// a copiarlo pasaría la prueba de arriba sin poner nada de su parte. Aquí el mensaje no
    /// lleva el nombre y el motivo lo tiene que llevar igual.
    #[test]
    fn el_motivo_nombra_el_contenedor_aunque_el_motor_no_lo_haga() {
        let medida = ErrorDelMotor::DockerResponseServerError {
            status_code: 404,
            message: "no such container".to_owned(),
        };

        let motivo = Error::del_contenedor("mi-contenedor", medida).to_string();

        assert!(
            motivo.contains("mi-contenedor"),
            "el motivo se conformó con el texto del motor y no nombró el contenedor: {motivo}"
        );
    }

    /// Un `500` del motor conserva su código y su mensaje.
    ///
    /// Se distingue del 404 porque no significan lo mismo: el 404 dice que el contenedor no
    /// está, y el 500 —el que midió el `attach` ausente— dice que la petición no se entendió.
    #[test]
    fn un_500_del_motor_conserva_su_codigo_y_su_mensaje() {
        let medida = ErrorDelMotor::DockerResponseServerError {
            status_code: 500,
            message: MENSAJE_DEL_500.to_owned(),
        };

        let error = Error::del_contenedor("mi-contenedor", medida);
        let motivo = error.to_string();

        assert!(
            matches!(
                &error,
                Error::MotorContesta {
                    status_code: 500,
                    message,
                    ..
                } if message == MENSAJE_DEL_500
            ),
            "el 500 no llegó como respuesta del motor con su mensaje: {error:?}"
        );
        assert!(
            motivo.contains(MENSAJE_DEL_500),
            "el motivo no conserva el mensaje del motor: {motivo}"
        );
    }

    /// Un fallo que **no** es una respuesta del servidor no se convierte en un código de
    /// respuesta: se conserva como causa.
    ///
    /// Es la diferencia entre «el motor dijo que no» y «no se pudo preguntar». Inventar un
    /// `status_code` para lo segundo mandaría a buscar un contenedor inexistente cuando el
    /// problema es el transporte.
    #[test]
    fn un_fallo_que_no_es_del_servidor_se_conserva_como_causa() {
        let error = Error::del_contenedor("mi-contenedor", ErrorDelMotor::RequestTimeoutError);

        assert!(
            matches!(&error, Error::MotorNoContesta { contenedor, .. } if contenedor == "mi-contenedor"),
            "un fallo de transporte no puede llegar como respuesta del motor: {error:?}"
        );
        assert!(
            std::error::Error::source(&error).is_some(),
            "la causa del fallo de transporte se perdió: {error:?}"
        );
    }

    /// Un socket que no existe se reconoce como tal, y el motivo nombra la ruta pedida.
    #[test]
    fn un_socket_inexistente_se_reconoce_por_su_ruta() {
        let error = Error::del_socket(
            "unix:///no/existe/kesto.sock",
            ErrorDelMotor::SocketNotFoundError("/no/existe/kesto.sock".to_owned()),
        );

        assert!(
            matches!(
                &error,
                Error::SocketInexistente { socket } if socket == "unix:///no/existe/kesto.sock"
            ),
            "el socket no se reconoció con la ruta que se pidió: {error:?}"
        );
        assert!(
            error.to_string().contains("unix:///no/existe/kesto.sock"),
            "el motivo no nombra el socket: {error}"
        );
    }

    /// El `Debug` de una orden publica el comando y los **nombres** del entorno, nunca sus
    /// valores.
    ///
    /// El entorno es donde viajan las credenciales de la base, así que un `Debug` derivado
    /// convertiría un registro de depuración en una contraseña escrita en el log.
    #[test]
    fn el_debug_de_una_orden_no_publica_el_entorno() {
        let orden = Orden {
            cmd: vec!["mariadb-dump".to_owned(), "--all-databases".to_owned()],
            env: vec!["MYSQL_PWD=secreto-de-prueba".to_owned()],
        };

        let debug = format!("{orden:?}");

        assert!(
            debug.contains("mariadb-dump"),
            "el Debug no publica el comando: {debug}"
        );
        assert!(
            debug.contains("MYSQL_PWD"),
            "el Debug no publica el nombre de la variable: {debug}"
        );
        assert!(
            !debug.contains("secreto-de-prueba"),
            "el Debug publicó el valor del entorno: {debug}"
        );
    }

    /// El `Debug` de los canales dice cuál es cuál, que es lo que hace legible un fallo.
    #[test]
    fn los_dos_canales_son_distintos_y_se_leen() {
        assert_ne!(Canal::Salida, Canal::Error);
        assert_eq!(format!("{:?}", Canal::Salida), "Salida");
        assert_eq!(format!("{:?}", Canal::Error), "Error");
    }
}
