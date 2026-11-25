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

#[cfg(any(feature = "motor", test))]
mod cliente;
#[cfg(any(feature = "motor", test))]
pub use cliente::Cliente;
// Estos dos ayudantes solo los llama el servidor IPC del motor; las pruebas del
// cliente los importan directamente del módulo, así que no hace falta reexportarlos.
#[cfg(feature = "motor")]
pub(crate) use cliente::{codigo_de_salida, exigir_la_etiqueta_de_fuente};

// El arnés de pruebas (`src/test_support.rs`, los tests de `engine`) usa el
// cliente real contra el socket de Podman: con `cfg(test)` tiene que compilarse
// el mismo transporte bollard que usa el motor, o las pruebas `#[ignore]`
// intentarían hablar con Docker por el canal IPC y se colgarían. La API de
// producción —sin `motor` y sin `test`— sigue usando solo el cliente IPC.
#[cfg(not(any(feature = "motor", test)))]
mod ipc_cliente;
#[cfg(not(any(feature = "motor", test)))]
pub use ipc_cliente::Cliente;

/// Causa del fallo, según el proceso: el error de la biblioteca en el motor y
/// en el arnés de pruebas, el del canal IPC en la API de producción.
#[cfg(any(feature = "motor", test))]
type CausaDelMotor = bollard::errors::Error;
/// Causa del fallo en la API, que no enlaza la biblioteca del daemon.
#[cfg(not(any(feature = "motor", test)))]
type CausaDelMotor = crate::ipc::Error;

use std::time::Duration;

use thiserror::Error;

/// La etiqueta con la que un contenedor se declara **fuente de copia**.
///
/// El motor inspecciona el contenedor de un target **antes** de crear ningún `exec` y exige
/// que lleve esta etiqueta con el valor [`VALOR_DE_FUENTE_DE_COPIA`]. Sin ella no se ejecuta
/// nada —es **fail-closed**—, y la declaración **viaja con el contenedor**: quien lo crea dice
/// si es fuente de copia, así que no hay una lista de configuración que poblar ni dos sitios
/// que mantener en sincronía. El propio contenedor de Kesto queda denegado por sí solo, porque
/// nadie lo etiqueta: no hace falta cablear su nombre, que cambiaría con cualquier renombrado
/// del despliegue.
pub const ETIQUETA_DE_FUENTE_DE_COPIA: &str = "io.kesto.backup-source";

