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
use crate::rustic::{Acceso, BarrasDelMotor, PuestaEnMarcha, Repositorio};

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
        if let Err(causa) =
            interior
                .cola
                .encolar(&interior.contexto, tarea.repository_id, ejecucion.clone())
        {
            ejecucion::fallar_si_no_termino(&interior.contexto, &ejecucion.id, &causa.to_string());

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use rustic_core::repofile::SnapshotId;
    use sqlx::SqlitePool;
    use tokio::sync::broadcast::error::TryRecvError;
    use uuid::Uuid;

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
        abrir_local, base_con_clave, directorio_de_prueba, sembrar_snapshots, snapshot_guardado,
        volcado_guardado,
    };

    /// Un plazo generoso para esperar a que una ejecución termine.
    ///
    /// Treinta segundos: los abridores de estas pruebas duermen a lo sumo tres, así que un fallo
    /// por este plazo es un cuelgue y no una prueba lenta.
    const PLAZO_DE_ESPERA: Duration = Duration::from_secs(30);

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
                repositorio: Uuid::nil(),
            };

            aparato.repositorio = aparato
                .dar_de_alta_el_repositorio("repositorio-de-prueba", &aparato.destino.clone())
                .await;

            aparato
        }

        /// Da de alta un repositorio cuyo `endpoint` es la ruta que se le diga.
        async fn dar_de_alta_el_repositorio(&self, nombre: &str, endpoint: &Path) -> Uuid {
            self.dependencias
                .repositorios
                .alta(AltaDeRepositorio {
                    name: nombre.to_owned(),
                    endpoint: endpoint.to_string_lossy().into_owned(),
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
        assert_eq!(
            aparato.estado_en_la_base(&ejecucion.id).await,
            "pending",
            "la ejecución no nace en `pending`"
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
        let mut receptor = motor.hub().suscribir();

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
        let mut receptor = motor.hub().suscribir();

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
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
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
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
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
