//! El motor de copias: qué se copia, por dónde sale y en qué orden.
//!
//! Este módulo es la lógica del §4 y del §2 del SRS, y tiene las dos mitades de la pieza:
//!
//! - [`extraccion`], que va de la fila del §3 hasta los bytes que hay que copiar: la elección de
//!   la ruta, el comando de volcado de cada tipo de target, la credencial por el entorno del `exec`
//!   y la conducción de ese proceso;
//! - y la orquestación —la cola y el trabajador por repositorio (`cola`), el ciclo de vida de una
//!   ejecución en `job_executions` (`ejecucion`), el hub de eventos que alimenta el stream
//!   (`hub`) y el arranque y la parada—, que es lo que hay en este fichero.
//!
//! # El motor trabaja en hilos propios, y nunca dentro del runtime de Tokio
//!
//! No es una preferencia de estilo: es la única forma que funciona. El backend `opendal` de
//! `rustic_backend` crea su **propio** runtime de Tokio, y llamar a Rustic desde dentro de otro
//! runtime entra en **pánico** —medido, literal: «Cannot start a runtime from within a runtime.»—.
//! Y `archive()` es una función **bloqueante** que puede tardar minutos: un `block_on` sobre el
//! runtime que atiende las peticiones HTTP detendría el hilo que despacha y, con él, el resto de la
//! aplicación.
//!
//! De ahí las dos mitades de la regla, que están escritas en los sitios donde se cumplen: el
//! trabajo va en hilos de sistema operativo creados por el motor, y quien vuelve al runtime de la
//! aplicación lo hace **explícitamente** y **solo** para hablar con la base.
//!
//! Y la misma regla gobierna las dos operaciones que el motor presta **sin** ser una copia
//! —[`Motor::abrir_repositorio`] y [`Motor::listar_snapshots`]—, que son las que necesita la capa
//! HTTP para el alta de un repositorio y para el explorador de instantáneas. Las dos son `async`
//! porque quien las llama es un manejador, y las dos sacan su trabajo del runtime por su cuenta
//! ([`fuera_del_runtime`]): la restricción del pánico queda en **un** sitio, el módulo que la
//! midió, en vez de repartida entre quien recuerde que existe.
//!
//! # Por dónde va el volcado, en una línea
//!
//! ```text
//! fila del §3 → Objetivo → ruta() → { PorContenedor → volcado → puente → hilo del trabajador
//!                                  { PorFichero    → el origen local de la biblioteca
//! ```
//!
//! El puente —[`crate::rustic::tunel_del_volcado`]— vive en `src/rustic` y **no** aquí: es un tipo
//! que existe para la API de la biblioteca, y traerlo al motor obligaría al motor a importar
//! `rustic_core`, que es justo lo que la separación evita. Lo que sí vive aquí es quién lo alimenta
//! y con qué plazo: ver [`INACTIVIDAD_MAXIMA`].
//!
//! # El nombre del error
//!
//! Cada mitad del motor tiene su error —la extracción tiene [`ErrorDeExtraccion`]— y ninguno se
//! llama `Error` a secas: cuando este módulo nombra a la vez su error, el del transporte
//! ([`crate::docker::Error`]) y el de la biblioteca ([`crate::rustic::Error`]), un `Error` pelado
//! obligaría a alias en cada import y dejaría el nombre más corto para el que menos lo merece. El
//! de aquí es [`ErrorDelMotor`], y los desenlaces que le corresponden **a un código HTTP** lo dicen
//! en su propia documentación: esa correspondencia es contrato y no puede vivir solo en el
//! manejador.

mod cola;
mod ejecucion;
pub mod extraccion;
mod hub;
pub mod restauracion;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use thiserror::Error;
use time::OffsetDateTime;
use tokio::runtime::Handle;
use uuid::Uuid;

use crate::db::DbError;
use crate::db::entities::{
    AltaDeEjecucion, Ejecucion, Ejecuciones, Estado, ObjetivoDeRestauracion, Repositorios,
    Restauraciones, Tareas, Targets,
};
use crate::docker::{Cliente, Orden};
use crate::rustic::{Acceso, BarrasDelMotor, PuestaEnMarcha, Repositorio, ResumenDeSnapshot};

use cola::Cola;
pub use hub::{CAPACIDAD_DEL_HUB, EventoDeEjecucion, EventoDeRestauracion, EventoDelMotor, Hub};

use extraccion::recortar;
pub use extraccion::{
    CAPACIDAD_DEL_CANAL, ErrorDeExtraccion, INACTIVIDAD_MAXIMA, Objetivo, Resultado, Ruta, ruta,
    volcar,
};
pub use restauracion::{CaminoDeRestauracion, ErrorDeRestauracion, comando};
use restauracion::{EscritorDelVolcado, TunelDeRestauracion, tunel_de_restauracion};

// Las dos constantes del puente, que el motor pasa a las dos esperas del volcado. Son las
// **mismas** para el transporte y para el lector del canal —es la misma pregunta vista desde dos
// sitios— y por eso se publican desde aquí: quien orqueste la copia las pasa tal cual en vez de
// elegir un número por su cuenta. `pub` y no `pub(crate)` por la regla de la casa: lo que no es
// alcanzable desde la superficie pública del crate se considera código muerto, y lo que consume
// estas dos son el transporte y el puente, que ya están.

/// Tiempo máximo por petición al motor de contenedores, por defecto.
///
/// Es el `timeout` del cliente HTTP del motor, y hay que saber qué acota: **el apretón de manos,
/// no el trabajo** —está medido: el `timeout` de la biblioteca se resuelve cuando llegan las
/// cabeceras, y el cuerpo del `exec` se drena después, fuera de él—. Lo que acota el volcado es
/// [`INACTIVIDAD_MAXIMA`].
///
/// El valor: cinco minutos. Holgado a propósito y **medido**: con varios contenedores creándose a
/// la vez, las peticiones al motor se hacen cola y una de ellas pasó de los sesenta segundos con el
/// motor perfectamente vivo. Un tiempo corto aquí solo produce copias que fallan por el entorno.
const ESPERA_DEL_MOTOR: Duration = Duration::from_secs(300);

/// Cuánto se espera a los trabajadores en la parada, por defecto.
///
/// Cinco segundos y no más: el `grace period` por defecto de un `docker stop` son diez, y la espera
/// tiene que caber **dentro** de él con margen para el resto del apagado. Esperar más no sirve de
/// nada —el orquestador resuelve con un `SIGKILL` cuando se le acaba el plazo, que es justo lo que
/// la parada ordenada existía para evitar—, y esperar menos convierte el apagado en un corte.
///
/// Una copia puede tardar minutos, así que lo normal es que **no** termine: lo que se pide es que
/// el proceso acabe, y lo que no acabe queda registrado como tal.
const ESPERA_DE_PARADA: Duration = Duration::from_secs(5);

/// Cómo se abre un repositorio a partir de su fila.
///
/// Es un punto de inyección, y no un adorno: sin él, la prueba del ciclo de vida completo
/// necesitaría un S3 de verdad, que no hay, y la de la serialización necesitaría medir tiempo. Con
/// él, la prueba abre un repositorio **local** —`rustic_backend::local`— o devuelve un fallo
/// controlado, y el proceso abre el de S3.
///
/// Es una **función** y no un cierre a propósito: un puntero a función no captura nada, así que lo
/// que se inyecta no puede llevar dentro credenciales ni una ruta que no se vea en el sitio donde
/// se inyectó.
pub type AbridorDeRepositorios =
    fn(&Acceso, BarrasDelMotor) -> Result<(Repositorio, PuestaEnMarcha), crate::rustic::Error>;

/// Los ajustes del motor: con qué habla y cuánto espera.
///
/// Los dos plazos van aquí y no son constantes sueltas por la misma razón que el abridor: son lo
/// que una prueba necesita cambiar para no esperar de verdad, y lo que un despliegue puede querer
/// ajustar sin recompilar.
#[derive(Clone)]
pub struct Ajustes {
    /// Socket del motor de contenedores, tal y como lo resuelve la configuración.
    pub socket_del_motor: String,
    /// Tiempo máximo por petición al motor de contenedores.
    pub espera_del_motor: Duration,
    /// Cuánto se espera a los trabajadores al parar el motor.
    pub espera_de_parada: Duration,
    /// Cómo se abre un repositorio: el de S3 en producción, el local en las pruebas.
    pub abridor: AbridorDeRepositorios,
}

impl Ajustes {
    /// Los ajustes de producción, con el abridor de S3 y las dos esperas por defecto.
    ///
    /// Es **síncrona** y no habla con nadie: montar el motor no comprueba que el socket del motor
    /// de contenedores conteste. Eso lo hace [`Motor::comprobar`], y quien lo llama es el arranque,
    /// para **avisar**: un socket que no responde no puede impedir que el proceso arranque, porque
    /// el panel tiene que poder levantarse precisamente para enseñar que no responde.
    pub fn nuevos(socket: impl Into<String>) -> Self {
        Self {
            socket_del_motor: socket.into(),
            espera_del_motor: ESPERA_DEL_MOTOR,
            espera_de_parada: ESPERA_DE_PARADA,
            abridor: Repositorio::poner_en_marcha,
        }
    }

    /// Los mismos ajustes con otro abridor de repositorios.
    pub fn con_abridor(mut self, abridor: AbridorDeRepositorios) -> Self {
        self.abridor = abridor;
        self
    }
}

impl std::fmt::Debug for Ajustes {
    /// Publica lo que se puede publicar de unos ajustes: dónde está el motor de contenedores y
    /// cuánto se espera. El abridor es un puntero y no dice nada, así que va como `abridor`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ajustes")
            .field("socket_del_motor", &self.socket_del_motor)
            .field("espera_del_motor", &self.espera_del_motor)
            .field("espera_de_parada", &self.espera_de_parada)
            .finish_non_exhaustive()
    }
}

/// Las cinco capas de acceso que el motor usa.
///
/// Existe para que el motor **no** tenga que nombrar [`crate::state::EstadoDeBase`]: `src/engine`
/// no importa `src/state`, y así el día que el `AppState` lleve el motor no hay un ciclo de
/// módulos. Quien lo construye es el arranque, con las capas del estado, y eso son unas pocas
/// líneas.
///
/// Van tras un [`Arc`] por lo mismo que en el resto del programa: el motor se clona y las capas no
/// son clonables, así que clonar el motor no vuelve a construir ninguna.
#[derive(Clone)]
pub struct Dependencias {
    /// La capa de acceso de `repositories`.
    pub repositorios: Arc<Repositorios>,
    /// La capa de acceso de `backup_targets`.
    pub targets: Arc<Targets>,
    /// La capa de acceso de `backup_tasks`.
    pub tareas: Arc<Tareas>,
    /// La capa de acceso de `job_executions`.
    pub ejecuciones: Arc<Ejecuciones>,
    /// La capa de acceso de `restore_executions`.
    ///
    /// Es la quinta y no la cuarta que había: la restauración de una base necesita anotar su ciclo
    /// de vida —de `running` a `success` o `failed`— en la fila que el manejador dejó en `pending`,
    /// y esa fila es de esta capa y de ninguna otra.
    pub restauraciones: Arc<Restauraciones>,
}

impl std::fmt::Debug for Dependencias {
    /// No publica nada de dentro: las capas llevan el pool y la clave de cifrado, y lo que un
    /// diagnóstico necesita saber es que están, no cómo son por dentro.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dependencias").finish_non_exhaustive()
    }
}

/// El error del motor de copias.
///
/// Se llama [`ErrorDelMotor`] y **no** `Error` a secas por lo mismo que el de la extracción: aquí
/// se nombran a la vez el error de la biblioteca, el del transporte y el de la base, y un `Error`
/// pelado dejaría el nombre más corto para el que menos lo merece.
///
/// **Ningún motivo puede llevar un secreto**, y por eso los motivos que vienen de fuera se pasan
/// por la red de la extracción antes de guardarse.
///
/// # La correspondencia con los códigos HTTP es contrato
///
/// Cada variante dice en su documentación **a qué código corresponde** cuando la usa la capa HTTP.
/// Vive aquí y no en el manejador a propósito: un `409` que solo esté escrito en el manejador es un
/// contrato que nadie puede leer desde la definición del error.
#[derive(Debug, Error)]
pub enum ErrorDelMotor {
    /// La tarea no existe. **Corresponde a un `404`**: no hay nada que encolar.
    ///
    /// Se comprueba **antes** de crear la fila, y no se deja que la clave foránea del §3 lo
    /// rechace: un fallo de clave foránea llegaría como un error de base —un `500`— y mandaría al
    /// operador a mirar la base en vez de su fila.
    #[error("la tarea `{job_id}` no está: no hay nada que encolar")]
    TareaQueNoEsta {
        /// La tarea que se pidió disparar.
        job_id: Uuid,
    },

    /// La tarea ya tiene una ejecución `pending` o `running`. **Corresponde a un `409`**.
    ///
    /// Encolarla otra vez no acelera nada —el trabajador de su repositorio las atendería de una en
    /// una—, la pone a esperar a la primera y deja al panel con dos barras para un solo trabajo. Lo
    /// que la hace cierta es la guarda **atómica** de la capa de acceso
    /// (`alta_si_no_hay_ninguna_en_curso`), no una lectura previa: entre la lectura y la escritura
    /// caben las peticiones de otro.
    #[error("la tarea `{job_id}` ya tiene una ejecución en curso: no se encola una segunda")]
    YaTieneUnaEnCurso {
        /// La tarea que ya estaba corriendo.
        job_id: Uuid,
    },

    /// Ese destino ya tiene una restauración **en vuelo**. **Corresponde a un `409`**.
    ///
    /// Es el desenlace de la guarda de exclusión de [`Motor::reservar_el_destino`]: dos
    /// restauraciones sobre el mismo `host_path` escribirían a la vez en el mismo directorio del
    /// host, así que la segunda se rechaza en vez de lanzarse.
    ///
    /// El sujeto es el **destino** y no la instantánea a propósito: dos instantáneas distintas del
    /// mismo repositorio que se restauran sobre el mismo target chocan exactamente igual —el disco
    /// es el mismo—, y por eso la reserva se lleva por ruta.
    ///
    /// El `destino` viaja para el **registro** —es una ruta de la fila del target, no un dato que
    /// mandara la petición— y no se publica: el mensaje del `409` lo compone el mapeo de la capa
    /// HTTP, que es quien decide qué se cuenta y qué no.
    #[error("el destino `{destino}` ya tiene una restauración en curso: no se lanza una segunda")]
    RestauracionEnCurso {
        /// El destino —el `host_path` del target— que ya estaba reservado.
        destino: String,
    },

    /// El motor ha dejado de aceptar trabajo. **Corresponde a un `503`**.
    ///
    /// Es el desenlace de un disparo que llega después de que el proceso haya empezado a apagarse.
    #[error("el motor está parado: no acepta trabajo nuevo")]
    MotorParado,

    /// La cola de ese repositorio está llena. **Corresponde a un `503`**.
    ///
    /// Con la guarda del `409` puesta y `CAPACIDAD_DE_LA_COLA` de fondo, esto es un caso
    /// extremo: muchas tareas distintas apuntando al mismo repositorio y esperando a la vez. La
    /// fila que se acababa de crear se marca como fallida antes de devolver este error, porque una
    /// fila `pending` que nadie va a ejecutar es peor que un error.
    #[error(
        "la cola del repositorio `{repository_id}` está llena: la ejecución no ha podido entrar, y \
         se ha apuntado como fallida en vez de dejarla esperando"
    )]
    ColaDelRepositorioLlena {
        /// El repositorio cuya cola está llena.
        repository_id: Uuid,
    },

    /// La cola de ese repositorio no tiene quien la atienda. **Corresponde a un `503`**.
    ///
    /// Dos desenlaces con el mismo nombre porque para el operador son el mismo —no hay trabajador—:
    /// no se pudo crear su hilo, o el que había ya no está. El motivo dice cuál de los dos, y los
    /// dos se arreglan mirando el proceso, no la fila del §3.
    ///
    /// # Por qué esta variante no tiene prueba
    ///
    /// Y se dice en vez de dejarse sin mirar: los dos desenlaces están fuera del alcance de una
    /// prueba, y por motivos distintos. El primero —no poder crear el hilo— solo ocurre si el
    /// sistema operativo se queda sin recursos, y provocarlo exigiría agotar los hilos del proceso:
    /// una prueba que fallaría por el entorno y no por el código. El segundo —el canal
    /// desconectado— es **inalcanzable por construcción**, y el camino del encolado lo dice con
    /// todas las letras: mientras el buzón siga en el mapa su emisor vive, y el emisor solo muere
    /// cuando el buzón se suelta, que es lo que cierra el canal. No se le abre a la cola un gancho
    /// de prueba solo para recorrerlo. Lo que sí queda cubierto es el `503` que el operador ve de
    /// verdad en ese camino —el de [`ColaDelRepositorioLlena`](ErrorDelMotor::ColaDelRepositorioLlena)
    /// y el de [`MotorParado`](ErrorDelMotor::MotorParado)—, que sí tienen prueba.
    #[error("el repositorio `{repository_id}` no tiene quien atienda su cola: {motivo}")]
    SinQuienAtiendaLaCola {
        /// El repositorio que se ha quedado sin trabajador.
        repository_id: Uuid,
        /// Cuál de los dos desenlaces fue.
        motivo: String,
    },

    /// La base no ha podido atender el encolado. **Corresponde a un `500`**.
    #[error("la base no ha podido atender el encolado: {causa}")]
    Base {
        /// El fallo de la capa de acceso, tal cual.
        #[source]
        causa: DbError,
    },

    /// El motor de contenedores no contesta.
    ///
    /// Lo devuelve [`Motor::comprobar`], que el arranque llama para **avisar** y nunca para
    /// decidir si el proceso arranca: un socket que no responde no puede dejar al operador sin la
    /// herramienta con la que diagnosticarlo.
    #[error("el motor de contenedores no contesta: {causa}")]
    MotorDeContenedores {
        /// El fallo del transporte, tal cual.
        #[source]
        causa: crate::docker::Error,
    },

    /// La biblioteca de copia no ha podido hacer su trabajo contra el repositorio.
    ///
    /// Lo devuelven las **cuatro** operaciones que el motor presta sin ser una copia:
    /// [`Motor::abrir_repositorio`], [`Motor::listar_snapshots`], [`Motor::tiene_la_instantanea`] y
    /// [`Motor::restaurar`]. En ninguna hay una fila de ejecución que apuntar —la de la
    /// restauración es **efímera** y no se persiste—, así que el fallo se cuenta a quien preguntó y
    /// no queda escrito en ninguna fila; y cuando no hay nadie a quien contárselo, se queda en el
    /// registro (ver más abajo el caso de la restauración).
    ///
    /// # La correspondencia con los códigos HTTP la elige quien llama
    ///
    /// Este es el único desenlace del motor en el que el código **no** se puede fijar aquí, y por
    /// eso la [`causa`](Self::Repositorio::causa) viaja entera y con su tipo: el alta de un
    /// repositorio de la API tiene que contestar `502` cuando el destino no se alcanza y `400`
    /// cuando la passphrase no abre uno que ya existe —dos desenlaces que el operador arregla de
    /// maneras distintas—, y distinguirlos exige mirar el error de la biblioteca, no su texto.
    ///
    /// Lo que **no** se publica es la causa: lo que sale de aquí hacia el operador es
    /// [`motivo`](Self::Repositorio::motivo), que va ya pasado por la red de secretos de la
    /// extracción —un error de un backend puede llevar dentro la URL del destino con su
    /// credencial, y esto se guarda y se muestra—.
    ///
    /// Y en la **restauración** ese `502` no llega a nadie: el trabajo va en segundo plano con el
    /// `200` ya entregado, así que la correspondencia la consume el **registro** —el motivo viaja
    /// con su `tracing::error!`— y el cliente se quedó con el `404`, el `409` o el `422` de la
    /// ruta, o con el `200` si la petición llegó a lanzarse. El `502` que sí contesta es el de un
    /// fallo del motor **antes** de lanzarla: el [`Motor::tiene_la_instantanea`] con el que el
    /// manejador comprueba que la copia sigue en el repositorio.
    #[error("el repositorio no ha respondido: {motivo}")]
    Repositorio {
        /// El fallo de la biblioteca, para quien tenga que **decidir** con él.
        #[source]
        causa: crate::rustic::Error,
        /// El mismo fallo, ya recortado y sin credenciales, para contarlo: la red de secretos
        /// quita los **valores** de las claves, no lo que el texto diga de una ruta del host.
        motivo: String,
    },

    /// La restauración **del contenedor** falló: la orden del cliente de la base salió con un
    /// código distinto de cero, o el transporte no llegó a darla. **Corresponde a un `500`**.
    ///
    /// Es el desenlace que se guarda en `restore_executions.error_message` y no llega a ninguna
    /// respuesta: la restauración corre en segundo plano, así que quien lee este error es el
    /// registro y la fila del §3. El [`motivo`](Self::Restauracion::motivo) viene de
    /// [`crate::engine::restauracion::ejecutar_las_ordenes`], que ya lo pasó por la red de
    /// credenciales —el `stderr` del cliente de la base— y lo recortó.
    ///
    /// Lo que lo separa de [`Repositorio`](Self::Repositorio) es de qué lado falló: aquí el
    /// repositorio entregó el volcado y el que no lo aceptó fue el contenedor, y por eso el motivo
    /// nombra la orden y el contenedor. Cuando el que falla es el repositorio, el motivo del
    /// transporte llega con el suyo dentro —ver [`crate::docker::Error::EntradaNoEntregada`]— y el
    /// desenlace sigue siendo éste, porque el trabajo que lo pidió es el de la restauración.
    #[error("{motivo}")]
    Restauracion {
        /// Qué dijo el contenedor o el transporte, ya depurado y recortado.
        motivo: String,
    },

    /// El trabajo que hablaba con la biblioteca no llegó a término. **Corresponde a un `500`**.
    ///
    /// Es el desenlace de un hilo de trabajo que se cayó —un pánico dentro de la biblioteca, que
    /// llama a código que este crate no controla— o de uno que no volvió, y por eso el motivo es
    /// propio y **no** el del pánico: el texto de un pánico es arbitrario y acabaría publicado en
    /// una respuesta, que es justo el sitio del que este módulo lo mantiene fuera. El detalle se
    /// queda en el registro, donde nadie lo lee desde fuera.
    #[error("el trabajo contra el repositorio se ha interrumpido: {motivo}")]
    TrabajoInterrumpido {
        /// Qué se sabe del hilo: que se cayó con un pánico, o que no llegó a término.
        motivo: String,
    },

    /// El motor se ha montado fuera de un runtime de Tokio.
    ///
    /// No es un desenlace de una petición, es un fallo de montaje: el motor captura el
    /// [`Handle`] del runtime de la aplicación al construirse, porque es el que usa para escribir
    /// en la base desde sus hilos. Sin runtime no hay dónde entrar, y falla al arrancar en vez de
    /// arrancar y morir en la primera copia.
    #[error(
        "el motor tiene que montarse desde dentro de un runtime de Tokio: es el runtime en el que \
         sus hilos escriben en la base"
    )]
    SinRuntime,
}

/// Lo que comparten el motor y sus hilos de trabajo.
///
/// Es un valor **de la fachada** y no del trabajo: el motor monta un `Contexto` y cada trabajador
/// recibe un clon —que clona `Arc`s, no datos—. De ahí que `Ajustes` y `Dependencias` sean
/// clonables.
#[derive(Clone)]
pub(crate) struct Contexto {
    /// Con qué habla el motor y cuánto espera.
    pub(crate) ajustes: Ajustes,
    /// Las cuatro capas de acceso.
    pub(crate) dependencias: Dependencias,
    /// El runtime de la aplicación, entrada explícita para la base.
    pub(crate) runtime: Handle,
    /// El hub por el que salen los avisos.
    pub(crate) hub: Hub,
}

impl Contexto {
    /// Entra en el runtime de la aplicación, **solo para la base**.
    ///
    /// Es el único sitio del motor que hace esto, y es lo que permite que el hilo del trabajador
    /// no tenga runtime propio sin dejar de poder escribir en `job_executions`: la llamada es
    /// explícita —se ve en cada sitio que la usa— y el futuro que se le pasa no puede llevar dentro
    /// una llamada a Rustic, que es lo que entraría en pánico.
    pub(crate) fn en_la_base<T>(&self, futuro: impl std::future::Future<Output = T>) -> T {
        self.runtime.block_on(futuro)
    }
}