/// El valor de la etiqueta que declara un contenedor fuente de copia.
///
/// Cualquier otro valor —`"1"`, `"yes"`, `"false"`— deniega, igual que la etiqueta ausente: la
/// comparación es **exacta** y no se acepta una aproximación que se leería como un sí sin
/// serlo.
pub const VALOR_DE_FUENTE_DE_COPIA: &str = "true";

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
///
/// # Seguridad
///
/// El [`Debug`](std::fmt::Debug) de este tipo **no** publica los valores del entorno, y eso
/// es necesario pero **no suficiente**: el `Debug` de aquí no es el único camino por el que
/// el valor puede salir. `bollard` registra a `DEBUG` el **cuerpo serializado** de cada
/// petición —medido: `serialize_payload` en `bollard-0.18.1/src/docker.rs:1227`—, y el
/// cuerpo de un `create_exec` lleva el `env` **entero**, con la contraseña dentro. Como el
/// suscriptor de registro instala el puente `log` → `tracing`, esas líneas acaban en
/// nuestro registro con solo subir el nivel a `debug`.
///
/// Es decir: **la exposición se corta por filtro, no por este `Debug`**. Quien lea esto y se
/// quede tranquilo por no usar `#[derive(Debug)]` se queda tranquilo en el sitio que no
/// toca. Lo que hay que hacer es no registrar el target `bollard` a `debug`, y de eso se
/// encarga el **proceso**: el arranque pone la directiva del proyecto detrás del filtro que
/// pida quien lance el binario —[`crate::DIRECTIVA_DEL_MOTOR`]—, de modo que no queda a la
/// memoria de quien lo ejecute; el `.justfile` la repite en `just dev` y en `just run`, y
/// explica allí por qué. Ver también el `# Seguridad` de [`Cliente::ejecutar`], que es donde
/// se pide el `exec`.
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
    ///
    /// Necesario, pero **no suficiente**: el valor también sale por debajo de este módulo, en
    /// la petición que serializa la biblioteca. Está contado en el `# Seguridad` de arriba.
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
///
/// # El motor no debe registrar nunca un `Trozo`
///
/// Su `Debug` se deriva, y eso es deliberado: imprime los bytes del trozo tal cual —la prueba
/// de los dos canales los enseña— y las pruebas de este módulo se apoyan en verlos. El precio
/// es el que hay que saber: un `Trozo` es la **copia de los datos**, así que registrarlo
/// escribe el volcado —las filas de las tablas, con sus datos personales— en el registro del
/// proceso. El motor cuenta progreso con el **tamaño** de los trozos, no con su contenido:
/// nunca `tracing::debug!(?trozo)`, ni el `Trozo` dentro de un evento, ni `{:?}` en un
/// mensaje. Si hay que diagnosticar de verdad, el canal y el número de bytes bastan.
#[derive(Debug)]
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
    ///
    /// El `Display` publica la causa para que el operador sepa qué falló, pero nunca
    /// nombra el socket: este error cruza la frontera IPC y acaba en `error_message`,
    /// que muestra el panel. La ruta se conserva en el campo para los registros y
    /// diagnósticos internos; en cambio, no se interpola en ese mensaje.
    #[error("no se pudo hablar con el motor de contenedores: {causa}")]
    MotorNoDisponible {
        /// Socket con el que se intentó.
        socket: String,
        /// Causa, tal y como la informa la biblioteca.
        #[source]
        causa: CausaDelMotor,
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
        causa: CausaDelMotor,
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

    /// La orden dejó de producir salida y el plazo de inactividad venció.
    ///
    /// Es un fallo **con nombre propio** y no un tiempo máximo agotado, porque el tiempo
    /// máximo del cliente no acota esto: acota el apretón de manos, y se resuelve cuando
    /// llegan las cabeceras. Lo que se acota aquí es la **inactividad entre trozos**, que es
    /// lo único que distingue un volcado lento de uno colgado: un volcado que avanza tarda
    /// lo que haga falta y no puede fallar por esto.
    ///
    /// Que exista esta variante es un requisito del motor y no un adorno: sin acotar la
    /// inactividad, un `mariadb-dump` colgado deja la ejecución en `running` para siempre y
    /// **el worker de su repositorio ocupado**, con todo el trabajo posterior de ese
    /// repositorio haciendo cola detrás. El motivo nombra el contenedor y el plazo, que es lo
    /// que hace falta para saber cuánto estuvo callado antes de darlo por colgado.
    #[error("la orden en `{contenedor}` no produjo salida en {inactividad:?}: puede estar colgada")]
    ExtraccionEstancada {
        /// Contenedor en el que se ejecutaba la orden.
        contenedor: String,
        /// Plazo de inactividad que venció.
        inactividad: Duration,
    },

    /// La orden de restauración dejó de producir salida y el plazo de inactividad venció.
    ///
    /// Es el espejo de [`ExtraccionEstancada`](Self::ExtraccionEstancada) para el camino de la
    /// restauración, y existe con nombre propio por la misma razón por la que existe el otro: lo
    /// que se acota es la **inactividad entre trozos** —el cliente de la base lento pero que avanza
    /// tarda lo que haga falta y no puede fallar por esto—, y un cliente colgado que no dice nada
    /// se caza aquí. El motivo nombra el contenedor y el plazo, que es lo que hace falta para saber
    /// cuánto estuvo callado antes de darlo por colgado.
    #[error(
        "la orden de restauración en `{contenedor}` no produjo salida en {inactividad:?}: el \
         cliente de la base puede estar colgado"
    )]
    RestauracionEstancada {
        /// Contenedor en el que se ejecutaba la orden.
        contenedor: String,
        /// Plazo de inactividad que venció.
        inactividad: Duration,
    },

    /// El volcado no llegó entero al `stdin` del `exec`.
    ///
    /// Es el fallo del lado de la entrada, y no del contenedor: el repositorio no pudo volcar el
    /// nodo, o el `stdin` de la orden rechazó la escritura, o la orden terminó **antes** de recibir
    /// el volcado entero. Se propaga **como error y no como un fin de fichero limpio**: dar por
    /// buena una restauración incompleta —porque el contenedor salió con cero, o porque su cliente
    /// decidió que ya tenía bastante— es justo el desenlace que este tipo existe para no cometer.
    /// La causa es la del puente —nombra el volcado y el motivo del repositorio—, la del `stdin` o
    /// la de la orden que se fue sin el volcado.
    #[error("no se pudo entregar el volcado al `stdin` de la orden en `{contenedor}`: {causa}")]
    EntradaNoEntregada {
        /// Contenedor en el que se ejecutaba la orden.
        contenedor: String,
        /// La causa: el volcado a medias, el `stdin` que rechazó la escritura, o la orden que
        /// terminó antes de recibir el volcado entero.
        #[source]
        causa: std::io::Error,
    },

    /// El contenedor no se declara fuente de copia: no se le crea ningún `exec`.
    ///
    /// El motor inspecciona el contenedor antes de crear el `exec` y exige la etiqueta
    /// [`ETIQUETA_DE_FUENTE_DE_COPIA`] con el valor [`VALOR_DE_FUENTE_DE_COPIA`]. Sin ella la
    /// orden no se ejecuta —**fail-closed**— y la ejecución acaba en `failed`.
    ///
    /// Es un desenlace **distinto** del de «no se pudo inspeccionar» —un contenedor que no
    /// existe o un motor que no contesta llegan como [`Error::MotorContesta`] o
    /// [`Error::MotorNoContesta`], con su código y su causa—: aquí el motor sí contestó y el
    /// contenedor está, pero no se ha declarado fuente de copia. Confundir los dos mandaría a
    /// mirar el motor cuando lo que falta es la etiqueta del contenedor.
    ///
    /// El motivo **nombra el contenedor** y no publica nada más: es lo que el operador
    /// necesita para saber qué contenedor recrear con la etiqueta.
    #[error(
        "el contenedor `{contenedor}` no se declara fuente de copia: no lleva la etiqueta \
         `io.kesto.backup-source=true`, así que el motor no crea ningún `exec` en él"
    )]
    ContenedorNoEsFuenteDeCopia {
        /// Contenedor que no se declaró fuente de copia.
        contenedor: String,
    },
}

