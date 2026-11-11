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

use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use time::OffsetDateTime;
use tokio::runtime::Handle;
use uuid::Uuid;

use crate::db::DbError;
use crate::db::entities::{
    AltaDeEjecucion, Ejecucion, Ejecuciones, Estado, Repositorios, Tareas, Targets,
};
use crate::docker::Cliente;
use crate::rustic::{Acceso, BarrasDelMotor, PuestaEnMarcha, Repositorio, ResumenDeSnapshot};

use cola::Cola;
pub use hub::{CAPACIDAD_DEL_HUB, EventoDeEjecucion, Hub};

pub use extraccion::{
    CAPACIDAD_DEL_CANAL, ErrorDeExtraccion, INACTIVIDAD_MAXIMA, Objetivo, Resultado, Ruta, ruta,
    volcar,
};

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

/// Las cuatro capas de acceso que el motor usa.
///
/// Existe para que el motor **no** tenga que nombrar [`crate::state::EstadoDeBase`]: `src/engine`
/// no importa `src/state`, y así el día que el `AppState` lleve el motor no hay un ciclo de
/// módulos. Quien lo construye es el arranque, con las cuatro capas del estado, y eso son cuatro
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
}

impl std::fmt::Debug for Dependencias {
    /// No publica nada de dentro: las cuatro capas llevan el pool y la clave de cifrado, y lo que
    /// un diagnóstico necesita saber es que están, no cómo son por dentro.
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
    /// Lo devuelven las dos operaciones que el motor presta sin ser una copia
    /// ([`Motor::abrir_repositorio`] y [`Motor::listar_snapshots`]): ahí no hay ejecución que
    /// apuntar, así que el fallo se cuenta a quien preguntó y no queda escrito en ninguna fila.
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
    #[error("el repositorio no ha respondido: {motivo}")]
    Repositorio {
        /// El fallo de la biblioteca, para quien tenga que **decidir** con él.
        #[source]
        causa: crate::rustic::Error,
        /// El mismo fallo, ya recortado y sin credenciales, para contarlo.
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
}

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use rustic_core::repofile::SnapshotId;
    use sqlx::SqlitePool;
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};
    use uuid::Uuid;

    use super::cola::CAPACIDAD_DE_LA_COLA;
    use super::hub::EventoDeEjecucion;
    use super::{Ajustes, Dependencias, ErrorDelMotor, Motor, ejecucion};
    use crate::db::entities::{
        AltaDeRepositorio, AltaDeTarea, AltaDeTarget, Ejecucion, Ejecuciones, Estado, Repositorios,
        Tareas, Targets, TipoDeTarget,
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
                ejecuciones: Arc::new(Ejecuciones::nuevo(pool.clone())),
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

    /// Vacía el receptor y devuelve los eventos que había.
    fn recoger(
        receptor: &mut tokio::sync::broadcast::Receiver<EventoDeEjecucion>,
    ) -> Vec<EventoDeEjecucion> {
        let mut eventos = Vec::new();

        loop {
            match receptor.try_recv() {
                Ok(evento) => eventos.push(evento),
                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                // Un cliente que se pierde eventos no se corta: sigue leyendo. Aquí no puede
                // pasar —se lee enseguida y son pocos—, pero el camino se escribe igual.
                Err(TryRecvError::Lagged(_)) => continue,
            }
        }

        eventos
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
}