/// El motor: la cola, los trabajadores y el hub de eventos.
///
/// Se monta una vez con [`Motor::arrancar`] y se comparte —es `Clone`, y clonarlo no copia ni una
/// cola ni un evento—. Es lo que el `AppState` lleva dentro, y lo que las tres rutas nuevas del §5
/// usan.
#[derive(Clone)]
pub struct Motor(Arc<Interior>);

/// Lo que hay dentro del motor.
struct Interior {
    /// Lo que comparten el motor y sus hilos.
    contexto: Contexto,
    /// La cola: los buzones y la parada.
    cola: Cola,
    /// Los destinos con una restauración **en vuelo**, para que dos no escriban a la vez.
    ///
    /// Vive aquí y no en [`Contexto`] porque los trabajadores de la cola no la necesitan: quien la
    /// consulta y la escribe son las peticiones de la capa HTTP y los trabajos de restauración que
    /// ellas lanzan, y todos pasan por el mismo `Motor`. Es un `Arc` porque cada reserva que se
    /// entrega —una por restauración— tiene que poder soltarse sola cuando su trabajo acabe, aunque
    /// para entonces el `Motor` de la petición ya no exista.
    ///
    /// El candado es de `std` y no de Tokio, como el del hub: lo que protege son dos operaciones
    /// instantáneas —mirar y anotar, o tachar— y **nunca se mantiene a lo largo de un `await`**;
    /// [`Motor::reservar_el_destino`] es síncrona justo por eso.
    ///
    /// Lo que guarda dentro son claves ya **normalizadas** ([`normalizar_el_destino`]) y no el texto
    /// crudo del `host_path`: es lo que hace que dos formas de escribir el mismo directorio sean el
    /// mismo destino y no dos.
    destinos_reservados: Arc<Mutex<HashSet<String>>>,
}

/// Las tres fases del aviso de restauración, que son **contrato** del §5.
///
/// No son literales sueltos porque no son texto de este módulo: el contrato del change fija tres
/// palabras —`restaurando` al arrancar y `terminada` o `fallida` al cerrar— y el panel decide qué
/// pintar con ellas. Vivir en constantes con nombre es lo que hace que el vocabulario se lea de una
/// vez y que el día que se añada una fase no haya que buscarla por el fichero.
///
/// Son constantes **privadas**: lo que la capa HTTP publica es el evento, y quien decide el nombre
/// `restore_progress` es ella. Aquí solo se decide qué fase lleva dentro.
const FASE_RESTAURANDO: &str = "restaurando";
/// La fase del cierre cuando la restauración llegó a término.
const FASE_TERMINADA: &str = "terminada";
/// La fase del cierre cuando la restauración **no** llegó a término, por la razón que sea.
const FASE_FALLIDA: &str = "fallida";

impl Motor {
    /// Monta el motor.
    ///
    /// Es **síncrona** porque no habla con nada: captura el [`Handle`] del runtime de la aplicación
    /// —de ahí que tenga que llamarse desde dentro de uno— y deja la cola vacía. Un socket de
    /// Docker que no responda **no** impide montarlo: eso lo averigua [`Motor::comprobar`], y quien
    /// decide qué hacer con ese fallo es el arranque.
    ///
    /// # Errors
    ///
    /// Devuelve [`ErrorDelMotor::SinRuntime`] si no se llama desde dentro de un runtime de Tokio.
    pub fn arrancar(ajustes: Ajustes, dependencias: Dependencias) -> Result<Self, ErrorDelMotor> {
        let runtime = Handle::try_current().map_err(|_| ErrorDelMotor::SinRuntime)?;

        Ok(Self(Arc::new(Interior {
            contexto: Contexto {
                ajustes,
                dependencias,
                runtime,
                hub: Hub::nuevo(),
            },
            cola: Cola::nueva(),
            destinos_reservados: Arc::new(Mutex::new(HashSet::new())),
        })))
    }

    /// Comprueba que el motor de contenedores contesta.
    ///
    /// La llama el arranque para **avisar**, nunca para decidir si el proceso arranca: un socket que
    /// no responde no puede dejar al operador sin la herramienta con la que diagnosticarlo, y el
    /// fallo aparece donde importa —al ejecutar una copia—. Aquí se comprueba para poder decirlo
    /// antes.
    ///
    /// # Errors
    ///
    /// Devuelve [`ErrorDelMotor::MotorDeContenedores`] si el socket no se puede abrir o el motor no
    /// contesta al latido.
    pub async fn comprobar(&self) -> Result<(), ErrorDelMotor> {
        let ajustes = &self.0.contexto.ajustes;

        let cliente = Cliente::conectar(&ajustes.socket_del_motor, ajustes.espera_del_motor)
            .map_err(|causa| ErrorDelMotor::MotorDeContenedores { causa })?;

        cliente
            .comprobar()
            .await
            .map_err(|causa| ErrorDelMotor::MotorDeContenedores { causa })
    }

    /// Encola una ejecución de esa tarea.
    ///
    /// En el cable esto es un `202`: la ejecución está **aceptada**, no hecha. La fila nace en
    /// `pending` —la crea la guarda atómica de la capa de acceso, que es también el `409`— y entra
    /// en la cola de **su** repositorio, donde como mucho sesenta y cuatro ejecuciones esperan
    /// detrás de la que esté corriendo.
    ///
    /// Y una cosa que no se hace: **no se mira `enabled`**. Una tarea desactivada sí se dispara a
    /// mano, porque `enabled` gobierna al planificador por `cron_expression` y el disparo manual es
    /// otra cosa; este motor no tiene planificador todavía, y negarse aquí sin poder explicar por
    /// qué sería peor que ejecutarla.
    ///
    /// # Errors
    ///
    /// [`ErrorDelMotor::TareaQueNoEsta`] (404), [`ErrorDelMotor::YaTieneUnaEnCurso`] (409),
    /// [`ErrorDelMotor::MotorParado`] (503), [`ErrorDelMotor::ColaDelRepositorioLlena`] (503),
    /// [`ErrorDelMotor::SinQuienAtiendaLaCola`] (503) y [`ErrorDelMotor::Base`] (500).
    pub async fn encolar(&self, job_id: Uuid) -> Result<Ejecucion, ErrorDelMotor> {
        let interior = &self.0;
        let dependencias = &interior.contexto.dependencias;

        // La comprobación de antes: la de dentro del candado es la que decide, pero ésta evita
        // crear una fila para nada.
        if interior.cola.esta_parada() {
            return Err(ErrorDelMotor::MotorParado);
        }

        // La tarea, primero: sin ella no hay repositorio al que apuntar la cola, y su ausencia es
        // un `404` y no un fallo de base.
        let tarea = dependencias
            .tareas
            .leer(&job_id)
            .await
            .map_err(|causa| ErrorDelMotor::Base { causa })?
            .ok_or(ErrorDelMotor::TareaQueNoEsta { job_id })?;

        // La guarda atómica: o la fila entra y se devuelve, o no entra y esto es un `409`.
        let ejecucion = dependencias
            .ejecuciones
            .alta_si_no_hay_ninguna_en_curso(AltaDeEjecucion {
                job_id,
                status: Estado::Pending,
                snapshot_id: None,
                bytes_processed: 0,
                bytes_uploaded: 0,
                error_message: None,
                started_at: OffsetDateTime::now_utc(),
                finished_at: None,
            })
            .await
            .map_err(|causa| ErrorDelMotor::Base { causa })?
            .ok_or(ErrorDelMotor::YaTieneUnaEnCurso { job_id })?;

        // Y si el encolado falla **después** de crear la fila, esa fila se marca como fallida antes
        // de devolver el error: una fila `pending` que nadie va a ejecutar se queda mintiendo en el
        // panel —«en cola»— para siempre, y eso es peor que un error que se ve.
        //
        // Se marca **desde aquí**, que es un `async` dentro del runtime, con la entrada
        // `async` de esa marca: la otra —`ejecucion::fallar_si_no_termino`— entra en el runtime con
        // un `block_on`, y un `block_on` dentro de un runtime es un **pánico**. Este camino lo
        // cometía, y lo delató la prueba de la cola llena: el hallazgo era que esta mitad no estaba
        // medida, y al medirla apareció el pánico.
        if let Err(causa) =
            interior
                .cola
                .encolar(&interior.contexto, tarea.repository_id, ejecucion.clone())
        {
            ejecucion::marcar_fallida_si_no_termino(
                &interior.contexto,
                &ejecucion.id,
                &causa.to_string(),
            )
            .await;

            return Err(causa);
        }

        // Y el nacimiento se publica: la ejecución acaba de existir y el panel tiene que poder
        // pintarla en `pending` sin esperar al primer avance.
        interior.contexto.hub.publicar(EventoDeEjecucion {
            execution_id: ejecucion.id,
            job_id,
            status: Estado::Pending,
            bytes_processed: 0,
            bytes_uploaded: 0,
            percentage: None,
            current_action: ejecucion::ACCION_ENCOLADA.to_owned(),
        });

        Ok(ejecucion)
    }

    /// El hub, para que la capa HTTP se suscriba.
    pub fn hub(&self) -> &Hub {
        &self.0.contexto.hub
    }

    /// Cierra el hub de eventos: los clientes del stream terminan y publicar deja de hacer nada.
    ///
    /// Es la pieza que el apagado ordenado necesita **antes** de esperar a que las conexiones
    /// terminen, y sin ella el apagado se cuelga: el cierre del servidor no resuelve hasta que
    /// todas las conexiones lo hacen, y el cuerpo de un `text/event-stream` solo termina cuando el
    /// canal se cierra. Con un cliente conectado —la única configuración en la que el stream sirve
    /// para algo— el proceso se quedaría vivo hasta que el período de gracia del orquestador lo
    /// matara con `SIGKILL`, con el hilo de trabajo a mitad de archivar. El porqué entero, y el
    /// límite que esto no cubre, en [`Hub::cerrar`].
    ///
    /// Es **idempotente** y no falla nunca: se puede llamar más de una vez y desde cualquier hilo.
    /// Y no confundirla con [`Motor::parar`]: esta cierra **el reparto** —lo que deja terminar a los
    /// clientes conectados— y aquella cierra **el trabajo**. El orden entre las dos lo decide quien
    /// apaga el proceso, y es el del `main`: primero el hub, para que el servidor pueda terminar de
    /// apagarse, y después la parada del motor.
    pub fn cerrar_el_hub(&self) {
        self.0.contexto.hub.cerrar();
    }

    /// Abre —o inicializa— el repositorio que describe el acceso, **fuera del runtime de Tokio**.
    ///
    /// Es la mitad del motor que no es una copia: la necesita la capa HTTP para el alta, que no
    /// puede limitarse a escribir la fila —una fila que describe un cubo que no existe hace que el
    /// panel lo liste como disponible y que el fallo aparezca mucho después, en la primera copia—.
    /// Devuelve cuál de los dos desenlaces ocurrió —[`PuestaEnMarcha::Inicializado`] o
    /// [`PuestaEnMarcha::Conectado`]—, que es lo que la respuesta del alta tiene que decir.
    ///
    /// El repositorio se **suelta** antes de volver: es de un solo uso práctico —reutilizarlo entre
    /// dos copias desactiva la deduplicación, ver `src/rustic/repositorio.rs`— y aquí no hay copia
    /// ninguna que hacer, solo saber si el destino contesta.
    ///
    /// Un acceso que no abre **no deja nada a medias**: no hay fila que escribir ni estado que
    /// tocar —quien decide no escribir la fila es el manejador, y esta operación no la escribe—, y
    /// lo que sale es el error. Ver [`ErrorDelMotor::Repositorio`] para a quién le toca elegir el
    /// código HTTP.
    ///
    /// # Errors
    ///
    /// [`ErrorDelMotor::Repositorio`] si la biblioteca no puede poner el repositorio en marcha, y
    /// [`ErrorDelMotor::TrabajoInterrumpido`] si el hilo que lo intentaba se cae.
    ///
    /// # Por qué el abridor es el inyectado y no el de S3 a pelo
    ///
    /// El abridor vive en [`Ajustes`] y es un puntero a función precisamente para esto: un
    /// despliegue abre el de S3 y una prueba abre uno **local**, así que la capa HTTP no necesita
    /// saber cómo se abre un repositorio y no puede equivocarse de runtime —la restricción del
    /// pánico se cumple aquí, una vez, y no en cada manejador—.
    pub async fn abrir_repositorio(&self, acceso: Acceso) -> Result<PuestaEnMarcha, ErrorDelMotor> {
        let abridor = self.0.contexto.ajustes.abridor;

        fuera_del_runtime(move || {
            abridor(&acceso, BarrasDelMotor::mudas())
                // El repositorio se suelta aquí: lo único que se quería saber es cuál de los dos
                // desenlaces ocurrió.
                .map(|(_repositorio, puesta)| puesta)
                .map_err(|causa| ErrorDelMotor::Repositorio {
                    motivo: ejecucion::motivo_de_la_biblioteca(&causa.to_string(), &acceso),
                    causa,
                })
        })
        .await
    }

    /// Lista las instantáneas de un repositorio, **fuera del runtime de Tokio**, hasta `ventana`.
    ///
    /// Es la otra mitad que la capa HTTP necesita: el explorador del §5 lee los snapshots del
    /// **repositorio** y no del histórico de ejecuciones, así que hay que abrirlo y preguntarle.
    /// Abre y suelta el repositorio en la misma llamada —por lo mismo que
    /// [`Motor::abrir_repositorio`]— y devuelve las instantáneas ya resueltas y ordenadas de la más
    /// reciente a la más antigua.
    ///
    /// Un repositorio que todavía no existe se **inicializa**, que es lo que hace la puesta en
    /// marcha: la lista sale vacía y el destino queda creado. No se rechaza aquí porque quien
    /// pregunta por un repositorio que no está tiene que mirar su fila, no el almacenamiento, y eso
    /// el motor no lo sabe: el `404` del listado lo decide el manejador, que es quien puede leer la
    /// fila.
    ///
    /// # Errors
    ///
    /// Las mismas dos que [`Motor::abrir_repositorio`], más el fallo del listado en sí —que la
    /// biblioteca no pueda leer los identificadores o resolver una instantánea—, que llega también
    /// como [`ErrorDelMotor::Repositorio`].
    pub async fn listar_snapshots(
        &self,
        acceso: Acceso,
        ventana: usize,
    ) -> Result<Vec<ResumenDeSnapshot>, ErrorDelMotor> {
        let abridor = self.0.contexto.ajustes.abridor;

        fuera_del_runtime(move || {
            let (repositorio, _puesta) =
                abridor(&acceso, BarrasDelMotor::mudas()).map_err(|causa| {
                    ErrorDelMotor::Repositorio {
                        motivo: ejecucion::motivo_de_la_biblioteca(&causa.to_string(), &acceso),
                        causa,
                    }
                })?;

            repositorio
                .listar_snapshots(ventana)
                .map_err(|causa| ErrorDelMotor::Repositorio {
                    motivo: ejecucion::motivo_de_la_biblioteca(&causa.to_string(), &acceso),
                    causa,
                })
        })
        .await
    }

    /// Reserva un destino para una restauración, o dice que ya hay una en vuelo.
    ///
    /// Es la guarda de exclusión por **destino**: dos `POST` de restauración sobre el mismo
    /// snapshot —o sobre dos instantáneas distintas del mismo target— escribirían a la vez en el
    /// mismo `host_path`, y lo que quedaría en el disco sería la mezcla de las dos. La reserva se
    /// toma **antes de contestar**, que es lo que hace que la segunda petición reciba un `409` en
    /// vez de un `200` que promete un trabajo condenado a pelearse con el primero.
    ///
    /// # La reserva se suelta al soltarse
    ///
    /// Lo que devuelve es una guarda —[`ReservaDeRestauracion`]— que tacha el destino **al soltarse**
    /// y no cuando alguien se acuerde de hacerlo: quien la recibe la lleva consigo mientras dure el
    /// trabajo, así que el destino vuelve a estar libre tanto si la restauración acaba bien como si
    /// acaba mal, y también si el trabajo se desmonta por un camino que nadie escribió —un pánico
    /// dentro del futuro que la lleva, o una cancelación—, porque Rust suelta los locales de un
    /// futuro al desmontarlo. Sin la guarda, un desenlace inesperado dejaría el destino reservado
    /// **para siempre** y ese target no volvería a restaurarse hasta reiniciar el proceso.
    ///
    /// Es **síncrona** a propósito: mira y anota bajo un candado de `std`, y mantenerlo a lo largo
    /// de un `await` sería el camino de un interbloqueo. No habla con el repositorio ni con la base
    /// —de ahí que no sea `async`—: lo único que decide es quién tiene el destino.
    ///
    /// Lo que se mira y se anota no es el texto que llega, sino su forma normalizada
    /// ([`normalizar_el_destino`]): dos maneras de escribir el mismo directorio —`/data` y
    /// `/data/`— son **una sola clave**, y una segunda restauración sobre él no entra.
    ///
    /// # Errors
    ///
    /// Devuelve [`ErrorDelMotor::RestauracionEnCurso`] —un `409`— si ese destino ya está reservado.
    pub fn reservar_el_destino(
        &self,
        destino: &str,
    ) -> Result<ReservaDeRestauracion, ErrorDelMotor> {
        // La clave con la que se mira y se anota es la **normalizada**, no lo que llegue escrito:
        // `"/data"` y `"/data/"` son el mismo directorio y tienen que ser la misma clave, o dos
        // restauraciones escribirían a la vez en el mismo sitio. El `Drop` de la guarda tacha con
        // esta misma forma —la lleva dentro—, así que lo que se libera es lo que se reservó.
        let clave = normalizar_el_destino(destino);
        let mut reservados = candado(&self.0.destinos_reservados);

        if !reservados.insert(clave.clone()) {
            return Err(ErrorDelMotor::RestauracionEnCurso {
                destino: destino.to_owned(),
            });
        }

        Ok(ReservaDeRestauracion {
            destino: clave,
            reservados: Arc::clone(&self.0.destinos_reservados),
        })
    }

    /// Dice si el repositorio tiene esa instantánea, **sin restaurarla**, fuera del runtime.
    ///
    /// Es la lectura que hace que el `404` de la restauración sea de verdad: el manejador resuelve
    /// la fila del repositorio y el vínculo de la instantánea con su target sin abrir el
    /// almacenamiento, así que sin esta pregunta el `200` prometería un trabajo contra una copia que
    /// puede no estar —una que purgó la retención, un repositorio recreado en el mismo destino— y el
    /// fallo solo aparecería en el registro.
    ///
    /// # Por qué es una segunda apertura, y por qué se acepta
    ///
    /// Abre y suelta el repositorio en la misma llamada —igual que [`Motor::listar_snapshots`]—, y
    /// eso significa que una restauración abre el almacenamiento **dos veces**: una para preguntar
    /// si la copia está y otra para restaurarla. Es el precio de que el `404` exista: saber si la
    /// instantánea está exige preguntárselo al repositorio, y no hay forma de saberlo por la fila
    /// —`job_executions.snapshot_id` dice cuál se copió, no si sigue ahí—. Una sola apertura
    /// obligaría a decidir el código de la respuesta **después** de contestar, que es exactamente el
    /// `200` optimista que el contrato prohíbe.
    ///
    /// Quien llame dos veces seguidas —la comprobación y la restauración— paga dos derivaciones de
    /// clave. Medido: `init` y `open` tardan 6,4 s cada uno en la compilación de pruebas, así que en
    /// el peor caso el `200` de una restauración llega unos segundos más tarde. Lo que compra es que
    /// ese `200` hable de una copia que existe.
    ///
    /// # Errors
    ///
    /// Las mismas dos que [`Motor::abrir_repositorio`], más el fallo de la lectura en sí —que el
    /// repositorio no se deje listar—, que llega también como [`ErrorDelMotor::Repositorio`].
    pub async fn tiene_la_instantanea(
        &self,
        acceso: Acceso,
        instantanea: &str,
    ) -> Result<bool, ErrorDelMotor> {
        let abridor = self.0.contexto.ajustes.abridor;
        // El trabajo viaja a un hilo propio y tiene que ser `'static`: se lleva una copia del
        // identificador en vez de prestar el de quien pregunta.
        let instantanea = instantanea.to_owned();

        fuera_del_runtime(move || {
            let (repositorio, _puesta) =
                abridor(&acceso, BarrasDelMotor::mudas()).map_err(|causa| {
                    ErrorDelMotor::Repositorio {
                        motivo: ejecucion::motivo_de_la_biblioteca(&causa.to_string(), &acceso),
                        causa,
                    }
                })?;

            repositorio
                .tiene_la_instantanea(&instantanea)
                .map_err(|causa| ErrorDelMotor::Repositorio {
                    motivo: ejecucion::motivo_de_la_biblioteca(&causa.to_string(), &acceso),
                    causa,
                })
        })
        .await
    }