#[cfg(any(feature = "motor", test))]
impl Error {
    /// Traduce un fallo del motor que se refiere a un contenedor concreto.
    ///
    /// Un fallo de respuesta del servidor se convierte en [`Error::MotorContesta`]
    /// conservando su `status_code` y su `message`; cualquier otro se conserva como causa
    /// de [`Error::MotorNoContesta`]. Se distinguen porque no se arreglan igual: lo
    /// primero es el motor diciendo que no a algo concreto, lo segundo es no haber podido
    /// preguntarle.
    pub(crate) fn del_contenedor(contenedor: &str, causa: bollard::errors::Error) -> Self {
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
    pub(crate) fn del_socket(socket: &str, causa: bollard::errors::Error) -> Self {
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

#[cfg(all(test, not(feature = "motor")))]
mod tests {
    use std::time::Duration;

    #[cfg(feature = "motor")]
    use bollard::errors::Error as ErrorDelMotor;

    use super::{Canal, Error, Orden};

    /// El mensaje que el motor da cuando el contenedor no existe, **medido** contra el
    /// Podman de esta máquina: el nombre del contenedor viaja dentro, entrecomillado.
    #[cfg(feature = "motor")]
    const MENSAJE_DEL_404: &str =
        "no container with name or ID \"mi-contenedor\" found: no such container";

    /// El mensaje que el motor da cuando el `exec` se pide sin ningún `attach`, también
    /// medido: es un `500` del motor, no un fallo del comando.
    #[cfg(feature = "motor")]
    const MENSAJE_DEL_500: &str = "must provide at least one stream to attach to: invalid argument";

    /// Un 404 del motor se traduce a nuestro error y **el motivo nombra el contenedor**.
    ///
    /// Es el requisito que impide que un contenedor mal escrito se lea como un fallo de
    /// volcado: el motivo tiene que llevar el nombre, y no se delega en el texto del motor
    /// —que hoy lo lleva— porque ese texto es suyo.
    #[cfg(feature = "motor")]
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
    #[cfg(feature = "motor")]
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
    #[cfg(feature = "motor")]
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
    #[cfg(feature = "motor")]
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
    #[cfg(feature = "motor")]
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

    /// **El plazo vencido de una restauración se nombra como tal, y no como el de una copia.**
    ///
    /// Son dos desenlaces distintos —uno acota el silencio de una orden que debería **producir**
    /// salida, el otro el de una que debería **consumir** su entrada y contestar— y el operador los
    /// lee: el motivo dice cuál venció, en qué contenedor y con qué plazo. Los dos plazos se
    /// resuelven con el mismo número, así que confundirlos mandaría a mirar el sitio equivocado.
    #[test]
    fn el_plazo_vencido_de_una_restauracion_se_distingue_del_de_una_copia() {
        let plazo = Duration::from_secs(300);

        let restauracion = Error::RestauracionEstancada {
            contenedor: "contenedor-de-la-base".to_owned(),
            inactividad: plazo,
        };
        let copia = Error::ExtraccionEstancada {
            contenedor: "contenedor-de-la-base".to_owned(),
            inactividad: plazo,
        };
        let motivo = restauracion.to_string();

        assert!(
            motivo.contains("contenedor-de-la-base") && motivo.contains("300s"),
            "el motivo no nombra el contenedor y el plazo que venció: {motivo}"
        );
        assert!(
            motivo.contains("restauración"),
            "el motivo no dice que el plazo vencido es el de una restauración: {motivo}"
        );
        assert_ne!(
            motivo,
            copia.to_string(),
            "el plazo de la restauración y el de la copia se leen igual: {motivo}"
        );
    }
}

#[cfg(test)]
mod pruebas_de_redaccion {
    use super::Error;

    /// El `Display` de este error llega al panel: ni la ruta del socket ni una credencial
    /// incrustada en ella pueden cruzar esa frontera.
    #[test]
    fn motor_no_disponible_no_publica_la_ruta_del_socket_en_el_display() {
        let socket = String::from("/run/secreto/motor.sock?token=CLAVE");
        let error = Error::MotorNoDisponible {
            socket: socket.clone(),
            causa: bollard::errors::Error::RequestTimeoutError,
        };

        let motivo = error.to_string();
        assert!(
            !motivo.contains("/run/secreto") && !motivo.contains("CLAVE"),
            "el Display publicó la ruta o la credencial: {motivo}"
        );
        assert!(
            matches!(&error, Error::MotorNoDisponible { socket: guardado, .. } if guardado == &socket),
            "el campo socket se perdió: {error:?}"
        );
    }
}