    /// Restaura `instantanea` sobre el `objetivo`, eligiendo el camino por su **tipo**.
    ///
    /// Es la operación que cierra las tres que la capa HTTP necesita y la única que **escribe**:
    /// abre el repositorio con el abridor inyectado y lleva el volcado a su sitio —al
    /// `host_path` de un directorio, con el origen local de la biblioteca, o al contenedor de una
    /// base, por el `stdin` del `exec`—. El [`Repositorio`] se **consume** en la llamada
    /// —restaurar lo deja en un estado intermedio, no reutilizable—, así que no se suelta a mano ni
    /// se guarda para nada más.
    ///
    /// # La rama la decide el tipo, y solo por el tipo
    ///
    /// [`restauracion::comando`] traduce el objetivo a un [`CaminoDeRestauracion`] y es la **única**
    /// boca que decide: un `directory` sale por [`CaminoDeRestauracion::PorDirectorio`] y una base
    /// —`mariadb`, `postgresql` o `sqlite`— por [`CaminoDeRestauracion::PorContenedor`]. Aquí no
    /// hay ninguna segunda comprobación del tipo, y no por descuido: dos decisiones sobre lo mismo
    /// son dos sitios que pueden separarse, y el día que se añada un tipo el `match` del módulo de
    /// la restauración avisa mientras que una comprobación suelta no.
    ///
    /// # El ciclo de vida de la fila lo escribe este método
    ///
    /// Al empezar, la fila —que dejó en `pending` quien lanzó la restauración— pasa a `running`; al
    /// terminar pasa a `success` con `finished_at`, o a `failed` con `error_message` y
    /// `finished_at`. La escritura final se **intenta también** cuando el trabajo falla: es lo que
    /// hace que un fallo no se quede en `running` mintiendo en el histórico **mientras la escritura
    /// llegue**. El límite está escrito donde toca, en
    /// [`cerrar_la_restauracion`](Self::cerrar_la_restauracion): lo que estas dos funciones ignoran
    /// —con su aviso, y sin detener el trabajo— es una fila que no está y una lectura que falle;
    /// una **escritura** que falle se registra igual y deja la fila como estaba, así que la garantía
    /// es que el desenlace se intenta, no que llega.
    ///
    /// Si no hay fila con ese identificador, el trabajo **sigue igual** y solo se registra el aviso:
    /// quien lanza una restauración la crea antes de contestar, así que su ausencia es un fallo de
    /// otro, y dejar la restauración a medias por una contabilidad que no existe sería peor que el
    /// aviso.
    ///
    /// # Por qué el primer aviso se publica **antes** de tocar el repositorio
    ///
    /// El stream SSE del §5 es **uno solo y global**, y `restore_progress` tiene que ser el
    /// **primer** evento que ve un cliente que acaba de suscribirse: si el aviso de arranque se
    /// publicara al terminar, el primer fotograma llegaría cuando ya no hay nada que pintar y el
    /// panel no tendría forma de saber que la restauración empezó. Por eso ese aviso sale aquí, en
    /// el **cuerpo asíncrono**, antes de programar el trabajo: así no depende de cuándo el conjunto
    /// de bloqueo decida despachar el cierre y llega en cuanto el manejador cede el control, que es
    /// lo antes que se puede.
    ///
    /// # Por qué el aviso de cierre también vive aquí, y no dentro del trabajo
    ///
    /// El de cierre se publica en el cuerpo asíncrono, **sobre el desenlace del `.await`**: `Ok` es
    /// `terminada` y `Err` es `fallida`. Y no dentro del cierre que corre en el hilo de bloqueo
    /// porque ahí se perdería justo el desenlace que más importa: un pánico de la biblioteca —que
    /// llama a código que este crate no controla— convierte el trabajo en un [`JoinError`], y el
    /// cierre que se le pasó a [`fuera_del_runtime`] **no llega a ejecutarse siquiera**. El cierre
    /// no se publicaría, el panel se quedaría en «restaurando» para siempre y nadie sabría que
    /// aquello terminó. Aquí, en cambio, un solo camino cubre los tres desenlaces que existen:
    /// éxito, error y desmontaje del hilo de trabajo.
    ///
    /// # Hasta dónde llega ese «un solo camino»
    ///
    /// Cubre el pánico del **hilo de trabajo**, que llega como [`JoinError`] por el `.await` y es un
    /// desenlace más. **No** cubre un pánico del propio cuerpo asíncrono de este método —ni el de la
    /// espera del `.await`—: ahí el cuerpo se deshace antes de llegar a publicar, y no hay desde
    /// dónde hacerlo. El límite no se puede cerrar sin perder algo: publicar el cierre desde el hilo
    /// de bloqueo cubriría ese caso y perdería el del pánico del hilo —y con él el de la biblioteca
    /// entera, que llama a código que este crate no controla y es la que más falla—, así que se
    /// elige el que más importa y el límite se deja escrito.
    ///
    /// Los contadores del primer aviso van a `0` —los totales no se conocen hasta que la
    /// biblioteca recorre el árbol, y el contrato del evento no admite un «aún no lo sé» más que
    /// como un cero— y los dos avisos llevan las fases de [`FASE_RESTAURANDO`] y del cierre, que son
    /// frases **constantes** del motor, no cifras.
    ///
    /// [`JoinError`]: tokio::task::JoinError
    ///
    /// # Errors
    ///
    /// Las mismas que [`Motor::abrir_repositorio`], más el fallo de la restauración en sí, que
    /// llega como [`ErrorDelMotor::Repositorio`] cuando el que no responde es el repositorio y como
    /// [`ErrorDelMotor::Restauracion`] cuando el que falla es el contenedor. Un objetivo
    /// incoherente —al que le falta un campo de su tipo— llega también como
    /// [`ErrorDelMotor::Restauracion`]. El aviso de cierre se publica tanto si la restauración
    /// termina bien (`terminada`) como si falla (`fallida`), y quien pregunta por el desenlace lo
    /// tiene igualmente en el `Result`.
    pub async fn restaurar(
        &self,
        acceso: Acceso,
        instantanea: String,
        objetivo: ObjetivoDeRestauracion,
        execution_id: Uuid,
    ) -> Result<(), ErrorDelMotor> {
        // El camino, **antes** de tocar nada: un objetivo incoherente no llega al punto de anunciar
        // un arranque que no va a ocurrir. La fila sí se cierra —a `failed`—, porque el trabajo se
        // tomó y esa es la verdad.
        let camino = match restauracion::comando(&objetivo) {
            Ok(camino) => camino,
            Err(causa) => {
                let motivo = recortar(&causa.to_string());
                self.cerrar_la_restauracion(execution_id, Some(&motivo))
                    .await;
                return Err(ErrorDelMotor::Restauracion { motivo });
            }
        };

        let hub = self.0.contexto.hub.clone();

        // El aviso de arranque, con los cuatro contadores a cero.
        hub.publicar_restauracion(evento_de_restauracion(
            execution_id,
            &instantanea,
            FASE_RESTAURANDO,
        ));

        // La fila, de `pending` a `running`: a partir de aquí una restauración en curso se ve en el
        // histórico.
        self.marcar_en_curso(execution_id).await;

        let desenlace = match camino {
            CaminoDeRestauracion::PorDirectorio { ruta } => {
                self.restaurar_en_el_directorio(acceso, &instantanea, ruta)
                    .await
            }
            CaminoDeRestauracion::PorContenedor {
                contenedor,
                ordenes,
            } => {
                self.restaurar_en_el_contenedor(acceso, &instantanea, contenedor, ordenes)
                    .await
            }
        };

        // Confirmación de cierre: un cliente que se suscribiera tarde todavía ve el desenlace de la
        // restauración, tanto si llegó a término como si no —y el `Err` cubre también el pánico del
        // hilo de trabajo, que llega como `JoinError` por el `.await` y es el desenlace que se
        // perdería publicando desde dentro—. Lo que este camino **no** cubre es un pánico de este
        // propio cuerpo asíncrono: se desharía antes de llegar aquí. El límite, entero, en el
        // rustdoc de arriba.
        let fase = if desenlace.is_ok() {
            FASE_TERMINADA
        } else {
            FASE_FALLIDA
        };

        hub.publicar_restauracion(evento_de_restauracion(execution_id, &instantanea, fase));

        // El desenlace, a la fila: `success` con su final, o `failed` con el motivo y su final.
        self.cerrar_la_restauracion(
            execution_id,
            desenlace
                .as_ref()
                .err()
                .map(|causa| recortar(&causa.to_string()))
                .as_deref(),
        )
        .await;

        desenlace
    }

    /// Restaura la instantánea en un directorio del sistema de ficheros, **fuera del runtime**.
    ///
    /// Es la rama de un target `directory`: abre el repositorio con el abridor inyectado y le pide
    /// que restaure la instantánea en la ruta del host. El trabajo va a un hilo de bloqueo por la
    /// misma razón de siempre —la biblioteca bloquea y construye su propio runtime— y por eso
    /// delega en [`fuera_del_runtime`].
    async fn restaurar_en_el_directorio(
        &self,
        acceso: Acceso,
        instantanea: &str,
        ruta: PathBuf,
    ) -> Result<(), ErrorDelMotor> {
        let abridor = self.0.contexto.ajustes.abridor;
        let instantanea = instantanea.to_owned();

        // La ruta canónica puede no ser UTF-8 —un nombre de fichero de Linux no tiene por qué
        // serlo—, y la biblioteca quiere un `&str`: quedarse con la pérdida es preferible a no
        // poder restaurar. El camino se compone con el mismo `PathBuf` que ya validó `comando`.
        let destino_local = ruta.to_string_lossy().into_owned();

        fuera_del_runtime(move || {
            let (repositorio, _puesta) =
                abridor(&acceso, BarrasDelMotor::mudas()).map_err(|causa| {
                    ErrorDelMotor::Repositorio {
                        motivo: ejecucion::motivo_de_la_biblioteca(&causa.to_string(), &acceso),
                        causa,
                    }
                })?;

            repositorio
                .restaurar_snapshot(&instantanea, &destino_local)
                .map_err(|causa| ErrorDelMotor::Repositorio {
                    motivo: ejecucion::motivo_de_la_biblioteca(&causa.to_string(), &acceso),
                    causa,
                })
        })
        .await
    }

    /// Restaura la instantánea en el contenedor del objetivo, por el `stdin` de sus `exec`.
    ///
    /// Es la rama de un target de base: el volcado sale del repositorio en un hilo de bloqueo
    /// —la biblioteca **escribe** con un `Write` bloqueante— y llega al `stdin` del `exec` por el
    /// puente acotado de [`tunel_de_restauracion`], sin pasar por el disco. Las dos mitades corren a
    /// la vez: parar el volcado mientras nadie lee llenaría el canal y bloquearía el hilo; parar el
    /// `exec` mientras nadie escribe lo dejaría esperando un volcado que no llega.
    ///
    /// El orden de los dos desenlaces a la hora de decidir el fallo no es indiferente: manda el del
    /// **contenedor**, porque el `exec` es quien traduce el fallo de la biblioteca cuando el volcado
    /// no llegó entero —[`crate::docker::Error::EntradaNoEntregada`] lleva dentro el motivo del
    /// repositorio—, así que dejar ganar al del hilo sería perder el que trae el detalle. Solo si el
    /// contenedor no falló se mira lo que dijo el hilo, y ese caso es el de un `exec` que salió bien
    /// sobre un volcado que el repositorio no pudo producir entero.
    async fn restaurar_en_el_contenedor(
        &self,
        acceso: Acceso,
        instantanea: &str,
        contenedor: String,
        ordenes: Vec<Orden>,
    ) -> Result<(), ErrorDelMotor> {
        let abridor = self.0.contexto.ajustes.abridor;
        let socket = self.0.contexto.ajustes.socket_del_motor.clone();
        let espera = self.0.contexto.ajustes.espera_del_motor;

        // El puente, antes de lanzar nada: es lo que da el `stdin` al `exec` y el `Write` a la
        // biblioteca. El nombre del volcado es el de la instantánea, que es su única identidad
        // —dentro del árbol no tiene un nombre conocido—.
        let TunelDeRestauracion { escritor, lector } =
            tunel_de_restauracion(instantanea.to_owned(), CAPACIDAD_DEL_CANAL);

        // El volcado, a un hilo de bloqueo: la biblioteca escribe con un `Write` bloqueante y
        // construye su propio runtime, así que **no** puede correr dentro del de Tokio.
        let instantanea_del_volcado = instantanea.to_owned();
        let volcado = tokio::task::spawn_blocking(move || {
            volcar_el_repositorio(&abridor, &acceso, &instantanea_del_volcado, escritor)
        });

        // Y el `exec`, aquí, consumiendo ese `stdin`. El cliente se construye por restauración,
        // como en la extracción: no hay agrupador de conexiones compartido, y a cambio una
        // restauración no puede dejar al runtime de la aplicación esperando a nada.
        let desenlace = match Cliente::conectar(&socket, espera) {
            Ok(cliente) => {
                restauracion::ejecutar_las_ordenes(
                    &cliente,
                    &contenedor,
                    &ordenes,
                    lector,
                    INACTIVIDAD_MAXIMA,
                )
                .await
            }
            Err(causa) => Err(ErrorDelMotor::Restauracion {
                motivo: recortar(&causa.to_string()),
            }),
        };

        // La unión **después** de decidir: el `lector` ya se consumió —y se soltó— con la llamada
        // de arriba, así que el hilo no puede quedarse bloqueado escribiendo en un canal sin
        // lector.
        let vuelta = match volcado.await {
            Ok(vuelta) => vuelta,
            Err(caida) => Err(ErrorDelMotor::TrabajoInterrumpido {
                motivo: format!("el hilo que volcaba el repositorio no llegó a término: {caida}"),
            }),
        };

        // El del contenedor primero; el del repositorio, si el contenedor no falló.
        desenlace.and(vuelta)
    }

    /// Pasa la fila de la restauración a `running`.
    ///
    /// Si la fila no está o no se pudo leer, el trabajo sigue igual y solo se registra el aviso,
    /// porque quien lanza una restauración crea la fila antes de contestar y su ausencia es un fallo
    /// de otro. El `running` que no se pudo **escribir** también se registra y no detiene nada: el
    /// desenlace se intentará escribir igual, que es lo que el operador necesita. Lo que este método
    /// **no** promete es que la fila acabe en `running`: si la escritura falla, se queda en
    /// `pending` y solo el registro lo dice.
    async fn marcar_en_curso(&self, execution_id: Uuid) {
        let mut fila = match self
            .0
            .contexto
            .dependencias
            .restauraciones
            .leer(&execution_id)
            .await
        {
            Ok(Some(fila)) => fila,
            Ok(None) => {
                tracing::warn!(
                    %execution_id,
                    "no hay fila de restauración para esta ejecución: no se anota su ciclo de vida"
                );
                return;
            }
            Err(causa) => {
                tracing::error!(
                    %causa, %execution_id,
                    "no se ha podido leer la fila de la restauración"
                );
                return;
            }
        };

        fila.status = Estado::Running;

        if let Err(causa) = self
            .0
            .contexto
            .dependencias
            .restauraciones
            .actualizar(&fila)
            .await
        {
            tracing::error!(
                %causa, %execution_id,
                "no se ha podido pasar la restauración a `running`"
            );
        }
    }

    /// Escribe el desenlace de la restauración en la fila: `success`, o `failed` con su motivo.
    ///
    /// `motivo` es `None` cuando la restauración llegó a término y `Some` cuando falló, y es el
    /// mismo texto que viaja en el error del motor —ya depurado y recortado—, que es lo que el
    /// operador necesita para saber si fue la contraseña, el binario o el contenedor.
    ///
    /// Se lee la fila **otra vez** en vez de escribirla por el identificador: la capa de acceso
    /// escribe el estado, el motivo y el final, y las columnas que no toca —la instantánea, el
    /// target, el `started_at`— tienen que seguir ahí. Un `UPDATE` que las reescribiera con lo que
    /// esta función cree saber sería una segunda fuente de verdad. Si la fila no está —o no se pudo
    /// leer—, el fallo se registra y se sigue: el trabajo ya terminó y no hay nada que deshacer. Y
    /// lo mismo vale para la **escritura** del desenlace: si falla, se registra y se sigue —no hay a
    /// quién devolvérselo, el trabajo ya terminó—, con lo que la fila se queda como estaba, en
    /// `running`. Ese es el límite de la garantía del ciclo de vida: se cubre que el desenlace se
    /// **intente** escribir, no que llegue.
    async fn cerrar_la_restauracion(&self, execution_id: Uuid, motivo: Option<&str>) {
        let mut fila = match self
            .0
            .contexto
            .dependencias
            .restauraciones
            .leer(&execution_id)
            .await
        {
            Ok(Some(fila)) => fila,
            Ok(None) => {
                tracing::warn!(
                    %execution_id,
                    "no hay fila de restauración que cerrar para esta ejecución"
                );
                return;
            }
            Err(causa) => {
                tracing::error!(
                    %causa, %execution_id,
                    "no se ha podido leer la fila de la restauración para cerrarla"
                );
                return;
            }
        };

        fila.status = if motivo.is_some() {
            Estado::Failed
        } else {
            Estado::Success
        };
        fila.error_message = motivo.map(str::to_owned);
        fila.finished_at = Some(OffsetDateTime::now_utc());

        if let Err(causa) = self
            .0
            .contexto
            .dependencias
            .restauraciones
            .actualizar(&fila)
            .await
        {
            tracing::error!(
                %causa, %execution_id,
                "no se ha podido escribir el desenlace de la restauración"
            );
        }
    }

    /// Deja de aceptar trabajo, espera lo acotado y une los hilos.
    ///
    /// Es **idempotente** y hay que llamarla desde el cierre ordenado del proceso, **después** de
    /// que el servidor haya dejado de aceptar conexiones: lo que se está apagando es el motor, y
    /// una petición que llegue en medio encontrará el motor parado.
    ///
    /// La espera es la que diga `espera` —por defecto [`Ajustes::espera_de_parada`]— y es **una**
    /// para todos los trabajadores: el instante límite se calcula una vez y cada uno espera lo que
    /// le quede, porque un plazo por cabeza convertiría una parada de cinco segundos en cien con
    /// veinte repositorios en vuelo.
    pub fn parar(&self, espera: Duration) {
        self.0.cola.parar(espera);
    }
}

impl std::fmt::Debug for Motor {
    /// Publica lo que un diagnóstico necesita de un motor: si sigue aceptando trabajo y, si tiene
    /// hub, cuántos eventos retiene. Ni los ajustes ni las dependencias salen de aquí.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Motor")
            .field("parado", &self.0.cola.esta_parada())
            .field("capacidad_del_hub", &self.0.contexto.hub.capacidad())
            .finish_non_exhaustive()
    }
}

/// La reserva de un destino para una restauración: mientras viva, ese destino está ocupado.
///
/// La entrega [`Motor::reservar_el_destino`] y **no se puede construir a mano** —sus campos son
/// privados—, que es lo que hace que una reserva solo pueda venir de haber mirado el mapa: quien
/// tiene una guarda tiene el destino.
///
/// # Por qué se suelta con un `Drop`
///
/// Porque el trabajo que la retiene puede terminar por caminos que nadie escribe. Quien la recibe
/// es el trabajo —el manejador se la pasa al task que restaura—, y ese futuro puede desmontarse por
/// un pánico dentro de él o por una cancelación; con una liberación explícita, esos dos caminos
/// dejarían el destino reservado para siempre y el target no volvería a restaurarse hasta reiniciar
/// el proceso. Con el `Drop`, lo suelta el propio desmontaje del futuro, que es lo último que Rust
/// garantiza antes de dar el trabajo por terminado.
///
/// Y la liberación **no entra en pánico** si otro hilo se cayó con el candado tomado: lo que hay
/// dentro es un conjunto de rutas, tan válido como era, y un pánico aquí —durante el desmontaje de
/// otro— abortaría el proceso.
pub struct ReservaDeRestauracion {
    /// El destino reservado, en la forma **normalizada** con la que se anotó: es exactamente lo que
    /// el `Drop` tacha, y tiene que serlo o la reserva no se soltaría nunca.
    destino: String,
    /// El mapa del motor, compartido: tachar aquí es lo que libera el destino.
    reservados: Arc<Mutex<HashSet<String>>>,
}

impl Drop for ReservaDeRestauracion {
    fn drop(&mut self) {
        candado(&self.reservados).remove(&self.destino);
    }
}

/// Reduce a **una sola forma** las maneras de escribir un mismo destino, para que dos
/// restauraciones no puedan escribir a la vez sobre el mismo directorio por escribirlo distinto.
///
/// La guarda es un conjunto de claves de texto, así que sin esto `"/data"` y `"/data/"` —el mismo
/// directorio— serían dos claves y la segunda restauración entraría. Quien fija ese texto es el
/// alta de targets, que **no** lo normaliza: guarda el `host_path` tal cual lo escribió el usuario
/// (`src/db/entities/target.rs`), y de ahí que la normalización tenga que estar aquí.
///
/// Hay dos ramas porque [`std::fs::canonicalize`] —la forma buena: resuelve `.`, `..`, las barras
/// de más y los enlaces simbólicos— **solo habla de lo que existe**:
///
/// 1. **El directorio existe:** se devuelve su ruta canónica. Es el caso de una restauración sobre
///    un destino ya usado, y el único que se puede resolver de verdad.
/// 2. **Todavía no existe** —lo corriente, porque es la restauración quien lo crea—:
///    `canonicalize` falla y se cae a una normalización **textual mínima**, que recorta las barras
///    finales sobrantes y deja en pie la raíz. No se puede ser más estricto: `.`, `..` y los
///    enlaces simbólicos dependen del sistema de ficheros, no del texto, así que resolverlos a mano
///    daría una clave **distinta** de la canónica que se usará cuando el directorio ya exista, y
///    eso es peor que no normalizar. Lo que la rama compra es el caso corriente —una barra de más
///    al final— sin inventar nada.
///
/// Las dos ramas tienen que salir **igual** en los dos momentos de la guarda, al reservar y al
/// soltar: el `Drop` tacha exactamente lo que se anotó, así que ambos pasan por aquí.
///
/// La ruta canónica se pasa a texto con `to_string_lossy`: una ruta que no sea UTF-8 en Linux solo
/// puede venir de un nombre de fichero que no lo es, y quedarse con la pérdida es preferible a que
/// la reserva no se pueda hacer.
fn normalizar_el_destino(ruta: &str) -> String {
    if let Ok(canonica) = std::fs::canonicalize(ruta) {
        return canonica.to_string_lossy().into_owned();
    }

    let recortada = ruta.trim_end_matches('/');

    // `"/"` recortado queda vacío, y la raíz no es lo mismo que la cadena vacía: se conserva.
    if recortada.is_empty() && ruta.starts_with('/') {
        "/".to_owned()
    } else {
        recortada.to_owned()
    }
}

/// Toma un candado **sin entrar en pánico** si otro hilo se cayó teniéndolo tomado.
///
/// Es la misma decisión que la del candado del hub y la de `src/engine/ejecucion.rs`: el dato de
/// dentro sigue siendo tan válido como era —un conjunto de rutas—, y un motor no se cae por el
/// pánico de otro. Importa además en el `Drop` de [`ReservaDeRestauracion`]: un pánico durante el
/// desmontaje de otro trabajo **abortaría el proceso**.
fn candado<T>(candado: &Mutex<T>) -> MutexGuard<'_, T> {
    match candado.lock() {
        Ok(guardado) => guardado,
        Err(veneno) => veneno.into_inner(),
    }
}

/// Corre un trabajo **fuera del runtime** y devuelve su desenlace.
///
/// Es la pieza que sostiene las dos operaciones de la fachada, y su forma está **medida**, no
/// elegida. Se midieron los tres sitios donde se puede poner el trabajo, con una llamada real al
/// abridor de S3 contra un destino que no responde:
///
/// | dónde corre el trabajo | `Handle::try_current()` | qué pasa |
/// |---|---|---|
/// | `spawn_blocking` | `Ok` | error limpio, **sin pánico** |
/// | `std::thread` pelado | `Err` | error limpio |
/// | `block_in_place` en un runtime `multi_thread` | `Ok` | error limpio |
/// | `block_in_place` en un runtime `current_thread` | — | **pánico del runtime** |
///
/// Los dos primeros son seguros y el tercero **no** vale, y por eso el trabajo va a un hilo de
/// bloqueo: `spawn_blocking` saca el trabajo de los hilos que despachan tareas —el hilo del
/// conjunto de bloqueo no está dentro de un runtime, aunque el `Handle` siga a mano, que es lo que
/// confunde— y no cuesta un hilo por llamada. El cuarto camino queda descartado por lo que mide la
/// tabla: `#[tokio::test]` monta un runtime `current_thread` por defecto, así que un
/// [`tokio::task::block_in_place`] haría caer media suite por un pánico del runtime y no por el
/// código que se está probando.
///
/// Lo que **no** hace falta es que el trabajo devuelva un `Result`: devuelve lo que devuelva, y
/// quien convierte el fallo de la biblioteca en un error del motor es el cierre que se le pasa.
/// Lo que sí hace, y es la razón de que exista, es **no dejar que un pánico del hilo se
/// propague**: un `join` caído se convierte en un error con nombre propio, igual que el cierre del
/// trabajador (`src/engine/cola.rs`) convierte el pánico de una ejecución en una fila fallida.
async fn fuera_del_runtime<T, F>(trabajo: F) -> Result<T, ErrorDelMotor>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ErrorDelMotor> + Send + 'static,
{
    match tokio::task::spawn_blocking(trabajo).await {
        Ok(desenlace) => desenlace,
        Err(caida) => {
            // El detalle del pánico se queda en el registro, que es lo que hacen también el
            // cierre del trabajador y la parada: es arbitrario y no puede acabar publicado.
            tracing::error!(
                %caida,
                "el hilo que hablaba con el repositorio no ha llegado a término"
            );

            Err(ErrorDelMotor::TrabajoInterrumpido {
                motivo: if caida.is_panic() {
                    "el hilo que hablaba con el repositorio se ha caído con un pánico".to_owned()
                } else {
                    "el hilo que hablaba con el repositorio no ha llegado a término".to_owned()
                },
            })
        }
    }
}

/// El aviso de restauración para una fase, con los cuatro contadores a **cero**.
///
/// Los contadores van a cero y no se calculan: los totales no se conocen hasta que la biblioteca
/// recorre el árbol, y el contrato del evento del §5 no admite un «aún no lo sé» más que como un
/// cero. La fase entra como [`&str`] porque las tres son constantes con nombre —[`FASE_RESTAURANDO`]
/// y las de cierre— y quien las elige es el cuerpo de [`Motor::restaurar`].
fn evento_de_restauracion(
    execution_id: Uuid,
    instantanea: &str,
    fase: &str,
) -> EventoDeRestauracion {
    EventoDeRestauracion {
        execution_id,
        snapshot_id: instantanea.to_owned(),
        phase: fase.to_owned(),
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
    }
}

/// Vuelca la instantánea del repositorio en el puente, **en un hilo de bloqueo**.
///
/// Es la mitad productora de la restauración de una base, y la que traduce el desenlace de la
/// biblioteca al del motor: la apertura del repositorio y el volcado en sí fallan los dos como
/// [`ErrorDelMotor::Repositorio`], con el motivo de la biblioteca ya depurado.
///
/// Lo que **no** se puede olvidar: declarar el final del volcado en el puente. El camino bueno es
/// [`EscritorDelVolcado::terminar_bien`], que es la señal que distingue un fin legítimo de un canal
/// cerrado por la muerte del escritor; y el camino malo es [`EscritorDelVolcado::fallar`], que deja
/// el motivo en la ranura que el lector consulta —sin ella, un volcado a medias llegaría al `stdin`
/// como un fin de fichero limpio y la restauración parecería buena—.
fn volcar_el_repositorio(
    abridor: &AbridorDeRepositorios,
    acceso: &Acceso,
    instantanea: &str,
    mut escritor: EscritorDelVolcado,
) -> Result<(), ErrorDelMotor> {
    let (repositorio, _puesta) =
        abridor(acceso, BarrasDelMotor::mudas()).map_err(|causa| ErrorDelMotor::Repositorio {
            motivo: ejecucion::motivo_de_la_biblioteca(&causa.to_string(), acceso),
            causa,
        })?;

    match repositorio.volcar_a(instantanea, &mut escritor) {
        Ok(()) => {
            escritor.terminar_bien();
            Ok(())
        }
        Err(causa) => {
            let motivo = ejecucion::motivo_de_la_biblioteca(&causa.to_string(), acceso);

            // La ranura **antes** de devolver: el `exec` puede estar leyendo el `stdin` ahora
            // mismo, y el motivo tiene que estar puesto cuando vea el canal cerrado.
            escritor.fallar(motivo.clone());

            Err(ErrorDelMotor::Repositorio { motivo, causa })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use rustic_core::repofile::SnapshotId;
    use sqlx::SqlitePool;
    use time::OffsetDateTime;
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};
    use uuid::Uuid;

    use super::cola::CAPACIDAD_DE_LA_COLA;
    use super::hub::{EventoDeEjecucion, EventoDeRestauracion, EventoDelMotor};
    use super::restauracion::destino_del_objetivo;
    use super::{
        Ajustes, Dependencias, ErrorDelMotor, FASE_FALLIDA, FASE_RESTAURANDO, FASE_TERMINADA,
        Motor, ejecucion, normalizar_el_destino,
    };
    use crate::db::entities::{
        AltaDeRepositorio, AltaDeRestauracion, AltaDeTarea, AltaDeTarget, Ejecucion, Ejecuciones,
        Estado, ObjetivoDeRestauracion, Repositorios, Restauracion, Restauraciones, Tareas,
        Targets, TipoDeTarget,
    };
    use crate::rustic::{Acceso, BarrasDelMotor, Error, PuestaEnMarcha, Repositorio};
    use crate::secret::SecretString;
    use crate::test_support::{
        CLAVE_SECRETA_DE_PRUEBA, DirectorioDePrueba, PASSPHRASE_DE_PRUEBA, abridor_local,
        abrir_local, base_con_clave, destino_de_s3_que_rechaza, directorio_de_prueba,
        sembrar_snapshots, snapshot_guardado, volcado_guardado,
    };

    /// Un plazo generoso para esperar a que una ejecución termine.
    ///
    /// Noventa segundos, y el número está **medido**: lo que tarda una ejecución no lo decide el
    /// abridor —los de estas pruebas duermen a lo sumo tres segundos— sino la propia biblioteca al
    /// abrir el repositorio, y ahí `init` y `open` tardan **6,4 s** cada uno en la compilación de
    /// pruebas (es el coste de derivar la clave del repositorio, sin optimizar). Un ciclo completo
    /// abre el repositorio tres o cuatro veces —al copiar y al leer lo copiado—, así que un test
    /// sano ronda los veinte segundos **en solitario**, y la suite corre dieciséis tests a la vez:
    /// con treinta segundos, el plazo lo agotaba una copia perfectamente correcta bajo carga, y el
    /// fallo se leía como un cuelgue que no era. Lo que sigue midiendo este plazo es un cuelgue de
    /// verdad: una ejecución que no acaba **nunca** no acaba ni en noventa segundos.
    const PLAZO_DE_ESPERA: Duration = Duration::from_secs(90);

    /// El plazo de las pruebas que **usan un contenedor de verdad**.
    ///
    /// Tres minutos, y no los treinta segundos de las demás: medido, la suite de `--ignored` levanta
    /// veinte contenedores a la vez y el motor de contenedores de esta máquina se atasca él solo
    /// —`database is locked`, en su propia base—, así que una copia que en solitario tarda un
    /// segundo puede tardar mucho más bajo esa carga. Lo que este plazo mide sigue siendo un
    /// cuelgue, no una prueba lenta: una copia de un mebibyte no tarda tres minutos en ningún
    /// entorno sano.
    const PLAZO_DEL_CONTENEDOR: Duration = Duration::from_secs(180);

    /// El socket de un motor de contenedores que no existe.
    ///
    /// Es lo que quieren casi todas estas pruebas: el camino de un volcado **no** se recorre aquí
    /// —necesita un contenedor de verdad—, y el resto de caminos no tocan el socket. Las que sí lo
    /// tocan fallan por él, que es exactamente lo que se busca.
    const SOCKET_QUE_NO_EXISTE: &str = "unix:///no/existe/kesto-prueba.sock";

    // ── Los abridores inyectados ─────────────────────────────────────────────
    //
    // El abridor es el instrumento de estas pruebas, y por eso el punto de inyección no es un
    // adorno: sin él habría que abrir un S3 de verdad para medir la cola, el orden o la
    // concurrencia. Cada abridor tiene sus propios `static` —y cada uno lo usa **una** prueba—,
    // porque la suite corre en paralelo y dos pruebas compartiendo un contador se pisarían.

    /// El fallo controlado de un abridor de prueba: el destino no se alcanza.
    fn no_alcanzable(acceso: &Acceso) -> Error {
        Error::DestinoNoAlcanzable {
            destino: acceso.endpoint.clone(),
            motivo: "el abridor de esta prueba no abre nada".to_owned(),
        }
    }

    /// Un abridor que se entretiene y falla. Deja al trabajador ocupado sin cronometrar nada.
    fn abridor_lento(
        acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        std::thread::sleep(Duration::from_secs(1));

        Err(no_alcanzable(acceso))
    }

    /// Un abridor que entra en **pánico**.
    ///
    /// Es el instrumento de la prueba que mide que un pánico del hilo de trabajo **no se propaga**:
    /// lo recoge el hilo de bloqueo y llega como un error del motor con nombre propio. El texto es
    /// explícito a propósito —lo imprime el gancho de pánicos del proceso y hay que poder saber que
    /// lo provocó la prueba—, y no debe parecerse a nada que la biblioteca diga.
    fn abridor_que_panica(
        _acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        panic!(
            "el abridor de esta prueba se cae a propósito, y el pánico no puede llegar a quien pregunta"
        );
    }

    /// Un abridor que siempre falla, sin entretenerse.
    fn abridor_que_no_abre(
        acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        Err(no_alcanzable(acceso))
    }

    /// Lo que vieron los abridores que miden concurrencia.
    static DENTRO: AtomicUsize = AtomicUsize::new(0);
    static MAXIMO: AtomicUsize = AtomicUsize::new(0);

    /// Un abridor que **mide** cuántos están dentro a la vez y falla. Nunca espera a nadie.
    ///
    /// Es el instrumento de la mitad «en serie»: con dos ejecuciones del mismo repositorio, el
    /// máximo tiene que quedarse en uno, porque el trabajador las atiende de una en una.
    fn abridor_que_mide_la_serie(
        acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        let dentro = DENTRO.fetch_add(1, Ordering::SeqCst) + 1;
        MAXIMO.fetch_max(dentro, Ordering::SeqCst);

        std::thread::sleep(Duration::from_millis(200));

        DENTRO.fetch_sub(1, Ordering::SeqCst);

        Err(no_alcanzable(acceso))
    }

    /// Lo que vio el abridor que espera al otro.
    static ESPERANDO: AtomicUsize = AtomicUsize::new(0);
    static MAXIMO_ESPERANDO: AtomicUsize = AtomicUsize::new(0);
    static EN_PARALELO: AtomicBool = AtomicBool::new(false);

    /// Un abridor que **espera a que el otro entre** antes de fallar.
    ///
    /// Es el instrumento de la mitad «en paralelo», y espera en vez de cronometrar a propósito: si
    /// dos repositorios distintos pueden avanzar a la vez, el segundo llega mientras el primero
    /// espera y la espera acaba sola. Si estuvieran serializados, la espera se agota —de ahí el
    /// plazo, para no colgarse— y la marca queda en falso.
    fn abridor_que_espera_al_otro(
        acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        let dentro = ESPERANDO.fetch_add(1, Ordering::SeqCst) + 1;
        MAXIMO_ESPERANDO.fetch_max(dentro, Ordering::SeqCst);

        let limite = Instant::now() + Duration::from_secs(5);
        while ESPERANDO.load(Ordering::SeqCst) < 2 && Instant::now() < limite {
            std::thread::yield_now();
        }

        if ESPERANDO.load(Ordering::SeqCst) >= 2 {
            EN_PARALELO.store(true, Ordering::SeqCst);
        }

        std::thread::sleep(Duration::from_millis(50));
        ESPERANDO.fetch_sub(1, Ordering::SeqCst);

        Err(no_alcanzable(acceso))
    }

    /// Cuántas veces ha llamado el abridor que falla la primera.
    static LLAMADAS: AtomicUsize = AtomicUsize::new(0);

    /// Un abridor que falla **la primera** vez y después abre de verdad.
    ///
    /// Es lo que mide que un fallo no se lleve por delante al trabajador: la segunda ejecución del
    /// mismo repositorio tiene que arrancar y acabar bien, sin reiniciar nada.
    fn abridor_que_falla_la_primera(
        acceso: &Acceso,
        barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        if LLAMADAS.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(no_alcanzable(acceso));
        }

        abridor_local(acceso, barras)
    }

    /// Si el abridor que mira el runtime lo encontró.
    static TENIA_RUNTIME: AtomicBool = AtomicBool::new(true);

    /// Un abridor que **apunta** si el hilo que abre está dentro de un runtime.
    ///
    /// Es el candado de la medición más grave de este change: llamar a Rustic desde dentro de un
    /// runtime entra en pánico, así que el hilo del trabajador no puede tener uno. Lo que se mide
    /// es el punto exacto —el abridor corre justo donde corre la apertura del repositorio— y se
    /// apunta en vez de entrar en pánico para que un fallo se lea como un fallo y no como un
    /// trabajador muerto.
    fn abridor_que_mira_el_runtime(
        acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        TENIA_RUNTIME.store(
            tokio::runtime::Handle::try_current().is_ok(),
            Ordering::SeqCst,
        );

        Err(no_alcanzable(acceso))
    }

    /// Un abridor que se entretiene tres segundos: para medir que el runtime sigue libre.
    fn abridor_que_duerme_tres_segundos(
        acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        std::thread::sleep(Duration::from_secs(3));

        Err(no_alcanzable(acceso))
    }

    /// Un abridor que no termina en el plazo de parada.
    fn abridor_que_no_termina(
        acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        std::thread::sleep(Duration::from_secs(5));

        Err(no_alcanzable(acceso))
    }

    /// Un abridor que informa un **total** por el gancho y falla.
    ///
    /// El gancho es lo que la biblioteca llama cuando avanza, así que usarlo desde el abridor es lo
    /// que permite medir el porcentaje sin copiar nada: se declara un total y unos bytes, y lo que
    /// se mira es lo que el motor publica.
    fn abridor_con_total(
        acceso: &Acceso,
        barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        let barra = <BarrasDelMotor as rustic_core::ProgressBars>::progress(
            &barras,
            rustic_core::ProgressType::Bytes,
            "prueba-del-motor",
        );
        barra.set_length(2000);
        barra.inc(500);

        Err(no_alcanzable(acceso))
    }

    /// El mismo abridor, pero **sin** declarar ningún total: como un volcado por `stdout`.
    fn abridor_sin_total(
        acceso: &Acceso,
        barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        let barra = <BarrasDelMotor as rustic_core::ProgressBars>::progress(
            &barras,
            rustic_core::ProgressType::Bytes,
            "prueba-del-motor",
        );
        barra.inc(500);

        Err(no_alcanzable(acceso))
    }

    // ── Los dos abridores que necesitan los hallazgos de la auditoría ────────

    /// El destino que la prueba espera que el motor resuelva, y el que el abridor recibió.
    static DESTINO_ESPERADO: OnceLock<String> = OnceLock::new();
    static DESTINO_RECIBIDO: OnceLock<String> = OnceLock::new();
    static EL_DESTINO_ERA_EL_ESPERADO: AtomicBool = AtomicBool::new(false);
    static DENTRO_DEL_ABRIDOR: AtomicBool = AtomicBool::new(false);
    static PUEDE_SEGUIR_EL_ABRIDOR: AtomicBool = AtomicBool::new(false);

    /// Un abridor que **apunta el destino que recibe** y se queda esperando una señal.
    ///
    /// Es el instrumento de la prueba de la serialización: lo que hay que medir es **contra qué
    /// repositorio** acaba copiando una ejecución cuya tarea se edita mientras su trabajador está
    /// parado. El destino entra por el `Acceso` —es lo único que el motor le resuelve a la
    /// biblioteca—, así que se apunta el `endpoint` y se compara con el que se esperaba; y la
    /// espera es lo que ensancha la ventana hasta que se puede medir sin cronometrar nada: la
    /// prueba edita la tarea **dentro** del abridor.
    ///
    /// Al soltarla abre el repositorio **de verdad** (`abridor_local`) para que la copia llegue a su
    /// final: así lo que se afirma no es lo que el abridor vio, sino dónde acabó la instantánea.
    fn abridor_que_apunta_el_destino_y_espera_la_senal(
        acceso: &Acceso,
        barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        let _ = DESTINO_RECIBIDO.set(acceso.endpoint.clone());
        EL_DESTINO_ERA_EL_ESPERADO.store(
            DESTINO_ESPERADO
                .get()
                .is_some_and(|esperado| *esperado == acceso.endpoint),
            Ordering::SeqCst,
        );
        DENTRO_DEL_ABRIDOR.store(true, Ordering::SeqCst);

        let limite = Instant::now() + Duration::from_secs(5);
        while !PUEDE_SEGUIR_EL_ABRIDOR.load(Ordering::SeqCst) && Instant::now() < limite {
            std::thread::yield_now();
        }

        abridor_local(acceso, barras)
    }

    /// Si el abridor que se atasca ya está dentro de su primera llamada.
    static ATASCADO_EN_EL_ABRIDOR: AtomicBool = AtomicBool::new(false);
    static PUEDE_SOLTARSE_EL_ABRIDOR: AtomicBool = AtomicBool::new(false);
    static LLAMADAS_AL_ABRIDOR: AtomicUsize = AtomicUsize::new(0);

    /// Un abridor que **se atasca la primera vez** y falla en el acto las siguientes.
    ///
    /// Es el instrumento de la prueba de la cola llena: para llenarla, el trabajador tiene que
    /// quedarse dentro de un trabajo mientras entran los demás, y para que la prueba no deje
    /// sesenta y cuatro copias lentas detrás, a partir de la segunda el abridor falla sin esperar
    /// nada: en cuanto la prueba lo suelta, el trabajador recorre lo que quedaba en el acto.
    fn abridor_que_se_atasca_la_primera(
        acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<(Repositorio, PuestaEnMarcha), Error> {
        if LLAMADAS_AL_ABRIDOR.fetch_add(1, Ordering::SeqCst) == 0 {
            ATASCADO_EN_EL_ABRIDOR.store(true, Ordering::SeqCst);

            let limite = Instant::now() + Duration::from_secs(5);
            while !PUEDE_SOLTARSE_EL_ABRIDOR.load(Ordering::SeqCst) && Instant::now() < limite {
                std::thread::yield_now();
            }
        }

        Err(no_alcanzable(acceso))
    }

    // ── El aparato de prueba ─────────────────────────────────────────────────

    /// La base, un repositorio **local** y las cuatro capas que el motor necesita.
    ///
    /// El repositorio de la fila lleva por `endpoint` la **ruta** de un directorio temporal: eso es
    /// lo que el abridor local del arnés lee para abrir un repositorio del sistema de ficheros en
    /// vez de un cubo de S3, y es lo que permite que todo el ciclo de vida —cola, hilo, archivado,
    /// fila— se recorra en el CI sin red, sin credenciales y sin motor de contenedores.
    struct Aparato {
        /// El directorio de la base: al soltarse se borra, y con él la base.
        _base: DirectorioDePrueba,
        /// El pool, para leer con **SQL directo** lo que el motor escribió.
        pool: SqlitePool,
        /// Las cuatro capas, tal y como las recibe el motor.
        dependencias: Dependencias,
        /// El directorio donde vive el repositorio local.
        _destino: DirectorioDePrueba,
        /// Su ruta, que es el `endpoint` de la fila.
        destino: PathBuf,
        /// Los directorios de los que copian los targets de tipo `directory`.
        ///
        /// Se retienen todos: un directorio que se borrara al soltar su guarda dejaría la copia
        /// leyendo un camino que ya no está.
        fuentes: Vec<DirectorioDePrueba>,
        /// Los destinos **de más** que haya dado de alta la prueba: se retienen por lo mismo.
        otros_destinos: Vec<DirectorioDePrueba>,
        /// El repositorio de la primera fila, el que usa casi todo.
        repositorio: Uuid,
    }

    impl Aparato {
        /// Monta el aparato: la base migrada, las cuatro capas y un repositorio local vacío.
        async fn nuevo() -> Self {
            let (base, pool, clave) = base_con_clave().await;
            let destino_guardado = directorio_de_prueba();
            let destino = destino_guardado.ruta().to_path_buf();

            // Las cuatro capas se construyen aquí a mano y **no** con `EstadoDeBase::nuevo`: el
            // motor no nombra `src/state` a propósito —para que el día que el `AppState` lleve el
            // motor no haya un ciclo de módulos—, y su prueba tampoco tiene por qué.
            let dependencias = Dependencias {
                repositorios: Arc::new(Repositorios::nuevo(pool.clone(), clave.clone())),
                targets: Arc::new(Targets::nuevo(pool.clone(), clave.clone())),
                tareas: Arc::new(Tareas::nuevo(pool.clone())),
                ejecuciones: Arc::new(Ejecuciones::nuevo(pool.clone(), clave.clone())),
                restauraciones: Arc::new(Restauraciones::nuevo(pool.clone())),
            };

            let mut aparato = Self {
                _base: base,
                pool,
                dependencias,
                _destino: destino_guardado,
                destino,
                fuentes: Vec::new(),
                otros_destinos: Vec::new(),
                repositorio: Uuid::nil(),
            };

            aparato.repositorio = aparato
                .dar_de_alta_el_repositorio("repositorio-de-prueba", &aparato.destino.clone())
                .await;

            aparato
        }

        /// Da de alta un repositorio cuyo `endpoint` es la cadena que se le diga, **tal cual**.
        ///
        /// Existe aparte del que toma una ruta porque un destino de S3 es una **URL**, no un camino
        /// del sistema de ficheros: meterla en un `Path` para reutilizar aquél sería escribir
        /// «parece una ruta» sobre un dato que no lo es.
        async fn dar_de_alta_el_repositorio_con_endpoint(
            &self,
            nombre: &str,
            endpoint: &str,
        ) -> Uuid {
            self.dependencias
                .repositorios
                .alta(AltaDeRepositorio {
                    name: nombre.to_owned(),
                    endpoint: endpoint.to_owned(),
                    bucket: "cubo-de-prueba".to_owned(),
                    region: "region-de-prueba".to_owned(),
                    access_key_id: "clave-de-acceso-de-prueba".to_owned(),
                    secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
                    passphrase: SecretString::new(PASSPHRASE_DE_PRUEBA.to_owned()),
                })
                .await
                .expect("el alta del repositorio no puede fallar")
                .id
        }

        /// Da de alta un repositorio cuyo `endpoint` es la ruta que se le diga.
        async fn dar_de_alta_el_repositorio(&self, nombre: &str, endpoint: &Path) -> Uuid {
            self.dar_de_alta_el_repositorio_con_endpoint(nombre, &endpoint.to_string_lossy())
                .await
        }

        /// Un **segundo** destino local: otro directorio temporal, con su fila de repositorio.
        ///
        /// Es lo que necesita la prueba de la serialización, que edita una tarea para apuntarla a
        /// «otro» repositorio y comprueba que la copia **no** fue a él. El directorio se retiene en
        /// `otros_destinos`: si se soltara, la copia que fuera a parar ahí escribiría en un camino
        /// que ya no está y la prueba mediría un fallo del entorno.
        async fn segundo_destino(&mut self) -> Uuid {
            let guardado = directorio_de_prueba();
            let ruta = guardado.ruta().to_path_buf();

            let id = self
                .dar_de_alta_el_repositorio_con_endpoint(
                    "repositorio-de-al-lado",
                    &ruta.to_string_lossy(),
                )
                .await;

            self.otros_destinos.push(guardado);

            id
        }

        /// El [`Acceso`] de un repositorio: su fila con los dos secretos **ya descifrados**.
        ///
        /// Es exactamente lo que tendrá que hacer el manejador de la capa HTTP —leer la fila y armar
        /// con ella el acceso—, así que la prueba lo hace igual y no por un atajo: si el día que se
        /// escriba el manejador resulta que esto no se puede hacer así, aquí se verá.
        async fn acceso_de(&self, repository_id: &Uuid) -> Acceso {
            let fila = self
                .dependencias
                .repositorios
                .leer(repository_id)
                .await
                .expect("la lectura del repositorio no puede fallar")
                .expect("el repositorio de la prueba tiene que estar");

            Acceso {
                endpoint: fila.endpoint,
                bucket: fila.bucket,
                region: fila.region,
                access_key_id: fila.access_key_id,
                secret_access_key: fila.secret_access_key,
                passphrase: fila.passphrase,
            }
        }

        /// Apunta una tarea a otro repositorio, como lo haría `PUT /api/v1/jobs/:id`.
        async fn apuntar_la_tarea_a(&self, tarea: &Uuid, repositorio: Uuid) {
            let mut fila = self
                .dependencias
                .tareas
                .leer(tarea)
                .await
                .expect("la lectura de la tarea no puede fallar")
                .expect("la tarea de la prueba tiene que estar");
            fila.repository_id = repositorio;

            let editada = self
                .dependencias
                .tareas
                .actualizar(&fila)
                .await
                .expect("la edición de la tarea no puede fallar");

            assert!(
                editada,
                "la tarea de la prueba tiene que haber quedado editada, o la ventana que se mide \
                 no se habría abierto"
            );
        }

        /// Un **segundo** repositorio, con una ruta que no existe: lo usan las pruebas cuyo
        /// abridor no abre nada, así que la ruta solo tiene que ser distinta.
        async fn segundo_repositorio(&self) -> Uuid {
            let endpoint = PathBuf::from(format!("/no/existe/kesto-prueba-{}", Uuid::new_v4()));

            self.dar_de_alta_el_repositorio("repositorio-hermano", &endpoint)
                .await
        }

        /// Da de alta un target de **directorio** con los ficheros que se le den.
        ///
        /// Devuelve su identificador y guarda el directorio: la copia lo lee de verdad.
        async fn target_de_directorio(&mut self, ficheros: &[(&str, &str)]) -> Uuid {
            let fuente = directorio_de_prueba();

            for (nombre, contenido) in ficheros {
                std::fs::write(fuente.ruta().join(nombre), contenido)
                    .expect("el fichero de la fuente se escribe");
            }

            let id = self
                .dependencias
                .targets
                .alta(AltaDeTarget {
                    name: "target-de-prueba".to_owned(),
                    target_type: TipoDeTarget::Directory,
                    container_name: None,
                    db_user: None,
                    db_password: None,
                    db_name: None,
                    host_path: Some(fuente.ruta().to_string_lossy().into_owned()),
                    db_path: None,
                })
                .await
                .expect("el alta del target no puede fallar")
                .id;

            self.fuentes.push(fuente);

            id
        }

        /// El directorio de la última fuente dada de alta, que es por donde se copia.
        fn la_fuente(&self) -> &Path {
            self.fuentes
                .last()
                .expect("esta prueba tiene que haber dado de alta una fuente")
                .ruta()
        }

        /// Da de alta un target de **base sin contenedor**: un objetivo incoherente.
        ///
        /// Es la forma más barata de que una ejecución falle de verdad sin depender de nadie: el
        /// fallo lo detecta el motor, antes de tocar el motor de contenedores.
        async fn target_de_base_sin_contenedor(&self) -> Uuid {
            self.dependencias
                .targets
                .alta(AltaDeTarget {
                    name: "target-incoherente".to_owned(),
                    target_type: TipoDeTarget::Mariadb,
                    container_name: None,
                    db_user: Some("operador".to_owned()),
                    db_password: None,
                    db_name: None,
                    host_path: None,
                    db_path: None,
                })
                .await
                .expect("el alta del target no puede fallar")
                .id
        }

        /// Da de alta un target de **contenedor**, con la contraseña que se le diga.
        async fn target_de_contenedor(&self, contenedor: &str, contrasena: Option<&str>) -> Uuid {
            self.dependencias
                .targets
                .alta(AltaDeTarget {
                    name: "tienda".to_owned(),
                    target_type: TipoDeTarget::Mariadb,
                    container_name: Some(contenedor.to_owned()),
                    db_user: Some("operador".to_owned()),
                    db_password: contrasena.map(|valor| SecretString::new(valor.to_owned())),
                    db_name: None,
                    host_path: None,
                    db_path: None,
                })
                .await
                .expect("el alta del target no puede fallar")
                .id
        }

        /// Da de alta una tarea sobre el target y el repositorio dados.
        async fn tarea(
            &self,
            target_id: Uuid,
            repository_id: Uuid,
            retencion: (i64, i64, i64),
            enabled: bool,
        ) -> Uuid {
            let (keep_daily, keep_weekly, keep_monthly) = retencion;

            self.dependencias
                .tareas
                .alta(AltaDeTarea {
                    target_id,
                    repository_id,
                    cron_expression: "0 3 * * *".to_owned(),
                    keep_daily,
                    keep_weekly,
                    keep_monthly,
                    enabled,
                    name: None,
                })
                .await
                .expect("el alta de la tarea no puede fallar")
                .id
        }

        /// Una tarea del repositorio de andar por casa, con la retención por defecto del §3.
        async fn tarea_normal(&self, target_id: Uuid) -> Uuid {
            let repositorio = self.repositorio;

            self.tarea(target_id, repositorio, (7, 4, 12), true).await
        }

        /// Monta el motor con el abridor que se le diga.
        fn motor_con(&self, abridor: super::AbridorDeRepositorios) -> Motor {
            Motor::arrancar(
                Ajustes::nuevos(SOCKET_QUE_NO_EXISTE).con_abridor(abridor),
                self.dependencias.clone(),
            )
            .expect("el motor se monta desde dentro del runtime de la prueba")
        }

        /// Monta el motor con los ajustes que se le den.
        fn motor_y_ajustes(&self, ajustes: Ajustes) -> Motor {
            Motor::arrancar(ajustes, self.dependencias.clone())
                .expect("el motor se monta desde dentro del runtime de la prueba")
        }

        /// Espera a que una ejecución llegue a un estado terminal.
        async fn esperar_el_final(&self, id: &Uuid) -> Ejecucion {
            esperar_el_final(&self.dependencias.ejecuciones, id).await
        }

        /// Cuántas filas de `job_executions` están en el estado dado, con SQL directo.
        async fn filas_en(&self, estado: Estado) -> i64 {
            sqlx::query_scalar("SELECT COUNT(*) FROM job_executions WHERE status = ?1")
                .bind(estado.texto())
                .fetch_one(&self.pool)
                .await
                .expect("el recuento de filas debe poder leerse")
        }

        /// El estado de una fila, con SQL directo.
        async fn estado_en_la_base(&self, id: &Uuid) -> String {
            sqlx::query_scalar("SELECT status FROM job_executions WHERE id = ?1")
                .bind(id.hyphenated().to_string())
                .fetch_one(&self.pool)
                .await
                .expect("el estado de la fila debe poder leerse")
        }

        /// La fila de una ejecución, leída por la capa de acceso.
        async fn fila(&self, id: &Uuid) -> Ejecucion {
            self.dependencias
                .ejecuciones
                .leer(id)
                .await
                .expect("la lectura no puede fallar")
                .expect("la fila tiene que seguir ahí")
        }

        /// Da de alta una fila de `restore_executions` en `pending`, como la deja quien lanza la
        /// restauración.
        ///
        /// Es lo que mide el ciclo de vida: el motor **no** la crea —eso es del manejador, que la
        /// necesita antes de contestar—, así que las pruebas de la persistencia la crean aquí y
        /// comprueban que el motor la lleva a `running` y de ahí a su desenlace.
        async fn alta_de_restauracion(&self, instantanea: &str) -> Restauracion {
            self.dependencias
                .restauraciones
                .alta(AltaDeRestauracion {
                    snapshot_id: instantanea.to_owned(),
                    target_id: None,
                    started_at: OffsetDateTime::now_utc(),
                })
                .await
                .expect("el alta de la restauración no puede fallar")
        }

        /// La fila de una restauración, leída por la capa de acceso.
        async fn fila_de_restauracion(&self, id: &Uuid) -> Restauracion {
            self.dependencias
                .restauraciones
                .leer(id)
                .await
                .expect("la lectura no puede fallar")
                .expect("la fila tiene que seguir ahí")
        }

        /// Cuántas instantáneas tiene el repositorio local, leídas con la biblioteca.
        fn instantaneas(&self) -> usize {
            abrir_local(&self.destino, PASSPHRASE_DE_PRUEBA)
                .list::<SnapshotId>()
                .expect("el repositorio local de prueba lista sus instantáneas")
                .count()
        }
    }

    /// Espera a que una ejecución llegue a `success` o a `failed`.
    async fn esperar_el_final(ejecuciones: &Ejecuciones, id: &Uuid) -> Ejecucion {
        esperar_el_final_con(ejecuciones, id, PLAZO_DE_ESPERA).await
    }

    /// La misma espera, con el plazo que la prueba necesite.
    async fn esperar_el_final_con(
        ejecuciones: &Ejecuciones,
        id: &Uuid,
        plazo: Duration,
    ) -> Ejecucion {
        let limite = Instant::now() + plazo;

        loop {
            let fila = ejecuciones
                .leer(id)
                .await
                .expect("la lectura no puede fallar")
                .expect("la fila tiene que seguir ahí");

            if matches!(fila.status, Estado::Success | Estado::Failed) {
                return fila;
            }

            assert!(
                Instant::now() < limite,
                "la ejecución no ha llegado a un estado terminal en {plazo:?}: {:?}",
                fila.status
            );

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Espera a que una fila esté en el estado dado.
    async fn esperar_el_estado(aparato: &Aparato, id: &Uuid, estado: Estado) {
        let limite = Instant::now() + PLAZO_DE_ESPERA;

        loop {
            if aparato.fila(id).await.status == estado {
                return;
            }

            assert!(
                Instant::now() < limite,
                "la ejecución no llegó a `{}` en {PLAZO_DE_ESPERA:?}",
                estado.texto()
            );

            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Espera a que una señal de las pruebas se ponga, con el plazo de siempre.
    ///
    /// Es lo que permite escribir las pruebas de la serialización y de la cola llena **sin
    /// cronometrar nada**: se espera a que el trabajador esté donde tiene que estar —dentro del
    /// abridor, con la tarea ya resuelta— y solo entonces se actúa, en vez de dormir un rato a ver
    /// si ya llegó.
    async fn esperar_a(señal: &AtomicBool, que: &str) {
        let limite = Instant::now() + PLAZO_DE_ESPERA;

        while !señal.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < limite,
                "no ha ocurrido {que} dentro de {PLAZO_DE_ESPERA:?}"
            );

            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    // ── El encolado ──────────────────────────────────────────────────────────

    /// **1.** `encolar` crea la fila **y la encola**: `pending` en la base, comprobado con SQL
    /// directo.
    ///
    /// La fila `pending` se observa en el hueco que el contrato hace posible: el trabajador está
    /// ocupado con la primera ejecución —su abridor se entretiene un segundo— así que la segunda
    /// espera en la cola, y ahí está `pending` de verdad y no de paso. Con un solo encolado habría
    /// que adivinar el instante, y el estado que se mide sería el que toque.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn encolar_crea_la_fila_y_la_encola() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let ocupada = aparato.tarea_normal(target).await;
        let en_cola = aparato.tarea_normal(target).await;
        let motor = aparato.motor_con(abridor_lento);

        let primera = motor
            .encolar(ocupada)
            .await
            .expect("la primera tiene que entrar");
        assert_eq!(
            primera.status,
            Estado::Pending,
            "una ejecución recién encolada nace en `pending`"
        );

        // El trabajador la toma y se queda dentro del abridor: la segunda entra en la cola.
        esperar_el_estado(&aparato, &primera.id, Estado::Running).await;

        let segunda = motor
            .encolar(en_cola)
            .await
            .expect("la segunda tarea tiene que entrar");
        assert_eq!(
            segunda.status,
            Estado::Pending,
            "la ejecución devuelta por el encolado no está en `pending`"
        );

        // Y en la tabla, con SQL directo: lo que se afirma es lo que la base tiene, no lo que el
        // alta devolvió.
        assert_eq!(
            aparato.estado_en_la_base(&segunda.id).await,
            "pending",
            "la fila de la ejecución en cola no quedó en `pending` en la tabla"
        );
        assert_eq!(
            aparato.filas_en(Estado::Pending).await,
            1,
            "tiene que haber exactamente una ejecución esperando en la cola"
        );

        // Y las dos acaban: nada se queda colgado.
        assert_eq!(
            aparato.esperar_el_final(&segunda.id).await.status,
            Estado::Failed
        );
    }

    /// **2.** `encolar` sobre una tarea que no existe da el error de «no está» y **no crea fila**.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn encolar_sobre_una_tarea_que_no_existe_da_el_error_y_no_crea_fila() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_local);
        let inexistente = Uuid::new_v4();

        let error = motor
            .encolar(inexistente)
            .await
            .expect_err("una tarea que no existe no puede encolarse");

        assert!(
            matches!(error, ErrorDelMotor::TareaQueNoEsta { job_id } if job_id == inexistente),
            "el error no es el de «la tarea no está»: {error:?}"
        );
        assert!(
            error.to_string().contains(&inexistente.to_string()),
            "el motivo no nombra la tarea que falta: {error}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_executions")
            .fetch_one(&aparato.pool)
            .await
            .expect("el recuento debe poder leerse");

        assert_eq!(filas, 0, "la tarea no existe y se creó una ejecución");
    }

    /// **3.** `encolar` dos veces sobre la misma tarea da el error de «ya tiene una en curso» y
    /// **no crea una segunda fila**.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn encolar_dos_veces_sobre_la_misma_tarea_da_el_error_y_no_crea_una_segunda_fila() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let tarea = aparato.tarea_normal(target).await;
        let motor = aparato.motor_con(abridor_lento);

        let primera = motor
            .encolar(tarea)
            .await
            .expect("la primera tiene que entrar");

        // El trabajador la toma: la fila queda bloqueando la tarea, en `running`.
        esperar_el_estado(&aparato, &primera.id, Estado::Running).await;

        let error = motor
            .encolar(tarea)
            .await
            .expect_err("con una ejecución en curso, la segunda no puede entrar");

        assert!(
            matches!(error, ErrorDelMotor::YaTieneUnaEnCurso { job_id } if job_id == tarea),
            "el error no es el del `409`: {error:?}"
        );
        assert_eq!(
            aparato.filas_en(Estado::Running).await + aparato.filas_en(Estado::Pending).await,
            1,
            "la guarda dijo que no, pero dejó una segunda fila en la tabla"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_executions")
            .fetch_one(&aparato.pool)
            .await
            .expect("el recuento debe poder leerse");
        assert_eq!(
            filas, 1,
            "se creó una segunda ejecución para la misma tarea"
        );

        assert_eq!(
            aparato.esperar_el_final(&primera.id).await.status,
            Estado::Failed
        );
    }

    /// **4.** Una tarea con `enabled = 0` se encola igual.
    ///
    /// `enabled` gobierna al planificador por `cron_expression`, y el disparo manual es otra cosa:
    /// el motor no tiene planificador todavía, así que negarse aquí sería negarse sin poder
    /// explicar por qué.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn una_tarea_desactivada_se_encola_igual() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let repositorio = aparato.repositorio;
        let desactivada = aparato.tarea(target, repositorio, (7, 4, 12), false).await;
        let motor = aparato.motor_con(abridor_lento);

        let ejecucion = motor
            .encolar(desactivada)
            .await
            .expect("una tarea desactivada sí se dispara a mano");

        assert_eq!(ejecucion.status, Estado::Pending);
        assert_eq!(
            aparato.esperar_el_final(&ejecucion.id).await.status,
            Estado::Failed
        );
    }

    // ── El ciclo de vida ─────────────────────────────────────────────────────

    /// **5.** El ciclo de vida completo, con un repositorio local **de verdad**: la prueba que
    /// demuestra que el motor copia.
    ///
    /// Se afirma en los dos extremos y no solo en la fila: la ejecución acaba en `success` con su
    /// instantánea y sus contadores, y el contenido está **dentro del repositorio que la fila
    /// declara**, leído con la biblioteca —que es el camino de quien recupera una copia—. Un motor
    /// que escribiera la fila sin copiar nada pasaría la primera mitad.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_ciclo_de_vida_completo_copia_de_verdad() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato
            .target_de_directorio(&[("uno.txt", "contenido uno"), ("dos.txt", "dos")])
            .await;
        let fuente = aparato.la_fuente().to_path_buf();
        let tarea = aparato.tarea_normal(target).await;
        let motor = aparato.motor_con(abridor_local);

        let ejecucion = motor
            .encolar(tarea)
            .await
            .expect("el encolado tiene que entrar");
        // El alta devuelve la fila tal y como se insertó, en memoria y sin que nadie la haya
        // tocado: aquí sí se puede afirmar `pending` sin carrera. Sobre la tabla no: el trabajador
        // es otro hilo y la marca `running` en cuanto la ejecución entra en su buzón, mucho antes
        // de abrir el repositorio, así que leer la tabla y exigir `pending` es competir con él.
        // Lo que esta prueba tiene que garantizar es que la ejecución *nace* en curso —`pending`
        // o ya `running`, nunca en un estado terminal— y que *acaba* en el estado que se afirma
        // más abajo con `esperar_el_final`.
        assert_eq!(
            ejecucion.status,
            Estado::Pending,
            "la ejecución no nace en `pending`"
        );
        let en_la_tabla = aparato.estado_en_la_base(&ejecucion.id).await;
        assert!(
            en_la_tabla == Estado::Pending.texto() || en_la_tabla == Estado::Running.texto(),
            "la ejecución no está en curso en la tabla, sino en `{en_la_tabla}`"
        );

        let final_de_la_fila = aparato.esperar_el_final(&ejecucion.id).await;

        assert_eq!(
            final_de_la_fila.status,
            Estado::Success,
            "la copia tenía que acabar bien y acabó en {:?}: {:?}",
            final_de_la_fila.status,
            final_de_la_fila.error_message
        );
        assert!(
            final_de_la_fila.finished_at.is_some(),
            "una ejecución terminada tiene que llevar su `finished_at`"
        );
        let instantanea = final_de_la_fila
            .snapshot_id
            .clone()
            .expect("una copia buena declara la instantánea que ha guardado");
        assert!(
            final_de_la_fila.bytes_processed > 0 && final_de_la_fila.bytes_uploaded > 0,
            "los contadores de una copia de directorio no pueden salir a cero: {}, {}",
            final_de_la_fila.bytes_processed,
            final_de_la_fila.bytes_uploaded
        );

        // Y el contenido, leído con la biblioteca **del repositorio que la fila declara**.
        let nodo = fuente
            .join("uno.txt")
            .to_string_lossy()
            .trim_start_matches('/')
            .to_owned();
        let guardado =
            volcado_guardado(&aparato.destino, PASSPHRASE_DE_PRUEBA, &instantanea, &nodo);

        assert_eq!(
            guardado.as_deref(),
            Some(b"contenido uno".as_slice()),
            "el fichero copiado no está en el repositorio, o no tiene el contenido que se copió"
        );

        let resumen = snapshot_guardado(&aparato.destino, PASSPHRASE_DE_PRUEBA, &instantanea);
        assert!(
            resumen.summary.is_some(),
            "la instantánea guardada no trae resumen: el archivado no procesó nada"
        );
    }

    /// **6.** Un fallo acaba en `failed` con `error_message` y `finished_at`, y **ninguna**
    /// ejecución se queda en `running`.
    ///
    /// Los dos fallos son distintos y el motivo tiene que distinguirlos: un objetivo incoherente
    /// —una base sin contenedor, que el motor caza antes de tocar nada— y un destino que no abre.
    /// Y después se recorre la tabla entera: lo que el contrato prohíbe no es que esta o aquella
    /// fila se quede a medias, es que quede alguna en `running`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_fallo_acaba_en_failed_con_su_motivo_y_ninguna_ejecucion_se_queda_en_running() {
        let mut aparato = Aparato::nuevo().await;
        let incoherente = aparato.target_de_base_sin_contenedor().await;
        let con_destino_roto = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let sin_contenedor = aparato.tarea_normal(incoherente).await;
        let sin_destino = aparato.tarea_normal(con_destino_roto).await;
        let motor = aparato.motor_con(abridor_que_no_abre);

        let primera = motor
            .encolar(sin_contenedor)
            .await
            .expect("tiene que entrar");
        let segunda = motor.encolar(sin_destino).await.expect("tiene que entrar");

        let primera = aparato.esperar_el_final(&primera.id).await;
        let segunda = aparato.esperar_el_final(&segunda.id).await;

        for fila in [&primera, &segunda] {
            assert_eq!(fila.status, Estado::Failed, "el fallo no acabó en `failed`");
            assert!(
                fila.finished_at.is_some(),
                "una ejecución fallida tiene que llevar su `finished_at`"
            );
            assert!(
                fila.error_message.as_deref().is_some_and(|m| !m.is_empty()),
                "una ejecución fallida tiene que decir por qué"
            );
        }

        let motivo_incoherente = primera.error_message.clone().unwrap_or_default();
        let motivo_del_destino = segunda.error_message.clone().unwrap_or_default();

        assert!(
            motivo_incoherente.contains("container_name"),
            "el motivo de un objetivo incoherente tiene que nombrar el campo que falta: \
             {motivo_incoherente}"
        );
        assert!(
            motivo_del_destino.contains(&aparato.destino.to_string_lossy().to_string())
                || motivo_del_destino.contains("no se alcanza")
                || motivo_del_destino.contains("destino"),
            "el motivo del destino roto no dice de qué va: {motivo_del_destino}"
        );
        assert_ne!(
            motivo_incoherente, motivo_del_destino,
            "los dos fallos tienen motivos distintos y la fila los guarda iguales"
        );

        assert_eq!(
            aparato.filas_en(Estado::Running).await,
            0,
            "alguna ejecución se ha quedado en `running`: el panel la pintaría como «en marcha»"
        );
    }

    // ── La retención ─────────────────────────────────────────────────────────

    /// **7.a** La retención **se aplica** en el camino bueno.
    ///
    /// Con tres instantáneas sembradas de tres días distintos y una retención diaria de una sola,
    /// el repositorio no puede quedarse con las cuatro que habría tras copiar: si el motor no
    /// llamara a la retención, contarían cuatro. Es la mitad que distingue una retención aplicada
    /// de una retención decorativa.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_retencion_se_aplica_en_el_camino_bueno() {
        let mut aparato = Aparato::nuevo().await;

        // El repositorio se siembra **antes** de que el motor lo abra, y con la biblioteca: lo que
        // se mide es que la retención del motor, sobre instantáneas que ya estaban, retira lo que
        // sobra.
        sembrar_snapshots(
            &aparato.destino,
            PASSPHRASE_DE_PRUEBA,
            &[
                "2026-01-01T10:00:00Z",
                "2026-01-02T10:00:00Z",
                "2026-01-03T10:00:00Z",
            ],
        );

        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let repositorio = aparato.repositorio;
        // La retención que se mide: **una** diaria, y las otras dos familias apagadas, que es lo
        // que aísla lo que se está midiendo.
        let tarea = aparato.tarea(target, repositorio, (1, 0, 0), true).await;
        let motor = aparato.motor_con(abridor_local);

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
        let final_de_la_fila = aparato.esperar_el_final(&ejecucion.id).await;

        assert_eq!(
            final_de_la_fila.status,
            Estado::Success,
            "la copia tenía que acabar bien: {:?}",
            final_de_la_fila.error_message
        );

        // Dos: la que la regla diaria conserva de las tres sembradas, y la que acaba de hacer la
        // copia —que cae en su propio grupo, porque el host, la etiqueta y las rutas son los de la
        // tarea, y el grupo es lo que hace que la retención de una tarea no decida sobre las
        // copias de otra—. El número es exacto a propósito: con `<= 3` una retención que no
        // hubiera llegado a aplicarse sobre **este** repositorio pasaría la prueba.
        let instantaneas = aparato.instantaneas();

        assert_eq!(
            instantaneas, 2,
            "el repositorio quedó con {instantaneas} instantáneas y no con dos: la regla diaria \
             tenía que retirar dos de las tres sembradas, y la copia nueva quedarse"
        );
    }

    /// **7.b** Un fallo de la retención **no cambia el resultado de la copia**.
    ///
    /// Es el desenlace que hay que separar con cuidado: la copia ya está hecha y es correcta, y lo
    /// que hay que contar es que la copia está y la retención no. Se fuerza el fallo con las tres
    /// reglas a cero, que es el único caso que la retención rechaza —no conservaría nada, ni la
    /// copia que se acaba de hacer—, y la fila tiene que seguir siendo `success` con su
    /// instantánea y **sin** motivo de error.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_fallo_de_la_retencion_no_cambia_el_resultado_de_la_copia() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let repositorio = aparato.repositorio;
        let tarea = aparato.tarea(target, repositorio, (0, 0, 0), true).await;
        let motor = aparato.motor_con(abridor_local);

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
        let final_de_la_fila = aparato.esperar_el_final(&ejecucion.id).await;

        assert_eq!(
            final_de_la_fila.status,
            Estado::Success,
            "la copia estaba hecha y la retención falló: la fila no puede decir `failed`: {:?}",
            final_de_la_fila.error_message
        );
        assert!(
            final_de_la_fila.snapshot_id.is_some(),
            "una copia buena declara su instantánea aunque la retención falle"
        );
        assert_eq!(
            final_de_la_fila.error_message, None,
            "una copia buena no puede llevar motivo de error porque falle la retención"
        );
        assert_eq!(
            aparato.instantaneas(),
            1,
            "la retención falló y aun así retiró algo"
        );
    }

    // ── La cola: en serie, en paralelo y en orden ────────────────────────────

    /// **8.a** Dos ejecuciones del **mismo** repositorio van **en serie**.
    ///
    /// Se mide con el abridor inyectado, que es lo que permite contar cuántos están dentro a la
    /// vez sin cronometrar nada: si el trabajador las atendiera a la vez, el máximo pasaría de uno.
    /// Y el motivo de que tengan que ir en serie está medido —`rustic_core` toma un bloqueo
    /// exclusivo del repositorio, así que dos copias a la vez no corren más: se bloquean—.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dos_ejecuciones_del_mismo_repositorio_van_en_serie() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let una = aparato.tarea_normal(target).await;
        let otra = aparato.tarea_normal(target).await;
        let motor = aparato.motor_con(abridor_que_mide_la_serie);

        let primera = motor
            .encolar(una)
            .await
            .expect("la primera tiene que entrar");
        let segunda = motor
            .encolar(otra)
            .await
            .expect("la segunda tiene que entrar");

        assert_eq!(
            aparato.esperar_el_final(&primera.id).await.status,
            Estado::Failed
        );
        assert_eq!(
            aparato.esperar_el_final(&segunda.id).await.status,
            Estado::Failed
        );

        assert_eq!(
            MAXIMO.load(Ordering::SeqCst),
            1,
            "dos ejecuciones del mismo repositorio estuvieron dentro del abridor a la vez: el \
             trabajador tiene que atenderlas de una en una"
        );
    }

    /// **8.b** Dos ejecuciones de repositorios **distintos** pueden estar a la vez en `running`.
    ///
    /// Es la mitad que impide «arreglar» el requisito serializando todo. El instrumento espera en
    /// vez de cronometrar: el abridor del primer repositorio se queda esperando a que entre el del
    /// segundo, así que si los dos avanzan a la vez la espera acaba sola y la marca queda puesta.
    /// Con todo serializado, la espera se agota y la marca se queda en falso.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dos_ejecuciones_de_repositorios_distintos_pueden_estar_a_la_vez_en_running() {
        let mut aparato = Aparato::nuevo().await;
        let hermana = aparato.segundo_repositorio().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let repositorio = aparato.repositorio;
        let una = aparato.tarea(target, repositorio, (7, 4, 12), true).await;
        let otra = aparato.tarea(target, hermana, (7, 4, 12), true).await;
        let motor = aparato.motor_con(abridor_que_espera_al_otro);

        let primera = motor
            .encolar(una)
            .await
            .expect("la primera tiene que entrar");
        let segunda = motor
            .encolar(otra)
            .await
            .expect("la segunda tiene que entrar");

        assert_eq!(
            aparato.esperar_el_final(&primera.id).await.status,
            Estado::Failed
        );
        assert_eq!(
            aparato.esperar_el_final(&segunda.id).await.status,
            Estado::Failed
        );

        assert!(
            EN_PARALELO.load(Ordering::SeqCst),
            "los dos repositorios no llegaron a estar a la vez dentro del abridor: dos \
             repositorios distintos no tienen por qué esperarse"
        );
        assert_eq!(
            MAXIMO_ESPERANDO.load(Ordering::SeqCst),
            2,
            "el máximo de ejecuciones simultáneas no fue dos"
        );
    }

    /// **9.** El orden de encolado se respeta: tres ejecuciones del mismo repositorio se atienden en
    /// el orden en que se encolaron.
    ///
    /// El orden se lee de la **tabla** y no de un contador del abridor: la fila pasa a `running`
    /// justo antes de que el trabajador abra el repositorio, así que el orden de las transiciones
    /// es el orden en que se atienden. Y el abridor se entretiene un segundo, de modo que cada
    /// ventana de `running` dura de sobra para verla.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_orden_de_encolado_se_respeta() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let motor = aparato.motor_con(abridor_lento);

        let mut encoladas = Vec::new();
        for _ in 0..3 {
            let tarea = aparato.tarea_normal(target).await;
            let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
            encoladas.push(ejecucion.id);
        }

        let limite = Instant::now() + PLAZO_DE_ESPERA;
        let mut observadas: Vec<Uuid> = Vec::new();

        while observadas.len() < encoladas.len() && Instant::now() < limite {
            let filas = aparato
                .dependencias
                .ejecuciones
                .listar()
                .await
                .expect("el listado no puede fallar");

            for fila in filas {
                if fila.status == Estado::Running && !observadas.contains(&fila.id) {
                    observadas.push(fila.id);
                }
            }

            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert_eq!(
            observadas, encoladas,
            "las tres ejecuciones no se atendieron en el orden en que se encolaron"
        );

        for id in &encoladas {
            assert_eq!(aparato.esperar_el_final(id).await.status, Estado::Failed);
        }
    }

    /// **10.** Un fallo **no detiene al trabajador**: la siguiente ejecución del mismo repositorio
    /// arranca y acaba bien.
    ///
    /// Es el requisito que impide que un motor haya que reiniciarlo cada vez que una base no
    /// responde, y se mide con el abridor que falla la primera vez y después abre el repositorio de
    /// verdad: la segunda copia tiene que acabar en `success` y dejar su instantánea.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_fallo_no_detiene_al_trabajador() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let falla = aparato.tarea_normal(target).await;
        let va_bien = aparato.tarea_normal(target).await;
        let motor = aparato.motor_con(abridor_que_falla_la_primera);

        let primera = motor.encolar(falla).await.expect("tiene que entrar");
        assert_eq!(
            aparato.esperar_el_final(&primera.id).await.status,
            Estado::Failed,
            "la primera tenía que fallar"
        );

        let segunda = motor.encolar(va_bien).await.expect("tiene que entrar");
        let final_de_la_fila = aparato.esperar_el_final(&segunda.id).await;

        assert_eq!(
            final_de_la_fila.status,
            Estado::Success,
            "después de un fallo, la siguiente ejecución del mismo repositorio tiene que arrancar \
             y acabar bien: {:?}",
            final_de_la_fila.error_message
        );
        assert!(
            final_de_la_fila.snapshot_id.is_some(),
            "la copia que fue bien no dejó su instantánea"
        );
    }

    // ── El runtime y el hilo de Tokio ────────────────────────────────────────

    /// **11.** ⚠️ El hilo del trabajador **no tiene runtime** cuando abre el repositorio.
    ///
    /// Es el candado de la medición más grave de este change: llamar a Rustic desde dentro de un
    /// runtime entra en pánico —«Cannot start a runtime from within a runtime.»—, así que si
    /// alguien envuelve la apertura en un `block_on`, esta prueba tiene que caer. El abridor corre
    /// exactamente donde corre la apertura, y **apunta** lo que ve en vez de entrar en pánico: un
    /// pánico lo recogería el cierre del trabajador y la prueba se leería como un fallo sin motivo.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_trabajador_no_tiene_runtime_cuando_abre_el_repositorio() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let tarea = aparato.tarea_normal(target).await;
        let motor = aparato.motor_con(abridor_que_mira_el_runtime);

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
        assert_eq!(
            aparato.esperar_el_final(&ejecucion.id).await.status,
            Estado::Failed
        );

        assert!(
            !TENIA_RUNTIME.load(Ordering::SeqCst),
            "el hilo del trabajador estaba dentro de un runtime al abrir el repositorio: llamar a \
             Rustic desde ahí entra en pánico, y el pánico deja la copia sin hacer"
        );
    }

    /// **12.** El trabajo **no ocupa un hilo de Tokio**: con una copia en curso sobre un abridor que
    /// duerme, una consulta a la base desde el runtime de la prueba contesta de inmediato.
    ///
    /// Lo que esto mide, dicho con precisión: el hilo del trabajador es un hilo del sistema
    /// operativo, así que el runtime sigue teniendo sus hilos libres para atender lo que llegue
    /// mientras una copia está en vuelo. Lo que **no** mide, y lo dice para que nadie se confíe: un
    /// runtime con dos hilos puede atender una consulta aunque uno esté bloqueado; el candado de que
    /// la apertura no ocurre *dentro* del runtime es la prueba 11.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_trabajo_no_ocupa_un_hilo_de_tokio() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let tarea = aparato.tarea_normal(target).await;
        let motor = aparato.motor_con(abridor_que_duerme_tres_segundos);

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
        esperar_el_estado(&aparato, &ejecucion.id, Estado::Running).await;

        // Un respiro para que el trabajador esté de verdad dentro del abridor.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let consulta = tokio::time::timeout(
            Duration::from_secs(1),
            aparato.dependencias.ejecuciones.leer(&ejecucion.id),
        )
        .await;

        assert!(
            consulta.is_ok(),
            "una consulta a la base desde el runtime no se atendió en un segundo: el trabajo de la \
             copia está ocupando un hilo de Tokio"
        );
        assert!(
            matches!(consulta.expect("ya se comprobó"), Ok(Some(_))),
            "la consulta no devolvió la fila de la ejecución"
        );
        assert_eq!(
            aparato.fila(&ejecucion.id).await.status,
            Estado::Running,
            "la copia ya había terminado cuando se hizo la consulta: la medición no dice nada"
        );

        assert_eq!(
            aparato.esperar_el_final(&ejecucion.id).await.status,
            Estado::Failed
        );
    }

    // ── El progreso ──────────────────────────────────────────────────────────

    /// Vacía el receptor y devuelve los eventos de ejecución que había.
    ///
    /// El receptor transporta [`EventoDelMotor`], y estas pruebas solo miden el progreso de una
    /// copia: los avisos de restauración se saltan, que es lo que haría cualquier lector que
    /// estuviera mirando esta mitad del contrato.
    fn recoger(
        receptor: &mut tokio::sync::broadcast::Receiver<EventoDelMotor>,
    ) -> Vec<EventoDeEjecucion> {
        let mut eventos = Vec::new();

        loop {
            match receptor.try_recv() {
                Ok(EventoDelMotor::Ejecucion(evento)) => eventos.push(evento),
                Ok(EventoDelMotor::Restauracion(_)) => continue,
                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                // Un cliente que se pierde eventos no se corta: sigue leyendo. Aquí no puede
                // pasar —se lee enseguida y son pocos—, pero el camino se escribe igual.
                Err(TryRecvError::Lagged(_)) => continue,
            }
        }

        eventos
    }

    /// Vacía el receptor y devuelve los avisos de **restauración** que había.
    ///
    /// Es el hermano de [`recoger`] para la otra mitad del contrato, con las mismas reglas: se lee
    /// lo que haya, el receptor que se hubiera perdido algún evento sigue leyendo, y los avisos de
    /// una copia se saltan —aquí lo que se mide es la restauración—.
    fn recoger_restauraciones(
        receptor: &mut tokio::sync::broadcast::Receiver<EventoDelMotor>,
    ) -> Vec<EventoDeRestauracion> {
        let mut avisos = Vec::new();

        loop {
            match receptor.try_recv() {
                Ok(EventoDelMotor::Restauracion(aviso)) => avisos.push(aviso),
                Ok(EventoDelMotor::Ejecucion(_)) => continue,
                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                Err(TryRecvError::Lagged(_)) => continue,
            }
        }

        avisos
    }

    /// Un objetivo de restauración **de directorio**: el más barato de los cuatro tipos.
    ///
    /// Lo usan las pruebas que miden el ciclo de vida, la rama y los dos avisos, y existe para no
    /// repetir los diez campos del tipo en cada una. Los dos identificadores van a `nil`: el motor
    /// no los usa —el `target_id` y el `job_id` los resuelve quien restaura, no el motor—, y un
    /// valor inventado sugeriría que hacen falta.
    fn objetivo_de_directorio(destino: &str) -> ObjetivoDeRestauracion {
        ObjetivoDeRestauracion {
            job_id: Uuid::nil(),
            target_id: Uuid::nil(),
            target_type: TipoDeTarget::Directory,
            container_name: None,
            db_user: None,
            db_password: None,
            db_name: None,
            host_path: Some(destino.to_owned()),
            db_path: None,
            target_name: "objetivo-de-prueba".to_owned(),
        }
    }

    /// Un objetivo de restauración **de base**: el contenedor y la base que se le digan.
    ///
    /// Es el que elige la rama de contenedor —`mariadb`, el más simple, con una sola orden—, y el
    /// que usan las pruebas de la reserva por destino y de la rama por tipo.
    fn objetivo_mariadb(contenedor: &str, db_name: Option<&str>) -> ObjetivoDeRestauracion {
        ObjetivoDeRestauracion {
            job_id: Uuid::nil(),
            target_id: Uuid::nil(),
            target_type: TipoDeTarget::Mariadb,
            container_name: Some(contenedor.to_owned()),
            db_user: Some("operador".to_owned()),
            db_password: None,
            db_name: db_name.map(str::to_owned),
            host_path: None,
            db_path: None,
            target_name: "objetivo-de-base-de-prueba".to_owned(),
        }
    }

    /// Un objetivo de `postgresql` con el contenedor y la base que se le digan.
    ///
    /// Sirve para comprobar que la clave de un destino de base **distingue el tipo**: el mismo
    /// contenedor con la misma base, pero otro servidor, es otro destino.
    fn objetivo_postgresql(contenedor: &str, db_name: Option<&str>) -> ObjetivoDeRestauracion {
        ObjetivoDeRestauracion {
            target_type: TipoDeTarget::Postgresql,
            ..objetivo_mariadb(contenedor, db_name)
        }
    }

    /// **14.** `percentage` es **nulo** con un origen sin total y **numérico** con uno que lo
    /// declara, y el evento lleva los siete campos con el `status` de la fila.
    ///
    /// Las dos mitades se miden con el gancho del progreso, que es donde la biblioteca informa: el
    /// abridor inyectado usa las barras que el motor le entrega, así que se puede forzar un total y
    /// unos bytes sin copiar nada. Y el `percentage` nulo no es comodidad: medido, un volcado por
    /// `stdout` **no recibe nunca** un total en su barra de bytes, así que su porcentaje es nulo por
    /// construcción.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_porcentaje_es_nulo_sin_total_y_numerico_con_total() {
        let mut aparato = Aparato::nuevo().await;
        let con_total = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let sin_total = aparato.target_de_directorio(&[("dos.txt", "dos")]).await;
        let tarea_con = aparato.tarea_normal(con_total).await;
        let tarea_sin = aparato.tarea_normal(sin_total).await;

        // 1. Con total declarado: el porcentaje es un número, y los dos contadores están.
        let motor = aparato.motor_con(abridor_con_total);
        let mut receptor = motor
            .hub()
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        let ejecucion = motor.encolar(tarea_con).await.expect("tiene que entrar");
        let fila = aparato.esperar_el_final(&ejecucion.id).await;
        let eventos = recoger(&mut receptor);

        assert!(
            eventos.len() >= 2,
            "se esperaban al menos el de `pending` y el final"
        );
        assert_eq!(
            eventos[0].status,
            Estado::Pending,
            "el primer evento no es el nacimiento de la ejecución"
        );
        assert_eq!(eventos[0].current_action, ejecucion::ACCION_ENCOLADA);

        let final_del_evento = eventos.last().expect("hay eventos");

        assert_eq!(
            final_del_evento.execution_id, ejecucion.id,
            "el evento final no es de la ejecución que corrió"
        );
        assert_eq!(
            final_del_evento.job_id, tarea_con,
            "el evento no lleva la tarea de la ejecución"
        );
        assert_eq!(
            final_del_evento.status, fila.status,
            "el `status` del evento no coincide con el de la fila"
        );
        assert_eq!(fila.status, Estado::Failed);
        assert_eq!(
            final_del_evento.bytes_processed, 2000,
            "el total que declaró el gancho no llegó al evento como bytes procesados"
        );
        assert_eq!(
            final_del_evento.bytes_uploaded, 500,
            "los bytes que informó el gancho no llegaron al evento"
        );
        assert_eq!(
            final_del_evento.percentage,
            Some(25.0),
            "con un total declarado y mayor que cero, el porcentaje tiene que ser un número"
        );
        assert!(
            !final_del_evento.current_action.is_empty(),
            "el evento no dice qué estaba haciendo la ejecución"
        );
        assert_eq!(
            fila.bytes_processed, 2000,
            "los contadores del evento y los de la fila tienen que ser los mismos"
        );
        assert_eq!(fila.bytes_uploaded, 500);

        // 2. Sin total: el porcentaje es nulo y los bytes procesados siguen informados.
        let motor = aparato.motor_con(abridor_sin_total);
        let mut receptor = motor
            .hub()
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        let ejecucion = motor.encolar(tarea_sin).await.expect("tiene que entrar");
        let fila = aparato.esperar_el_final(&ejecucion.id).await;
        let eventos = recoger(&mut receptor);
        let final_del_evento = eventos.last().expect("hay eventos");

        assert_eq!(
            final_del_evento.percentage, None,
            "sin un total declarado el porcentaje tiene que ser nulo: un porcentaje sin \
             denominador es una cifra inventada"
        );
        assert_eq!(
            final_del_evento.bytes_processed, 500,
            "sin total, los bytes procesados son los que se han leído, y siguen informados"
        );
        assert_eq!(final_del_evento.bytes_uploaded, 500);
        assert_eq!(fila.status, final_del_evento.status);
    }

    // ── La parada ────────────────────────────────────────────────────────────

    /// **15.** `parar` deja de aceptar trabajo y **espera lo acotado**.
    ///
    /// Con un abridor que dura más que el plazo, la parada tiene que volver **dentro** de su plazo
    /// —no esperar a la copia— y dejar constancia de lo que no terminó. Después, `encolar` da el
    /// error del motor parado. Y al final se deja que la copia acabe por su cuenta: lo que la
    /// parada abandona no es la copia, es la **espera**; en producción el proceso puede acabar antes
    /// de que la fila se escriba, y eso es justo lo que el contrato admite.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn parar_deja_de_aceptar_trabajo_y_espera_lo_acotado() {
        // El abridor dura un segundo y la parada espera doscientos milisegundos: la copia **no**
        // puede terminar dentro del plazo, que es lo que se quiere medir.
        const ESPERA: Duration = Duration::from_millis(200);

        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let tarea = aparato.tarea_normal(target).await;

        let mut ajustes = Ajustes::nuevos(SOCKET_QUE_NO_EXISTE).con_abridor(abridor_que_no_termina);
        ajustes.espera_de_parada = ESPERA;
        let motor = aparato.motor_y_ajustes(ajustes);

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
        esperar_el_estado(&aparato, &ejecucion.id, Estado::Running).await;

        // El registro de la parada se emite desde **este** hilo, así que se captura aquí: el arnés
        // del registro elige el buffer por hilo, y el del trabajador es otro.
        let (buffer, guardia) = crate::test_support::capturar_log();

        let empezado = Instant::now();
        motor.parar(ESPERA);
        let tardado = empezado.elapsed();

        let registro = buffer.texto();
        drop(guardia);

        assert!(
            tardado < Duration::from_secs(2),
            "la parada no respetó su plazo: tardó {tardado:?}"
        );
        assert!(
            tardado >= ESPERA,
            "la parada volvió antes de que se le acabara el plazo: {tardado:?}"
        );
        assert!(
            registro.contains("plazo de parada"),
            "la parada no dejó constancia de la copia que no terminó: {registro}"
        );

        // Y después de parar, no se acepta trabajo nuevo.
        let error = motor
            .encolar(tarea)
            .await
            .expect_err("un motor parado no puede aceptar trabajo");
        assert!(
            matches!(error, ErrorDelMotor::MotorParado),
            "el error de un motor parado no es el que dice que está parado: {error:?}"
        );

        // La parada no se lleva la copia por delante: el trabajador sigue, y cuando su abridor
        // vuelve escribe el final de su fila.
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert_eq!(
            aparato.esperar_el_final(&ejecucion.id).await.status,
            Estado::Failed,
            "la copia abandonada por la parada no llegó a escribir su final"
        );
    }

    // ── El cierre del hub ────────────────────────────────────────────────────

    /// **16.** `cerrar_el_hub` termina el stream **aunque haya una ejecución en vuelo**.
    ///
    /// Es la mitad del arreglo del apagado que un hub de un solo dueño no necesitaría, y la que
    /// hacía falta de verdad: `EnVivo` —el estado de una ejecución en curso— guarda un clon del hub
    /// y publica por él, así que un extremo de envío por clon mantendría el canal abierto aunque el
    /// motor cerrara el suyo. El cuerpo del `text/event-stream` no terminaría, el cierre de `axum`
    /// esperaría a esa conexión y el proceso acabaría con el `SIGKILL` del período de gracia, con
    /// el hilo de trabajo a mitad de archivar.
    ///
    /// La copia se deja **en vuelo** a propósito —el abridor no termina dentro del plazo de
    /// parada—, así que su `EnVivo` existe en el momento del cierre: es el escenario que hace falta
    /// y no una aproximación. Lo que se mide es un `recv` de verdad, que es donde el flujo real
    /// está esperando: el `Closed` tiene que **despertarlo**.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cerrar_el_hub_termina_el_stream_aunque_haya_una_ejecucion_en_vuelo() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let tarea = aparato.tarea_normal(target).await;

        let motor = aparato.motor_con(abridor_que_no_termina);
        let mut cliente = motor
            .hub()
            .suscribir()
            .expect("el hub no está cerrado antes de la ejecución");

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
        esperar_el_estado(&aparato, &ejecucion.id, Estado::Running).await;

        motor.cerrar_el_hub();

        // La copia sigue viva: si hubiera terminado antes, este cierre no estaría midiendo el caso
        // del `EnVivo`, que es el único donde el clon del hub importa.
        assert_eq!(
            aparato.estado_en_la_base(&ejecucion.id).await,
            Estado::Running.texto(),
            "la copia terminó antes del cierre: esta prueba solo mide el caso del estado en vivo si \
             la ejecución sigue en vuelo cuando se cierra el hub"
        );

        // El desenlace se busca **hasta el `Closed`**, y no en una sola lectura: el receptor
        // entrega primero lo que tuviera en el búfer —los avisos del encolado y del paso a
        // `running`—, igual que el flujo real, que sigue leyendo hasta que el canal se cierra. Lo
        // que se mide es que ese `Closed` llega con la copia todavía en vuelo.
        let limite = Instant::now() + Duration::from_secs(5);

        let desenlace = loop {
            let restante = limite.saturating_duration_since(Instant::now());

            match tokio::time::timeout(restante, cliente.recv()).await {
                Ok(Ok(_)) => continue,
                Ok(Err(error)) => break error,
                Err(caducado) => panic!(
                    "el cierre del hub no despertó al cliente con una ejecución en vuelo dentro del \
                     plazo: el clon del hub que guarda su estado en vivo mantiene el canal abierto, \
                     el cuerpo del stream no termina y el apagado ordenado se queda esperándolo \
                     ({caducado})"
                ),
            }
        };

        assert!(
            matches!(desenlace, RecvError::Closed),
            "el cliente del stream no vio su canal cerrado, sino {desenlace:?}"
        );

        // Y el cliente que llegue después del cierre tampoco tiene stream: el estado está en el
        // tipo, no en una suscripción que ya no puede servir nada.
        assert!(
            motor.hub().suscribir().is_none(),
            "un hub cerrado siguió dando receptores"
        );
    }

    // ── El final de una ejecución que no llega a empezar ─────────────────────

    /// **Una ejecución que no puede pasar a `running` acaba en `failed`, y su tarea se puede
    /// volver a disparar.**
    ///
    /// Es uno de los dos hallazgos bloqueantes de la auditoría del motor: un fallo de escritura al
    /// pasar la fila a `running` la dejaba en `pending` **sin `finished_at` y sin motivo**, y esa
    /// fila huérfana no es un apunte inofensivo. La guarda del `409` cuenta como «en curso» toda
    /// ejecución en `pending` **o** `running`, así que el disparo de esa tarea se quedaba en un
    /// `409` para siempre —el panel la pinta «en cola» indefinidamente— y la única salida era
    /// borrar la fila a mano.
    ///
    /// El fallo se fuerza con un **disparador de SQLite** que aborta solo las actualizaciones que
    /// intentan dejar la fila en `running`: el paso a `running` falla de verdad, que es el fallo de
    /// escritura que se quiere medir, y la escritura del final sigue funcionando, que es el caso en
    /// el que el remedio tiene que servir. El caso extremo —que tampoco se pueda escribir el
    /// `failed`— está documentado en el rustdoc de `ejecucion::ejecutar` y no se puede provocar
    /// desde una prueba sin dejar la base inservible para el resto de la medición.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn una_ejecucion_que_no_puede_empezar_acaba_en_failed_y_no_bloquea_su_tarea() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let tarea = aparato.tarea_normal(target).await;

        sqlx::query(
            "CREATE TRIGGER bloquea_el_running BEFORE UPDATE ON job_executions \
             FOR EACH ROW WHEN NEW.status = 'running' \
             BEGIN SELECT RAISE(ABORT, 'el paso a `running` está bloqueado por la prueba'); END",
        )
        .execute(&aparato.pool)
        .await
        .expect("el disparador de la prueba se crea");

        let motor = aparato.motor_con(abridor_local);
        let ejecucion = motor
            .encolar(tarea)
            .await
            .expect("el alta tiene que entrar");

        let fila = aparato.esperar_el_final(&ejecucion.id).await;

        assert_eq!(
            fila.status,
            Estado::Failed,
            "la fila se quedó en {:?} con `finished_at` {:?}: una ejecución que no puede empezar \
             tiene que acabar en `failed`, o su tarea no se puede volver a disparar y el panel \
             miente diciendo que está en cola",
            fila.status,
            fila.finished_at
        );
        assert!(
            fila.finished_at.is_some(),
            "una ejecución que acaba tiene que llevar su `finished_at`, y ésta no lo llevaba"
        );
        let motivo = fila.error_message.clone().unwrap_or_default();
        assert!(
            motivo.contains("`running`"),
            "el motivo tiene que decir que no se pudo pasar a `running`: {motivo}"
        );

        // Y la tarea vuelve a entrar, que es la mitad que importa: es lo que el `409` eterno
        // impedía.
        let segunda = motor.encolar(tarea).await.expect(
            "la tarea tiene que poder volver a dispararse después de una ejecución que no empezó",
        );
        assert_eq!(segunda.status, Estado::Pending);
        assert_eq!(
            aparato.esperar_el_final(&segunda.id).await.status,
            Estado::Failed
        );
    }

    // ── La cola llena ────────────────────────────────────────────────────────

    /// **La cola llena da un `503` con nombre propio, y su fila no se queda mintiendo.**
    ///
    /// La capacidad es de [`CAPACIDAD_DE_LA_COLA`](super::cola::CAPACIDAD_DE_LA_COLA) ejecuciones y
    /// el trabajador atiende de una en una en el orden en que se encoló, así que llenarla son 65
    /// que caben —una en la mano y 64 esperando— y una **sexagésimo sexta** que no cabe. Se miden
    /// las dos mitades del contrato: que el encolado que no cabe devuelve
    /// [`ErrorDelMotor::ColaDelRepositorioLlena`], y que la fila que el alta había creado para él
    /// acaba en `failed` con su `finished_at` —`Motor::encolar` la marca antes de devolver el error
    /// «para que no quede mintiendo»—, y no en `pending`.
    ///
    /// Hacen falta tareas **distintas** porque la guarda del `409` no deja encolar dos veces la
    /// misma, y el abridor se atasca solo la primera vez: al soltarlo, las demás fallan en el acto y
    /// el trabajador vuelve a su recepción.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_cola_llena_da_un_error_con_nombre_y_su_fila_no_se_queda_en_pending() {
        let mut aparato = Aparato::nuevo().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let motor = aparato.motor_con(abridor_que_se_atasca_la_primera);

        // El trabajador se queda dentro del primer trabajo, y la cola empieza a llenarse.
        let primera = aparato.tarea_normal(target).await;
        let mut encoladas = vec![motor.encolar(primera).await.expect("tiene que entrar")];
        esperar_a(
            &ATASCADO_EN_EL_ABRIDOR,
            "que el trabajador se atasque dentro del abridor",
        )
        .await;

        for _ in 0..CAPACIDAD_DE_LA_COLA {
            let tarea = aparato.tarea_normal(target).await;
            encoladas.push(
                motor
                    .encolar(tarea)
                    .await
                    .expect("tiene que caber en la cola"),
            );
        }

        assert_eq!(
            aparato.filas_en(Estado::Pending).await,
            CAPACIDAD_DE_LA_COLA as i64,
            "la cola no quedó llena de verdad, así que el encolado de abajo no mediría lo que dice"
        );

        // Y la que no cabe:
        let de_mas = aparato.tarea_normal(target).await;
        let error = motor
            .encolar(de_mas)
            .await
            .expect_err("con la cola llena, la siguiente no puede entrar");

        assert!(
            matches!(
                error,
                ErrorDelMotor::ColaDelRepositorioLlena { repository_id }
                    if repository_id == aparato.repositorio
            ),
            "el error de una cola llena no es el que la nombra: {error:?}"
        );

        // La fila que el alta creó para esa ejecución se marcó como fallida: no se queda «en cola».
        let (estado, finished_at): (String, Option<String>) =
            sqlx::query_as("SELECT status, finished_at FROM job_executions WHERE job_id = ?1")
                .bind(de_mas.hyphenated().to_string())
                .fetch_one(&aparato.pool)
                .await
                .expect("la fila de la ejecución que no cupo tiene que existir");

        assert_eq!(
            estado, "failed",
            "la fila de la ejecución que no cupo se quedó en `{estado}`: el panel la pintaría \
             esperando una copia que nadie va a hacer"
        );
        assert!(
            finished_at.is_some(),
            "la fila de la ejecución que no cupo no llevaba `finished_at`"
        );

        // Y se suelta el trabajador, para que no quede nada en vuelo al acabar la prueba. La
        // última de la cola es la última que se atiende —el orden es el de encolado—, así que
        // esperarla es esperarlas todas.
        PUEDE_SOLTARSE_EL_ABRIDOR.store(true, Ordering::SeqCst);

        let ultima = encoladas.last().expect("la prueba encoló al menos una");
        assert_eq!(
            aparato.esperar_el_final(&ultima.id).await.status,
            Estado::Failed
        );
    }

    // ── La serialización y el repositorio que se editó entre medias ──────────

    /// **La copia va al repositorio con el que se encoló, aunque la tarea se edite mientras
    /// espera.**
    ///
    /// Es el otro hallazgo medio de la auditoría y la mitad que faltaba del requisito «un worker
    /// secuencial por repositorio»: la ejecución se encolaba en el buzón de un repositorio —leído al
    /// encolar— y la copia se hacía con el repositorio que la tarea tuviera al ejecutar, que es una
    /// fila **editable** (`PUT /api/v1/jobs/:id`) y una ventana que puede durar horas si hay cola.
    /// Con las dos lecturas separadas, una tarea editada entre medias copiaba contra un repositorio
    /// distinto del que eligió el trabajador que la atiende, y dos copias podían coincidir en el
    /// mismo repositorio **sin que nadie las hubiera serializado** —lo que `rustic_core` no admite:
    /// toma un bloqueo exclusivo—.
    ///
    /// La ventana se ensancha **sin cronometrar nada**: el abridor se queda parado apuntando el
    /// destino que recibió, y la tarea se edita mientras el trabajador está dentro. Se afirman las
    /// dos cosas: que el destino que recibió el abridor es el del encolado, y que la copia acabó
    /// **ahí** —una instantánea en el repositorio de la cola, ninguna en el otro—.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_copia_va_al_repositorio_del_encolado_aunque_la_tarea_se_edite_entre_medias() {
        let mut aparato = Aparato::nuevo().await;
        let al_lado = aparato.segundo_destino().await;
        let target = aparato.target_de_directorio(&[("uno.txt", "uno")]).await;
        let repositorio = aparato.repositorio;
        let tarea = aparato.tarea(target, repositorio, (7, 4, 12), true).await;
        let motor = aparato.motor_con(abridor_que_apunta_el_destino_y_espera_la_senal);

        let _ = DESTINO_ESPERADO.set(aparato.destino.to_string_lossy().into_owned());

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");

        // El trabajador está dentro del abridor: la cola ya eligió su repositorio.
        esperar_a(&DENTRO_DEL_ABRIDOR, "que el trabajador entre en el abridor").await;

        // Y ahora la tarea se apunta al otro repositorio, que es lo que hace `PUT /jobs/:id`.
        aparato.apuntar_la_tarea_a(&tarea, al_lado).await;

        PUEDE_SEGUIR_EL_ABRIDOR.store(true, Ordering::SeqCst);

        let fila = aparato.esperar_el_final(&ejecucion.id).await;

        assert_eq!(
            fila.status,
            Estado::Success,
            "la copia tenía que acabar bien y acabó en {:?}: {:?}",
            fila.status,
            fila.error_message
        );
        assert!(
            EL_DESTINO_ERA_EL_ESPERADO.load(Ordering::SeqCst),
            "el abridor recibió el destino {:?} y el que decidió la serialización era {:?}: la \
             copia fue a un repositorio que nadie serializó",
            DESTINO_RECIBIDO.get(),
            DESTINO_ESPERADO.get()
        );
        assert_eq!(
            aparato.instantaneas(),
            1,
            "la instantánea no está en el repositorio que decidió la serialización"
        );
    }

    // ── La fachada: abrir y listar fuera del runtime ─────────────────────────

    /// **`abrir_repositorio` inicializa el destino que no está y conecta con el que está.**
    ///
    /// Es la operación que la capa HTTP necesita para el alta —que conecta o inicializa, y tiene que
    /// poder decir cuál de los dos—, y se recorre **como la va a llamar el manejador**: desde un
    /// `async`, con el repositorio inyectado y con el acceso construido desde su fila. Lo que se
    /// mide es el camino entero —el `async` de la fachada, el trabajo fuera del runtime, el abridor
    /// y la vuelta— y no una función interna.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn abrir_repositorio_inicializa_el_que_no_esta_y_conecta_con_el_que_esta() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_local);

        let primera = motor
            .abrir_repositorio(aparato.acceso_de(&aparato.repositorio).await)
            .await
            .expect("un destino vacío y alcanzable se inicializa");

        assert_eq!(
            primera,
            PuestaEnMarcha::Inicializado,
            "un destino sin repositorio se inicializa, y dijo otra cosa"
        );

        let segunda = motor
            .abrir_repositorio(aparato.acceso_de(&aparato.repositorio).await)
            .await
            .expect("un destino que ya tiene repositorio se abre");

        assert_eq!(
            segunda,
            PuestaEnMarcha::Conectado,
            "un destino que ya tiene repositorio se conecta, y dijo otra cosa"
        );
    }

    /// **`listar_snapshots` lee del repositorio y respeta la ventana.**
    ///
    /// Es la otra mitad que necesita la capa HTTP —el explorador del §5 lee del **repositorio**, no
    /// del histórico de ejecuciones—, y lo que fija es que sale de ahí y que la ventana acota.
    ///
    /// Las dos cosas se afirman en **una sola** llamada, y no es un ahorro de líneas: en el
    /// repositorio hay tres instantáneas sembradas y en `job_executions` no hay **ninguna** fila,
    /// así que pedir dos y recibir dos dice a la vez que la lista sale del repositorio —de la base
    /// habría salido vacía— y que la ventana acota. Y los resúmenes traen lo que el panel pinta.
    ///
    /// Se pide una sola vez porque cada llamada abre y cierra un repositorio, y abrirlo cuesta
    /// segundos: medido, `init` y `open` tardan **6,4 s** cada uno en la compilación de pruebas —es
    /// el coste de derivar la clave del repositorio—, y una prueba que llama dos veces paga dos
    /// veces por lo mismo.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn listar_snapshots_lee_del_repositorio_y_respeta_la_ventana() {
        let aparato = Aparato::nuevo().await;
        sembrar_snapshots(
            &aparato.destino,
            PASSPHRASE_DE_PRUEBA,
            &[
                "2026-01-01T10:00:00Z",
                "2026-01-02T10:00:00Z",
                "2026-01-03T10:00:00Z",
            ],
        );
        let motor = aparato.motor_con(abridor_local);

        let devueltas = motor
            .listar_snapshots(aparato.acceso_de(&aparato.repositorio).await, 2)
            .await
            .expect("el repositorio sembrado se lista");

        assert_eq!(
            devueltas.len(),
            2,
            "el repositorio tiene tres instantáneas y `job_executions` está vacía: pedir dos y \
             recibir {} dice a la vez que la ventana acota y de dónde sale la lista",
            devueltas.len()
        );

        // Y cada resumen trae lo que el panel pinta —identificador, fecha, rutas, etiqueta y
        // resumen—, ordenado de la más reciente a la más antigua, que es lo que el módulo promete.
        for resumen in &devueltas {
            assert!(
                !resumen.id.is_empty() && !resumen.rutas.is_empty() && resumen.etiqueta.is_some(),
                "el resumen no trae lo que pide el panel: {resumen:?}"
            );
        }
        assert!(
            devueltas.windows(2).all(|par| par[0].fecha >= par[1].fecha),
            "el listado no viene de la más reciente a la más antigua: {:?}",
            devueltas
                .iter()
                .map(|resumen| resumen.fecha)
                .collect::<Vec<_>>()
        );
    }

    /// **`tiene_la_instantanea` resuelve el identificador contra el repositorio de verdad.**
    ///
    /// Es la lectura que hace que el `404` de la restauración sea de verdad, y lo que se mide aquí
    /// es la mitad que solo se puede medir con un repositorio abierto: que la comprobación mire
    /// **dónde debe** —el almacenamiento del repositorio de la fila, con el abridor del motor y
    /// fuera del runtime—, y no en la fila ni en el histórico de ejecuciones.
    ///
    /// El vínculo está sembrado en la base y la instantánea en el repositorio, así que los dos
    /// desenlaces se afirman sobre el mismo motor: la que está dice que sí y una bien formada que no
    /// existe dice que no. Con la lista de identificadores en la mano y la fila apuntando al
    /// repositorio, lo que se descarta es un «sí» por haber visto cualquier cosa.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tiene_la_instantanea_resuelve_contra_el_repositorio() {
        let aparato = Aparato::nuevo().await;
        let siembra = sembrar_snapshots(
            &aparato.destino,
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );
        let sembrada = siembra.identificadores[0].clone();
        let motor = aparato.motor_con(abridor_local);

        let acceso = aparato.acceso_de(&aparato.repositorio).await;

        assert!(
            motor
                .tiene_la_instantanea(acceso, &sembrada)
                .await
                .expect("el repositorio sembrado se deja preguntar"),
            "la instantánea `{sembrada}` está en el repositorio de la fila y la comprobación dijo \
             que no: el `404` de la restauración se lo llevaría una copia que sí está"
        );

        assert!(
            !motor
                .tiene_la_instantanea(
                    aparato.acceso_de(&aparato.repositorio).await,
                    &"f".repeat(64)
                )
                .await
                .expect("preguntar por una que no está no es un fallo del repositorio"),
            "una instantánea bien formada que no está en el repositorio tiene que dar `false`, y \
             dio `true`: la restauración prometería un trabajo contra una copia ausente"
        );
    }

    /// **Un repositorio que no se puede abrir no se confunde con una instantánea que no está.**
    ///
    /// La distinción es la del código HTTP: el repositorio que no responde es un `502` y la copia
    /// ausente un `404`, así que un «no está» inventado por un destino caído mandaría al operador a
    /// buscar una copia que quizá esté. Se mide con el abridor que falla siempre: el desenlace tiene
    /// que ser un error del motor con la causa de dentro, no un `Ok(false)`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resolver_una_instantanea_con_un_repositorio_que_no_abre_es_un_error() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_que_no_abre);

        let desenlace = motor
            .tiene_la_instantanea(
                aparato.acceso_de(&aparato.repositorio).await,
                &"f".repeat(64),
            )
            .await;

        assert!(
            matches!(desenlace, Err(ErrorDelMotor::Repositorio { .. })),
            "un repositorio que no se puede abrir no puede leerse como «esa instantánea no está»: \
             {desenlace:?}"
        );
    }

    /// **La restauración que no llega a término publica su aviso de cierre como `fallida`.**
    ///
    /// Es el hallazgo de auditoría que se perdía: publicando el cierre **dentro** del trabajo —en el
    /// hilo de bloqueo—, un pánico de la biblioteca se convierte en un `JoinError` y el cierre no
    /// llega a ejecutarse, así que no se publicaba nada y el panel se quedaba en «restaurando» para
    /// siempre. El instrumento es el abridor que entra en pánico a propósito: no abre nada de verdad
    /// y el hilo se cae, que es exactamente el desenlace que el cierre tiene que contar.
    ///
    /// Se afirman las **dos** fases y en orden: `restaurando` sale antes de tocar el repositorio
    /// —contrato del primer fotograma— y `fallida` sale al final, que es lo que esta corrección
    /// añade. Y los cuatro contadores siguen a cero en los dos: el aviso describe el empezó/terminó,
    /// no un avance.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn una_restauracion_que_se_cae_publica_el_cierre_como_fallida() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_que_panica);
        let mut receptor = motor
            .hub()
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        let execution_id = Uuid::new_v4();
        let instantanea = "f".repeat(64);

        let desenlace = motor
            .restaurar(
                aparato.acceso_de(&aparato.repositorio).await,
                instantanea.clone(),
                objetivo_de_directorio(&aparato.destino.to_string_lossy()),
                execution_id,
            )
            .await;

        assert!(
            matches!(desenlace, Err(ErrorDelMotor::TrabajoInterrumpido { .. })),
            "el pánico del hilo de trabajo tiene que llegar con nombre propio y no como un fallo \
             del repositorio: {desenlace:?}"
        );

        let avisos = recoger_restauraciones(&mut receptor);

        assert_eq!(
            avisos
                .iter()
                .map(|aviso| aviso.phase.as_str())
                .collect::<Vec<_>>(),
            vec![FASE_RESTAURANDO, FASE_FALLIDA],
            "el aviso de cierre tiene que publicarse aunque el trabajo se caiga, y con la fase \
             `fallida`: {avisos:?}"
        );

        for aviso in &avisos {
            assert_eq!(
                aviso.execution_id, execution_id,
                "el aviso no lleva la ejecución de la restauración: {aviso:?}"
            );
            assert_eq!(
                aviso.snapshot_id, instantanea,
                "el aviso no lleva la instantánea que se restauraba: {aviso:?}"
            );
            assert!(
                [
                    aviso.files_done,
                    aviso.files_total,
                    aviso.bytes_done,
                    aviso.bytes_total
                ]
                .iter()
                .all(|contador| *contador == 0),
                "en esta versión los cuatro contadores van a cero: {aviso:?}"
            );
        }
    }

    /// **La restauración que llega a término publica `terminada`, y el `Result` dice lo mismo.**
    ///
    /// Es la otra mitad del escenario de cierre, y la que ata el aviso al desenlace: lo que el
    /// stream publica y lo que devuelve la llamada tienen que contar lo mismo. Se hace contra un
    /// repositorio **de verdad** —el abridor local del arnés y una instantánea sembrada con la
    /// biblioteca—, porque restaurar de verdad es lo único que produce un `Ok`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn una_restauracion_que_llega_a_termino_publica_terminada() {
        let aparato = Aparato::nuevo().await;
        let siembra = sembrar_snapshots(
            &aparato.destino,
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );
        let instantanea = siembra.identificadores[0].clone();

        // El destino de la restauración, retenido: al soltarse se borra, y la restauración sigue
        // corriendo cuando la prueba ya está leyendo el aviso.
        let host = directorio_de_prueba();
        let motor = aparato.motor_con(abridor_local);
        let mut receptor = motor
            .hub()
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        let execution_id = Uuid::new_v4();

        let desenlace = motor
            .restaurar(
                aparato.acceso_de(&aparato.repositorio).await,
                instantanea,
                objetivo_de_directorio(&host.ruta().to_string_lossy()),
                execution_id,
            )
            .await;

        assert!(
            desenlace.is_ok(),
            "restaurar una instantánea que está en el repositorio no puede fallar: {desenlace:?}"
        );

        let avisos = recoger_restauraciones(&mut receptor);

        assert_eq!(
            avisos
                .iter()
                .map(|aviso| aviso.phase.as_str())
                .collect::<Vec<_>>(),
            vec![FASE_RESTAURANDO, FASE_TERMINADA],
            "una restauración que llega a término tiene que cerrar con `terminada`: {avisos:?}"
        );
    }

    /// **La restauración que llega a término deja su fila en `success` con su `finished_at`.**
    ///
    /// Es la mitad buena del ciclo de vida, y la única que se puede medir sin contenedor: un target
    /// de directorio restaura contra un repositorio **de verdad** —el abridor local del arnés y una
    /// instantánea sembrada—, así que el `Ok` es real y lo que se lee después es lo que el motor
    /// escribió en la base. La fila nace en `pending`, como la deja quien lanza la restauración, y lo
    /// que se afirma es que al final está en `success`, sin motivo de fallo y con su `finished_at`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_restauracion_que_llega_a_termino_deja_la_fila_en_success() {
        let aparato = Aparato::nuevo().await;
        let siembra = sembrar_snapshots(
            &aparato.destino,
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );
        let instantanea = siembra.identificadores[0].clone();

        let host = directorio_de_prueba();
        let motor = aparato.motor_con(abridor_local);

        let fila = aparato.alta_de_restauracion(&instantanea).await;
        assert_eq!(
            fila.status,
            Estado::Pending,
            "la fila tiene que nacer en `pending`, como la crea quien lanza la restauración"
        );

        let desenlace = motor
            .restaurar(
                aparato.acceso_de(&aparato.repositorio).await,
                instantanea,
                objetivo_de_directorio(&host.ruta().to_string_lossy()),
                fila.id,
            )
            .await;

        assert!(
            desenlace.is_ok(),
            "una restauración de directorio contra una instantánea sembrada no puede fallar: \
             {desenlace:?}"
        );

        let guardada = aparato.fila_de_restauracion(&fila.id).await;

        assert_eq!(
            guardada.status,
            Estado::Success,
            "la restauración llegó a término y su fila no quedó en `success`"
        );
        assert!(
            guardada.error_message.is_none(),
            "una restauración que llega a término no deja motivo de fallo: {guardada:?}"
        );
        assert!(
            guardada.finished_at.is_some(),
            "la fila de una restauración terminada tiene que llevar su `finished_at`: {guardada:?}"
        );
    }

    /// **La restauración que falla deja su fila en `failed` con el motivo, nunca en `running`.**
    ///
    /// Es la mitad que no se puede dejar al cuidado de nadie: una escritura final que solo ocurriera
    /// por el camino bueno dejaría la fila en `running` **para siempre** y el operador creería que su
    /// restauración sigue en curso. El instrumento es un abridor que no abre —el repositorio no
    /// responde—, que es un fallo del camino de directorio y no necesita contenedor; lo que se afirma
    /// es el estado final, que lleva motivo y que no es `running`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_restauracion_que_falla_deja_la_fila_en_failed_con_su_motivo() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_que_no_abre);

        let instantanea = "a".repeat(64);
        let host = directorio_de_prueba();

        let fila = aparato.alta_de_restauracion(&instantanea).await;

        let desenlace = motor
            .restaurar(
                aparato.acceso_de(&aparato.repositorio).await,
                instantanea,
                objetivo_de_directorio(&host.ruta().to_string_lossy()),
                fila.id,
            )
            .await;

        assert!(
            desenlace.is_err(),
            "un repositorio que no se puede abrir no puede dar una restauración buena"
        );

        let guardada = aparato.fila_de_restauracion(&fila.id).await;

        assert_eq!(
            guardada.status,
            Estado::Failed,
            "la restauración falló y su fila no quedó en `failed`: quedó en `{:?}`, y una fila en \
             `running` miente —dice que sigue en curso— para siempre",
            guardada.status
        );
        assert!(
            guardada.error_message.is_some(),
            "la fila de una restauración fallida tiene que llevar el motivo: {guardada:?}"
        );
        assert!(
            guardada.finished_at.is_some(),
            "la fila de una restauración fallida tiene que llevar su `finished_at`: {guardada:?}"
        );
    }

    /// **La rama de la restauración la decide el tipo del objetivo, y solo el tipo.**
    ///
    /// Las dos mitades, y cada una descarta el error de la otra: un objetivo de **directorio** se
    /// restaura en el `host_path` —el fichero sembrado aparece ahí— sin pasar por ningún contenedor,
    /// y un objetivo de **base** no escribe **nada** en el `host_path` —aunque se le ponga uno—,
    /// porque sale por el contenedor. El primero usa el abridor local y una instantánea sembrada, así
    /// que su éxito es real; el segundo falla por el contenedor, que es lo que se espera de un socket
    /// que no existe.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_rama_de_la_restauracion_la_decide_el_tipo_del_objetivo() {
        let aparato = Aparato::nuevo().await;
        let siembra = sembrar_snapshots(
            &aparato.destino,
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );
        let instantanea = siembra.identificadores[0].clone();
        let camino_del_volcado = siembra.nodo.clone();

        let motor = aparato.motor_con(abridor_local);

        // 1. Un directorio: el volcado acaba en el host, y no se habla con ningún contenedor.
        let host = directorio_de_prueba();

        let desenlace = motor
            .restaurar(
                aparato.acceso_de(&aparato.repositorio).await,
                instantanea.clone(),
                objetivo_de_directorio(&host.ruta().to_string_lossy()),
                Uuid::new_v4(),
            )
            .await;

        assert!(
            desenlace.is_ok(),
            "un target de directorio se restaura en el host y no necesita contenedor: {desenlace:?}"
        );
        assert!(
            host.ruta().join(&camino_del_volcado).exists(),
            "un target de directorio tiene que escribir en su `host_path`, y el fichero sembrado no \
             está en `{}`",
            host.ruta().display()
        );

        // 2. Una base: **no** toca el host, y falla por el contenedor.
        let host_de_la_base = directorio_de_prueba();
        let mut objetivo = objetivo_mariadb("contenedor-que-no-existe", None);
        objetivo.host_path = Some(host_de_la_base.ruta().to_string_lossy().into_owned());

        let desenlace = motor
            .restaurar(
                aparato.acceso_de(&aparato.repositorio).await,
                instantanea,
                objetivo,
                Uuid::new_v4(),
            )
            .await;

        assert!(
            matches!(desenlace, Err(ErrorDelMotor::Restauracion { .. })),
            "un target de base sale por el contenedor: sin uno que exista tiene que fallar como tal \
             y no como la rama de directorio: {desenlace:?}"
        );
        assert!(
            std::fs::read_dir(host_de_la_base.ruta())
                .expect("el host de la base tiene que poder listarse")
                .next()
                .is_none(),
            "un target de base no escribe en el sistema de ficheros de Kesto, y `{}` tiene ficheros",
            host_de_la_base.ruta().display()
        );
    }

    /// **Dos restauraciones sobre el mismo destino de base no caben, y destinos distintos sí.**
    ///
    /// Es la guarda del §3 extendida a la base: el sujeto de la reserva es el destino —el contenedor
    /// junto con la base, o el `db_path` de un `sqlite`—, y no la instantánea. Lo que se afirma, en
    /// una sola prueba, es que la clave que separa dos destinos de base nombra lo que tiene que
    /// nombrar: el mismo contenedor y la misma base colisionan, un contenedor distinto no, una base
    /// distinta del mismo contenedor tampoco, y dos tipos de servidor en el mismo contenedor **no**
    /// son el mismo destino —son dos servidores—.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_reserva_de_un_destino_de_base_excluye_la_segunda_y_admite_otro_destino() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_que_no_abre);

        let mismo_destino = objetivo_mariadb("contenedor-de-prueba", Some("tienda"));
        let otro_contenedor = objetivo_mariadb("otro-contenedor", Some("tienda"));
        let otra_base = objetivo_mariadb("contenedor-de-prueba", Some("otra"));
        let otro_servidor = objetivo_postgresql("contenedor-de-prueba", Some("tienda"));

        let clave = destino_del_objetivo(&mismo_destino)
            .expect("el objetivo de una base nombra su destino");

        let reserva = motor
            .reservar_el_destino(&clave)
            .expect("el primer destino de base está libre");

        assert!(
            matches!(
                motor.reservar_el_destino(
                    &destino_del_objetivo(&mismo_destino).expect("el objetivo nombra su destino")
                ),
                Err(ErrorDelMotor::RestauracionEnCurso { .. })
            ),
            "dos restauraciones sobre el mismo contenedor y la misma base no pueden caber a la vez"
        );

        for (nombre, objetivo) in [
            ("un contenedor distinto", &otro_contenedor),
            ("otra base del mismo contenedor", &otra_base),
            ("otro servidor en el mismo contenedor", &otro_servidor),
        ] {
            let clave = destino_del_objetivo(objetivo).expect("el objetivo nombra su destino");

            assert!(
                motor.reservar_el_destino(&clave).is_ok(),
                "{nombre} tiene que ser un destino distinto y entrar"
            );
        }

        drop(reserva);

        assert!(
            motor.reservar_el_destino(&clave).is_ok(),
            "soltar la reserva tiene que dejar el destino de base libre"
        );
    }

    /// **La reserva de un destino es exclusiva por destino, y se suelta al soltarse.**
    ///
    /// Es la guarda de la tercera corrección: dos restauraciones sobre el mismo `host_path` no caben
    /// —escribirían a la vez en el mismo directorio—, y la guarda es **por destino** y no global, así
    /// que dos targets distintos siguen restaurándose a la vez. Lo segundo se mide en la misma prueba
    /// porque es lo que descarta una guarda que excluyera de más.
    ///
    /// Y lo que se libera al soltar la guarda es lo que hace que un `409` hable de una restauración
    /// que está corriendo **ahora**: si no se soltara, el destino quedaría reservado para siempre.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_reserva_de_un_destino_excluye_la_segunda_y_se_suelta_al_soltarse() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_que_no_abre);

        let destino = "/destino/de/la/restauracion";
        let otro_destino = "/destino/de/otra/restauracion";

        let reserva = motor
            .reservar_el_destino(destino)
            .expect("el primer destino está libre");

        assert!(
            matches!(
                motor.reservar_el_destino(destino),
                Err(ErrorDelMotor::RestauracionEnCurso { .. })
            ),
            "dos restauraciones sobre el mismo destino no pueden caber a la vez"
        );

        let otra_reserva = motor
            .reservar_el_destino(otro_destino)
            .expect("la guarda es por destino, y otro destino tiene que entrar");

        drop(reserva);

        assert!(
            motor.reservar_el_destino(destino).is_ok(),
            "soltar la reserva tiene que dejar el destino libre"
        );

        drop(otra_reserva);
    }

    /// **Dos formas de escribir el mismo destino colisionan, y el `Drop` libera la forma anotada.**
    ///
    /// Es el menor que encontró la re-auditoría: la guarda es un conjunto de claves de texto, así
    /// que `"/data"` y `"/data/"` —el mismo directorio— **no** coincidían y dos restauraciones podían
    /// escribir a la vez sobre el mismo sitio. Se mide con un directorio **real** y no con una ruta
    /// inventada porque la rama buena de la normalización es `canonicalize`, que solo habla de lo
    /// que existe.
    ///
    /// Lo segundo que se afirma es la mitad que no se puede olvidar: el `Drop` tacha la forma
    /// **normalizada**, que es la que se anotó, y por eso hay que volver a poder reservar el mismo
    /// directorio escrito **de la otra manera**. Un `Drop` que tachara el texto crudo dejaría la
    /// clave normalizada dentro para siempre y ese target no volvería a restaurarse.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_reserva_normaliza_el_destino_y_dos_formas_del_mismo_directorio_colisionan() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_que_no_abre);

        let directorio = directorio_de_prueba();
        let sin_barra = directorio.ruta().to_string_lossy().into_owned();
        let con_barra = format!("{sin_barra}/");

        assert_ne!(
            sin_barra, con_barra,
            "la prueba necesita dos textos distintos del mismo directorio"
        );

        let reserva = motor
            .reservar_el_destino(&sin_barra)
            .expect("el primer destino está libre");

        assert!(
            matches!(
                motor.reservar_el_destino(&con_barra),
                Err(ErrorDelMotor::RestauracionEnCurso { .. })
            ),
            "`{sin_barra}` y `{con_barra}` son el mismo directorio: la segunda reserva no puede \
             entrar, porque dos restauraciones escribirían a la vez en el mismo sitio"
        );

        drop(reserva);

        assert!(
            motor.reservar_el_destino(&con_barra).is_ok(),
            "soltar la reserva tachó otra clave que la que se anotó: el destino normalizado se \
             quedó reservado para siempre"
        );
    }

    /// **La normalización textual cubre el destino que todavía no existe.**
    ///
    /// Es la otra rama de [`normalizar_el_destino`], y la de este caso: la restauración es quien
    /// crea el directorio, así que en el momento de reservar `canonicalize` falla y lo único que
    /// queda es recortar las barras finales. Lo que se afirma es que la clave sale de todos modos
    /// normalizada —las dos formas colisionan— y que la raíz no se confunde con la cadena vacía.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_reserva_normaliza_tambien_un_destino_que_no_existe() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_que_no_abre);

        let directorio = directorio_de_prueba();
        let destino = directorio.ruta().join("todavia-no-existe");
        let sin_barra = destino.to_string_lossy().into_owned();
        let con_barra = format!("{sin_barra}/");

        assert!(
            !destino.exists(),
            "esta prueba mide la rama de la normalización textual: {sin_barra} no puede existir"
        );

        let reserva = motor
            .reservar_el_destino(&sin_barra)
            .expect("el destino que no existe todavía está libre");

        assert!(
            matches!(
                motor.reservar_el_destino(&con_barra),
                Err(ErrorDelMotor::RestauracionEnCurso { .. })
            ),
            "la barra de más tiene que colisionar también cuando el directorio no existe todavía"
        );

        drop(reserva);

        assert_eq!(
            normalizar_el_destino("/"),
            "/",
            "la raíz recortada no es la cadena vacía: no se puede perder"
        );
        assert_eq!(
            normalizar_el_destino(&format!("{sin_barra}//")),
            sin_barra,
            "la rama textual recorta las barras finales sobrantes"
        );
    }

    /// **La reserva se suelta aunque el trabajo que la retiene entre en pánico.**
    ///
    /// Es la mitad que no se puede dejar al cuidado de nadie: una liberación explícita no se ejecuta
    /// cuando el futuro que lleva la reserva se desmonta por un pánico —ni por una cancelación—, y
    /// el destino se quedaría reservado **para siempre**, así que ese target no volvería a
    /// restaurarse hasta reiniciar el proceso. Se reproduce el reparto que hace el manejador: la
    /// reserva viaja **dentro** del trabajo y el trabajo entra en pánico a propósito.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn una_reserva_se_suelta_aunque_el_trabajo_entre_en_panico() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_que_no_abre);

        let destino = "/destino/que/se/cae";
        let reserva = motor
            .reservar_el_destino(destino)
            .expect("el destino está libre");

        let del_trabajo = motor.clone();
        let caido = tokio::spawn(async move {
            let _reserva = reserva;

            panic!("el trabajo de esta prueba se cae a propósito: la reserva no puede quedarse");
        })
        .await;

        assert!(
            caido.is_err(),
            "el pánico del trabajo tiene que llegar como un pánico del task"
        );

        assert!(
            del_trabajo.reservar_el_destino(destino).is_ok(),
            "el destino se quedó reservado después de un pánico en el trabajo: ese target no \
             volvería a restaurarse hasta reiniciar el proceso"
        );
    }

    /// **Abrir un repositorio de S3 desde el runtime no entra en pánico.**
    ///
    /// Es la prueba que justifica la fachada entera, y la que el CI no podía tener: el abridor es
    /// **el de producción** —el de S3, el que entra en pánico si se le llama desde dentro de un
    /// runtime—, la llamada sale de un `async` (como saldrá del manejador del alta) y el destino es
    /// un oyente local que rechaza. Medido: esa misma llamada desde dentro de un runtime panica con
    /// «`Cannot start a runtime from within a runtime`», así que lo que se mide aquí es que la
    /// fachada saca el trabajo del runtime antes de llamar a la biblioteca.
    ///
    /// El endpoint es un oyente local que contesta `403` y no un puerto muerto por lo que está
    /// medido en `src/test_support.rs::destino_de_s3_que_rechaza`: contra un puerto donde nadie
    /// escucha, los cinco reintentos con retroceso exponencial de la biblioteca hacen que el fallo
    /// tarde entre 41 y 54 segundos, y eso no lo debe pagar cada `cargo test`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn abrir_repositorio_de_s3_desde_el_runtime_no_entra_en_panico() {
        let aparato = Aparato::nuevo().await;
        let endpoint = destino_de_s3_que_rechaza();
        let repositorio = aparato
            .dar_de_alta_el_repositorio_con_endpoint("destino-que-rechaza", &endpoint)
            .await;

        // Los ajustes de **producción**: su abridor es el de S3 (`Repositorio::poner_en_marcha`).
        let motor = aparato.motor_y_ajustes(Ajustes::nuevos(SOCKET_QUE_NO_EXISTE));
        let acceso = aparato.acceso_de(&repositorio).await;

        let error = motor
            .abrir_repositorio(acceso)
            .await
            .expect_err("un destino que rechaza no pone nada en marcha");

        // El fallo llega **con su tipo y con su motivo depurado**: el tipo es lo que el manejador
        // mira para elegir el código —un destino inalcanzable no es una passphrase que no abre—, y
        // el motivo es lo que sale hacia el operador.
        let (causa, motivo) = match error {
            ErrorDelMotor::Repositorio { causa, motivo } => (causa, motivo),
            otro => panic!(
                "el fallo de la biblioteca tiene que llegar como `ErrorDelMotor::Repositorio` y \
                 llegó como {otro:?}"
            ),
        };

        assert!(
            matches!(causa, Error::DestinoNoAlcanzable { .. }),
            "el destino que rechaza tiene que llegar como un fallo de destino y no como una \
             passphrase que no abre: {causa:?}"
        );
        assert!(
            motivo.contains(&endpoint),
            "el motivo tiene que nombrar el destino al que no se llegó: {motivo}"
        );
        assert!(
            !motivo.contains(CLAVE_SECRETA_DE_PRUEBA) && !motivo.contains(PASSPHRASE_DE_PRUEBA),
            "el motivo de la fachada publica un secreto del repositorio: {motivo}"
        );
    }

    // ── El pánico del hilo de trabajo ────────────────────────────────────────

    /// **Un pánico del hilo de trabajo no se propaga: llega como un error del motor.**
    ///
    /// Es la otra mitad de «sacar el trabajo del runtime», y la que hace que la fachada se pueda
    /// usar desde un manejador sin que un pánico de dentro de la biblioteca —que es código que este
    /// crate no controla— se lleve por delante la petición: el hilo de bloqueo recoge el pánico y lo
    /// devuelve como [`ErrorDelMotor::TrabajoInterrumpido`], con un motivo propio que **no** publica
    /// el texto del pánico —es arbitrario y acabaría en una respuesta—. Un `join` con un `unwrap`
    /// habría matado al que preguntó.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_panico_del_hilo_de_trabajo_llega_como_error_y_no_se_propaga() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_que_panica);
        let acceso = aparato.acceso_de(&aparato.repositorio).await;

        let error = motor
            .abrir_repositorio(acceso)
            .await
            .expect_err("un hilo que se cae con un pánico no puede dar un repositorio abierto");

        let motivo = match error {
            ErrorDelMotor::TrabajoInterrumpido { motivo } => motivo,
            otro => panic!(
                "el pánico del hilo de trabajo tiene que llegar como `TrabajoInterrumpido` y llegó \
                 como {otro:?}"
            ),
        };

        assert!(
            motivo.contains("pánico"),
            "el motivo tiene que decir que el hilo se cayó con un pánico: {motivo}"
        );
        assert!(
            !motivo.contains("se cae a propósito"),
            "el motivo publica el texto del pánico, que es arbitrario y no puede salir hacia \
             fuera: {motivo}"
        );
    }

    // ── El latido del motor de contenedores ──────────────────────────────────

    /// **`comprobar` dice que el motor de contenedores no contesta.**
    ///
    /// Es el punto de entrada que usa el arranque para **avisar** de un socket caído y el único que
    /// no tenía prueba. Lo que se fija es que el fallo llega como tal: el arranque decide con él qué
    /// contar y, sobre todo, **no** decide con él si el proceso arranca —un socket que no responde
    /// no puede dejar al operador sin la herramienta con la que diagnosticarlo—. Se mide con un
    /// socket que no existe, así que no hace falta ningún motor de contenedores.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn comprobar_dice_que_el_motor_de_contenedores_no_contesta() {
        let aparato = Aparato::nuevo().await;
        let motor = aparato.motor_con(abridor_local);

        let error = motor
            .comprobar()
            .await
            .expect_err("un socket que no existe no puede contestar al latido");

        assert!(
            matches!(error, ErrorDelMotor::MotorDeContenedores { .. }),
            "el fallo del motor de contenedores no llega como tal: {error:?}"
        );
    }

    // ── Con motor de contenedores: `cargo test -- --ignored` ─────────────────

    /// El binario del volcado que se instala dentro del contenedor de prueba.
    ///
    /// El motor construye el comando a partir del tipo de target —`mariadb-dump …`— y no hay forma
    /// de inyectarle otro, así que lo que se instala es **ese** binario: un guion que hace lo que la
    /// prueba necesita. Se escribe con `printf` porque el contenedor de prueba es un `alpine`
    /// pelado, y la escritura entra como un solo argumento de `sh -c`, así que no hay comillas de
    /// shell por fuera.
    async fn instalar_el_binario_del_volcado(contenedor: &str, escritura: &str) {
        let cliente = crate::test_support::cliente_del_motor()
            .expect("estas pruebas necesitan un motor de contenedores");

        let orden = crate::docker::Orden {
            cmd: vec!["sh".to_owned(), "-c".to_owned(), escritura.to_owned()],
            env: Vec::new(),
        };

        let codigo = cliente
            .ejecutar(
                contenedor,
                &orden,
                crate::test_support::TIMEOUT_DEL_MOTOR,
                |_trozo| {},
            )
            .await
            .expect("el alta del binario dentro del contenedor no puede fallar");

        assert_eq!(codigo, 0, "el guion que prepara el contenedor falló");
    }

    /// **El camino de un volcado de punta a punta**, con un contenedor de verdad.
    ///
    /// Es la única prueba que recorre el `PorContenedor` entero: el `exec`, el drenaje asíncrono, el
    /// productor en su hilo, el canal acotado, el archivado y las tres comprobaciones de la
    /// condición de éxito. El binario del volcado es un guion que escribe `seq 1 200000`, así que lo
    /// que tiene que quedar dentro de la instantánea son esos bytes, y la cuenta de bytes procesados
    /// tiene que cuadrar con ellos.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn el_motor_copia_un_volcado_de_punta_a_punta() {
        const GUION: &str = r##"printf "#!/bin/sh\nseq 1 200000\n" > /usr/local/bin/mariadb-dump; chmod +x /usr/local/bin/mariadb-dump"##;

        let contenedor = crate::test_support::ContenedorDePrueba::nuevo(
            "el_motor_copia_un_volcado_de_punta_a_punta",
        )
        .await;
        instalar_el_binario_del_volcado(contenedor.nombre(), GUION).await;

        let aparato = Aparato::nuevo().await;
        let target = aparato
            .target_de_contenedor(contenedor.nombre(), None)
            .await;
        let tarea = aparato.tarea_normal(target).await;

        let socket = crate::test_support::socket_del_motor()
            .expect("estas pruebas necesitan un socket de motor de contenedores");
        let motor = aparato.motor_y_ajustes(Ajustes::nuevos(socket).con_abridor(abridor_local));

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
        let fila = esperar_el_final_con(
            &aparato.dependencias.ejecuciones,
            &ejecucion.id,
            PLAZO_DEL_CONTENEDOR,
        )
        .await;

        assert_eq!(
            fila.status,
            Estado::Success,
            "la copia de un volcado de verdad tenía que acabar bien: {:?}",
            fila.error_message
        );
        let instantanea = fila
            .snapshot_id
            .clone()
            .expect("una copia buena deja su instantánea");

        // El contenido que `seq 1 200000` escribe, byte a byte.
        let esperado: Vec<u8> = (1..=200_000)
            .map(|numero| format!("{numero}\n"))
            .collect::<String>()
            .into_bytes();

        let guardado = volcado_guardado(
            &aparato.destino,
            PASSPHRASE_DE_PRUEBA,
            &instantanea,
            "tienda.sql",
        );

        assert_eq!(
            guardado.as_deref(),
            Some(esperado.as_slice()),
            "lo que quedó dentro de la instantánea no es lo que el contenedor escribió"
        );
        assert_eq!(
            fila.bytes_processed,
            esperado.len() as i64,
            "los bytes procesados no son los que salieron del `exec`"
        );

        contenedor.retirar().await;
    }

    /// **El `error_message` no lleva la contraseña**, ni siquiera cuando el contenedor la repite.
    ///
    /// Es el caso extremo del §4.A con un contenedor de verdad: la credencial viaja por el entorno
    /// del `exec` —nunca en la línea de comandos—, el proceso la escribe en su canal de error al
    /// fallar, y el campo donde acabaría se muestra en el panel y se guarda en la base.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn el_motivo_de_un_fallo_que_repite_la_contrasena_no_la_publica() {
        /// La contraseña de la prueba: distinguible y con el prefijo que el marcador delata.
        const CONTRASENA: &str = "Zq7-clave-de-base-distinguible-motor";

        const GUION: &str = r##"printf "#!/bin/sh\nprintf 'no puedo con %%s' \"\$MYSQL_PWD\" >&2\nexit 1\n" > /usr/local/bin/mariadb-dump; chmod +x /usr/local/bin/mariadb-dump"##;

        let contenedor = crate::test_support::ContenedorDePrueba::nuevo(
            "el_motivo_de_un_fallo_que_repite_la_contrasena",
        )
        .await;
        instalar_el_binario_del_volcado(contenedor.nombre(), GUION).await;

        let aparato = Aparato::nuevo().await;
        let target = aparato
            .target_de_contenedor(contenedor.nombre(), Some(CONTRASENA))
            .await;
        let tarea = aparato.tarea_normal(target).await;

        let socket = crate::test_support::socket_del_motor()
            .expect("estas pruebas necesitan un socket de motor de contenedores");
        let motor = aparato.motor_y_ajustes(Ajustes::nuevos(socket).con_abridor(abridor_local));

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
        let fila = esperar_el_final_con(
            &aparato.dependencias.ejecuciones,
            &ejecucion.id,
            PLAZO_DEL_CONTENEDOR,
        )
        .await;

        assert_eq!(
            fila.status,
            Estado::Failed,
            "un contenedor que sale con uno no puede dar una copia buena"
        );
        let motivo = fila.error_message.clone().unwrap_or_default();

        assert!(
            !motivo.contains(CONTRASENA),
            "el motivo que se guarda publica la contraseña de la base: {motivo}"
        );
        for longitud in 1..=CONTRASENA.len() {
            assert!(
                !motivo.contains(&CONTRASENA[..longitud]),
                "el motivo guarda los primeros {longitud} caracteres de la contraseña: {motivo}"
            );
        }
        assert!(
            motivo.contains(crate::secret::REDACTED),
            "el motivo no dice que haya algo redactado: {motivo}"
        );
        assert!(
            motivo.contains('1'),
            "el motivo no dice con qué código salió el comando de extracción: {motivo}"
        );
        assert!(
            fila.finished_at.is_some(),
            "una ejecución fallida tiene que llevar su `finished_at`"
        );

        contenedor.retirar().await;
    }

    /// **Una copia que no se da por buena no deja su instantánea en el repositorio.**
    ///
    /// Es el RED del cableado de #52 y la única prueba que lo recorre de punta a punta con un
    /// contenedor de verdad: el archivado escribe su instantánea **antes** de que el motor pueda
    /// comprobar nada —vuelve `Ok` aunque el volcado no se entregara entero—, así que una copia que
    /// acaba en `Failed` tiene que **retirar** esa instantánea o deja un residuo que ninguna
    /// ejecución `success` reclama.
    ///
    /// El desenlace se fuerza con un `mariadb-dump` que sale con uno: el volcado no declara su
    /// final, `el_volcado_se_entrego` falla y el motor retira lo que el archivado escribió. Se
    /// comprueba en las dos direcciones: la fila queda en `Failed` con su motivo, **y** el
    /// repositorio no tiene ninguna instantánea. Lo segundo es lo que muerde contra la mutación: si
    /// el olvido se quitara, la instantánea seguiría ahí y esta prueba lo vería.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn una_copia_que_no_se_da_por_buena_no_deja_su_instantanea() {
        const GUION: &str = r##"printf "#!/bin/sh\nexit 1\n" > /usr/local/bin/mariadb-dump; chmod +x /usr/local/bin/mariadb-dump"##;

        let contenedor = crate::test_support::ContenedorDePrueba::nuevo(
            "una_copia_que_no_se_da_por_buena_no_deja_su_instantanea",
        )
        .await;
        instalar_el_binario_del_volcado(contenedor.nombre(), GUION).await;

        let aparato = Aparato::nuevo().await;
        let target = aparato
            .target_de_contenedor(contenedor.nombre(), None)
            .await;
        let tarea = aparato.tarea_normal(target).await;

        let socket = crate::test_support::socket_del_motor()
            .expect("estas pruebas necesitan un socket de motor de contenedores");
        let motor = aparato.motor_y_ajustes(Ajustes::nuevos(socket).con_abridor(abridor_local));

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
        let fila = esperar_el_final_con(
            &aparato.dependencias.ejecuciones,
            &ejecucion.id,
            PLAZO_DEL_CONTENEDOR,
        )
        .await;

        assert_eq!(
            fila.status,
            Estado::Failed,
            "un contenedor que sale con uno no puede dar una copia buena"
        );
        assert!(
            fila.error_message.is_some(),
            "una copia fallida tiene que dejar su motivo en la fila"
        );

        assert_eq!(
            aparato.instantaneas(),
            0,
            "la instantánea que el archivado escribió antes de decidir tenía que haberse retirado, \
             y el repositorio dice que le quedan {}",
            aparato.instantaneas()
        );

        contenedor.retirar().await;
    }

    /// **Y una copia cuyo contenedor no está tampoco deja su instantánea.**
    ///
    /// Es el **otro** desenlace de [`copiar_el_volcado`] que olvida la instantánea, y el que un
    /// volcado que sale con uno **no** cubre: cuando el propio productor termina con un error —aquí,
    /// el `exec` contra un contenedor que no existe—, el motor recibe el `Err` del hilo y no llega a
    /// consultar la entrega. El archivado ya escribió una instantánea vacía antes de que el motor
    /// pudiera decidir, así que también tiene que retirarse.
    ///
    /// Se mide contra el motor de contenedores del host porque hasta conectar con él hace falta el
    /// socket; no hace falta ningún contenedor de verdad, que es lo que la hace barata.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn una_copia_con_un_contenedor_que_no_esta_no_deja_su_instantanea() {
        let aparato = Aparato::nuevo().await;
        let target = aparato
            .target_de_contenedor("kesto-contenedor-que-no-existe", None)
            .await;
        let tarea = aparato.tarea_normal(target).await;

        let socket = crate::test_support::socket_del_motor()
            .expect("estas pruebas necesitan un socket de motor de contenedores");
        let motor = aparato.motor_y_ajustes(Ajustes::nuevos(socket).con_abridor(abridor_local));

        let ejecucion = motor.encolar(tarea).await.expect("tiene que entrar");
        let fila = esperar_el_final_con(
            &aparato.dependencias.ejecuciones,
            &ejecucion.id,
            PLAZO_DEL_CONTENEDOR,
        )
        .await;

        assert_eq!(
            fila.status,
            Estado::Failed,
            "un contenedor que no existe no puede dar una copia buena"
        );
        assert!(
            fila.error_message.is_some(),
            "una copia fallida tiene que dejar su motivo en la fila"
        );

        assert_eq!(
            aparato.instantaneas(),
            0,
            "la instantánea que el archivado escribió antes de decidir tenía que haberse retirado, \
             y el repositorio dice que le quedan {}",
            aparato.instantaneas()
        );
    }
}
