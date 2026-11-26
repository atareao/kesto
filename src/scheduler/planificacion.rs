//! El reloj y su sincronización con `backup_jobs`.
//!
//! Este fichero tiene las tres cosas que el módulo **decide**: qué está planificado, cada cuánto se
//! relee la base, y cuándo se deja de aceptar trabajo. Lo que no tiene es el disparo, que vive en
//! [`super::disparo`] porque es una llamada al motor y no una decisión del reloj.
//!
//! # La verdad es SQLite, y el reloj es efímero
//!
//! El reloj es **nativo**: un bucle de Tokio con un mapa de tareas en memoria y un despertador
//! calculado sobre `time::OffsetDateTime`. `backup_jobs` ya es el almacén, y duplicarlo abriría dos
//! verdades que se desincronizan sin dar error. La consecuencia se declara: al reiniciar no hay nada
//! planificado hasta que termina la planificación inicial, y esa ventana se cierra **antes** de
//! empezar a escuchar.
//!
//! # La sincronización es periódica: [`PERIODO_DE_RECONCILIACION`]
//!
//! Cada período, [`Planificador::reconciliar`] lee las tareas y ajusta lo que tiene planificado:
//! añade lo que falta, retira lo que sobra y reemplaza lo que cambió. No hay notificación desde la
//! API, y el motivo que decide es que la notificación **no cubre los cambios que no pasan por
//! ella** —una fila editada con `sqlite3`, una base restaurada—, que se quedarían con la
//! planificación vieja para siempre. El precio es la latencia acotada que el período declara, y ese
//! número se documenta **aquí y en un solo sitio**.
//!
//! # La tabla `job_id → mango`, que es la única autoridad
//!
//! `add()` devuelve un `Uuid` que es el mango de la tarea y `remove(&Uuid)` es lo único que la
//! retira; `remove` de un mango que no existe devuelve `Ok(())` y no hay forma de enumerar lo
//! planificado. Así que la tabla de este módulo es la única autoridad sobre qué está planificado, y
//! **hay que olvidar el mango viejo al reemplazar**: una tarea cuyo calendario cambia se retira por
//! su mango de entonces y se vuelve a añadir. Si el mango viejo se pierde sin retirarse, la
//! planificación duplicada **no da ningún error** —simplemente dispara dos veces—, y por eso la
//! tabla guarda el mango y no solo un booleano.
//!
//! # La puerta de parada: una bandera de un solo sentido, cerrada sin esperar candados
//!
//! Está medido que apagar el bucle puede dejar un tick disparando después, así que la garantía de
//! que no se encola tras la parada **no se delega en el bucle**: [`Interior::abierta`] es una bandera
//! atómica que [`Planificador::parar`] pone a `false` **antes de cualquier `await`**. El cierre no
//! puede depender de adquirir un candado dentro del plazo: si el candado [`Interior::estado`] está
//! tomado más que el plazo —un disparo que lo retiene durante `encolar` mientras una escritura
//! espera el `BUSY_TIMEOUT` de la base—, una puerta que solo se cerrara al adquirirlo se quedaría
//! abierta **para siempre**, y un tick posterior encolaría tras la parada.
//!
//! [`super::disparo`] consulta la bandera **dentro de la misma sección crítica que el encolado**
//! —el candado [`Interior::estado`]—, así que un encolado o termina antes del cierre o no empieza,
//! y eso cubre también el tick que el bucle pueda disparar de más. Y como la bandera solo pasa de
//! abierta a cerrada, un disparo que ya pasó el chequeo termina su encolado y ninguno nuevo
//! empieza. El plazo de [`Planificador::parar`] acota **el drenaje del módulo** —la espera al
//! disparo en vuelo y el apagado del bucle—, no el cierre de la puerta, que ya está hecho.
//!
//! # Por qué el reloj va detrás de un `Mutex` cuando nadie lo comparte
//!
//! Porque el bucle del reloj necesita `&mut self` para apagarse y la superficie de este módulo solo
//! presta `&self`. El módulo se queda el reloj nativo y le pasa **clones** a los cierres de disparo,
//! que es lo que hace que un disparo vea la planificación viva y que la parada alcance a todos.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sqlx::SqlitePool;
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::validacion::{Agenda, validar};
use crate::db::DbError;
use crate::db::entities::Tareas;
use crate::engine::Motor;

/// Cada cuánto se reconcilia la planificación con `backup_jobs`.
///
/// Es el número que acota la latencia entre un cambio en la base —el alta de una tarea, la edición
/// de su expresión, su borrado— y su efecto en el reloj, y se documenta **en un solo sitio**: aquí.
/// No es un ajuste de entorno porque el §8 no pide una variable nueva: es una constante declarada,
/// y [`Ajustes::con_periodo`] existe para que una prueba —o un despliegue con otra latencia
/// asumible— la cambie sin recompilar la constante.
///
/// Treinta segundos: bastante para no leer la tabla cada segundo por nada, y lo bastante corto para
/// que una tarea recién dada de alta no se quede esperando un ciclo largo.
pub const PERIODO_DE_RECONCILIACION: Duration = Duration::seconds(30);

/// El plazo que acota el drenaje del planificador al pararlo.
///
/// Es el número con el que [`Planificador::parar`] acota **su** drenaje —la reconciliación o el
/// disparo que estuviera en vuelo, bajo el candado que comparte con el encolado, y el apagado del
/// bucle—, no el cierre de la puerta, que ya está hecho. Se
/// declara **aquí y en un solo sitio**: el proceso lo lee de esta constante al montar el apagado
/// ordenado, y no de una variable de entorno, porque la parada del planificador no añade
/// configuración.
///
/// Dos segundos: lo que acota es un encolado —una inserción condicional en SQLite— o una lectura de
/// `backup_jobs`, así que vuelve muy por debajo de este número en cualquier máquina sana. Y es menor
/// que la espera de la parada del motor (cinco segundos por defecto) a propósito: las dos caben, una
/// detrás de otra —el reloj dentro del futuro de apagado y el motor después de `serve`—, dentro del
/// período de gracia de un `docker stop` (diez segundos). Igualar el plazo del motor dejaría el peor
/// caso pegado a ese límite, y acotar existe justo para no acabar en un `SIGKILL`.
pub const PLAZO_DE_PARADA: Duration = Duration::seconds(2);

/// Cómo se arranca el planificador y cada cuánto sincroniza.
///
/// Se llama [`Ajustes`] y no comparte nombre con [`crate::engine::Ajustes`] a propósito: son dos
/// tipos distintos —el del motor habla de sockets y de plazos de parada, el del reloj de un
/// período— y un nombre compartido obligaría a alias en cada sitio que los nombre a la vez.
#[derive(Clone)]
pub struct Ajustes {
    /// Cada cuánto se relee `backup_jobs` y se ajusta lo planificado.
    ///
    /// Es el período que [`PERIODO_DE_RECONCILIACION`] declara, salvo que quien lo construya lo
    /// cambie con [`Ajustes::con_periodo`].
    pub periodo: Duration,
}

impl Ajustes {
    /// Los ajustes de producción: el período de reconciliación declarado.
    ///
    /// Es **síncrona** y no habla con nadie: montar el planificador no abre el reloj todavía. Eso
    /// lo hace [`Planificador::arrancar`], que es donde se leen las tareas y se planifican.
    pub fn nuevos() -> Self {
        Self {
            periodo: PERIODO_DE_RECONCILIACION,
        }
    }

    /// Los mismos ajustes con otro período de reconciliación.
    pub fn con_periodo(mut self, periodo: Duration) -> Self {
        self.periodo = periodo;
        self
    }
}

/// Por qué el planificador no ha podido hacer su trabajo.
///
/// Solo tiene los dos desenlaces que **impiden** una operación. Una tarea cuya expresión no parsea
/// **no es un error de aquí**: se registra, se salta y las demás siguen, que es el requisito 2 y la
/// decisión de que la errata de una tarea no pueda convertirse en la ausencia de todas las copias.
#[derive(Debug, Error)]
pub enum ErrorDelPlanificador {
    /// El reloj nativo no ha podido arrancar, aceptar una tarea o pararse.
    ///
    /// El motivo es texto y **no** un tipo ajeno a este módulo a propósito: un tipo de la
    /// implementación del reloj en esta superficie pública ataría el borde a la implementación.
    #[error("el reloj del planificador no ha respondido: {motivo}")]
    Reloj {
        /// Qué dijo el reloj, ya pasado a texto.
        motivo: String,
    },

    /// La base no ha podido leerse para sincronizar la planificación.
    ///
    /// Es el desenlace de [`Planificador::reconciliar`] cuando la lectura de `backup_jobs` falla, y
    /// el de [`Planificador::arrancar`] cuando falla la planificación inicial.
    #[error("la base no ha podido leerse para planificar: {causa}")]
    Base {
        /// El fallo de la capa de acceso, tal cual.
        #[source]
        causa: DbError,
    },
}

/// Por qué una operación interna del reloj nativo no se ha podido confirmar.
///
/// No sale de este módulo: es el tipo que [`Estado::aplicar_retirada`] usa para decidir si una
/// entrada se queda en la tabla esperando un reintento.
#[derive(Debug, Error)]
pub(super) enum ErrorDelReloj {
    /// La operación no se pudo completar; el motivo se guarda como texto.
    ///
    /// La variante existe también en producción: el apagado del bucle puede fallar y el llamante
    /// tiene que poder distinguir un reloj que no confirmó su retirada de uno que sí lo hizo.
    #[error("el reloj nativo no ha podido completar la operación: {motivo}")]
    Operacion {
        /// La razón, ya convertida en texto.
        motivo: String,
    },
}

/// El cuerpo del disparo que el reloj llama en cada vencimiento.
///
/// Recibe el identificador de la tarea de Kesto y devuelve un futuro que se ejecuta en su propia
/// tarea de Tokio, para que un encolado lento no retrase el cálculo del siguiente vencimiento.
type Disparo = Arc<dyn Fn(Uuid) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Lo que el reloj sabe de un job planificado.
///
/// El `mango` es la clave del mapa y el identificador que la tabla [`Estado::planificadas`] guarda;
/// el `job_id` es el de la tarea de `backup_jobs` y es lo que se entrega al disparo.
struct JobInfo {
    /// La tarea de Kesto a la que pertenece este job.
    job_id: Uuid,
    /// La agenda validada, con la que se calcula cada vencimiento.
    agenda: Agenda,
    /// El cuerpo que se ejecuta al vencer.
    disparo: Disparo,
    /// El siguiente vencimiento ya calculado; `None` si la agenda no tiene ocurrencia siguiente.
    proxima_ejecucion: Option<OffsetDateTime>,
}

/// El reloj nativo: un mapa de jobs, un despertador y un bucle que los evalúa.
///
/// No guarda nada en disco: `backup_jobs` es la única verdad. El mapa vive tras un `Arc<Mutex<_>>`
/// para que el bucle y las operaciones de alta/baja compartan exactamente el mismo estado sin
/// copias.
pub(super) struct NativeScheduler {
    /// Los jobs vivos, por mango.
    jobs: Arc<Mutex<HashMap<Uuid, JobInfo>>>,
    /// Despierta al bucle cuando cambia el mapa o se ordena la parada.
    notify: Arc<Notify>,
    /// La bandera de parada propia del bucle.
    apagado: Arc<AtomicBool>,
    /// La tarea del bucle, para poder esperarla al apagar.
    tarea: Mutex<Option<JoinHandle<()>>>,
}

impl NativeScheduler {
    /// Construye un reloj vacío, aún sin bucle en marcha.
    fn nuevo() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            notify: Arc::new(Notify::new()),
            apagado: Arc::new(AtomicBool::new(false)),
            tarea: Mutex::new(None),
        }
    }

    /// Lanza el bucle que calcula vencimientos y dispara.
    async fn arrancar(&self) {
        let jobs = Arc::clone(&self.jobs);
        let notify = Arc::clone(&self.notify);
        let apagado = Arc::clone(&self.apagado);
        let tarea = tokio::spawn(async move {
            bucle_del_reloj(jobs, notify, apagado).await;
        });

        let mut guardia = self.tarea.lock().await;
        *guardia = Some(tarea);
    }

    /// Añade un job y devuelve su mango.
    async fn añadir(&self, job_id: Uuid, agenda: Agenda, disparo: Disparo) -> Uuid {
        let mango = Uuid::new_v4();
        // La próxima marca se calcula **al alta**, no en el primer tick del bucle: la tabla tiene
        // que poder responder por la próxima ejecución desde el mismo instante en que se crea.
        let proxima_ejecucion = agenda.proxima_tras(OffsetDateTime::now_utc());
        // El mango se inserta antes de despertar: el bucle no puede ver un aviso sin job.
        let mut jobs = self.jobs.lock().await;
        jobs.insert(
            mango,
            JobInfo {
                job_id,
                agenda,
                disparo,
                proxima_ejecucion,
            },
        );
        drop(jobs);
        // `notify_one` guarda un permiso: si el bucle todavía no ha registrado su `notified()`,
        // el aviso se consume en cuanto lo registre. `notify_waiters` solo avisa a los waiters
        // ya registrados y perdería este aviso.
        self.notify.notify_one();
        mango
    }

    /// Retira un job por su mango, sin fallar si ya no estaba.
    pub(super) async fn remove(&self, mango: &Uuid) -> Result<(), ErrorDelReloj> {
        let mut jobs = self.jobs.lock().await;
        if jobs.remove(mango).is_none() {
            tracing::debug!(
                %mango,
                "el mango que se retira ya no estaba en el reloj"
            );
        }
        drop(jobs);
        self.notify.notify_one();
        Ok(())
    }

    /// El siguiente vencimiento que el reloj tiene calculado para un mango.
    #[cfg(test)]
    async fn proxima_de(&self, mango: Uuid) -> Result<Option<OffsetDateTime>, ErrorDelReloj> {
        let jobs = self.jobs.lock().await;
        Ok(jobs.get(&mango).and_then(|info| info.proxima_ejecucion))
    }

    /// Apaga el bucle y espera a que termine.
    async fn apagar(&self) -> Result<(), ErrorDelReloj> {
        self.apagado.store(true, Ordering::SeqCst);
        self.notify.notify_one();

        let tarea = self.tarea.lock().await.take();
        if let Some(tarea) = tarea
            && let Err(error) = tarea.await
        {
            return Err(ErrorDelReloj::Operacion {
                motivo: error.to_string(),
            });
        }
        Ok(())
    }
}

/// El bucle del reloj nativo.
///
/// En cada vuelta calcula el vencimiento más próximo entre todos los jobs, ejecuta los que ya
/// vencieron en tareas propias y duerme hasta el siguiente —o hasta que un alta, una baja o la
/// parada lo despierte—. La bandera de parada se comprueba arriba y el despertar de la parada llega
/// por el mismo `Notify`, así que no hay ventana en la que el bucle siga durmiendo tras el cierre.
async fn bucle_del_reloj(
    jobs: Arc<Mutex<HashMap<Uuid, JobInfo>>>,
    notify: Arc<Notify>,
    apagado: Arc<AtomicBool>,
) {
    loop {
        if apagado.load(Ordering::SeqCst) {
            return;
        }

        let ahora = OffsetDateTime::now_utc();
        let (proximo, vencidas) = {
            let mut mapa = jobs.lock().await;
            let mut proximo: Option<OffsetDateTime> = None;
            let mut vencidas = Vec::new();

            for info in mapa.values_mut() {
                if info.proxima_ejecucion.is_none() {
                    info.proxima_ejecucion = info.agenda.proxima_tras(ahora);
                }

                if let Some(cuando) = info.proxima_ejecucion {
                    if cuando <= ahora {
                        vencidas.push((info.job_id, Arc::clone(&info.disparo)));
                        info.proxima_ejecucion = info.agenda.proxima_tras(cuando);
                    } else if proximo.is_none() || cuando < proximo.expect("hay próximo") {
                        proximo = Some(cuando);
                    }
                }
            }

            (proximo, vencidas)
        };

        for (job_id, disparo) in vencidas {
            tokio::spawn(disparo(job_id));
        }

        match proximo {
            Some(cuando) if cuando > ahora => {
                let resto = cuando - ahora;
                let plazo = std::time::Duration::from_nanos(resto.whole_nanoseconds() as u64);
                tokio::select! {
                    _ = tokio::time::sleep(plazo) => {}
                    _ = notify.notified() => {}
                }
            }
            Some(_) => {
                // Ya venció: otra vuelta lo ejecuta sin dormir.
            }
            None => {
                notify.notified().await;
            }
        }
    }
}

/// Convierte un plazo de `time` al tipo que espera `tokio::time`.
fn a_std(plazo: Duration) -> std::time::Duration {
    std::time::Duration::from_nanos(plazo.whole_nanoseconds() as u64)
}

/// El reloj del planificador: las tareas activas y su disparo.
///
/// Es **clonable y barato** —como [`Motor`]—, porque por dentro hay un [`Arc`]: clonarlo no vuelve
/// a leer la base ni a montar un reloj. El clon que viaja a cada cierre de disparo comparte estado
/// con el original, que es lo que hace que un disparo vea la planificación viva y que la parada
/// alcance a todos.
///
/// No entra en el `AppState`: ninguna ruta lo necesita, y meterlo obligaría a que cada prueba de
/// las rutas levantara un reloj. Su mango viaja por `main` hasta la parada ordenada, al lado del
/// motor.
#[derive(Clone)]
pub struct Planificador(pub(super) Arc<Interior>);

/// Lo que comparten el planificador y sus clones.
///
/// Es `pub(super)` y no público porque [`super::disparo`] —un módulo hermano— tiene que alcanzar el
/// motor y el estado del disparo, y que el cierre del reloj llame al cuerpo del disparo sin que este
/// módulo publique su interior hacia fuera.
pub(super) struct Interior {
    /// El motor: la única puerta por la que nace una ejecución, la misma del disparo manual.
    pub(super) motor: Motor,
    /// La capa de acceso de `backup_jobs`, sobre el pool migrado que también usa el motor.
    pub(super) tareas: Tareas,
    /// Cómo se arranca y cada cuánto reconcilia.
    pub(super) ajustes: Ajustes,
    /// El reloj nativo.
    ///
    /// Tras un candado porque el bucle del reloj necesita `&mut self` para apagarse y la superficie
    /// de aquí solo presta `&self`. Orden de cerrojo: siempre [`Interior::estado`] antes que este,
    /// para que un disparo que retira una tarea no se cruce con una reconciliación.
    pub(super) reloj: Mutex<NativeScheduler>,
    /// La puerta de parada: si el planificador sigue aceptando disparos.
    ///
    /// Es una bandera atómica y **no** un campo de [`Estado`], porque su cierre no puede depender de
    /// adquirir el candado de [`Interior::estado`]: [`Planificador::parar`] la pone a `false` **antes
    /// de cualquier `await`**, así que, venza o no el plazo de drenaje, la puerta queda cerrada.
    /// Es de **un solo sentido** —de abierta a cerrada, nunca al revés—, y eso es lo que hace que
    /// consultarla fuera del candado (para [`Planificador::esta_parado`]) y dentro de él (en
    /// [`super::disparo`], en la misma sección crítica que el encolado) sea seguro: un disparo que ya
    /// pasó el chequeo termina su encolado y ninguno nuevo empieza. Se cierra al empezar la parada
    /// —[`Planificador::parar`]— y cuando el motor deja de aceptar trabajo —[`super::disparo`], al
    /// ver `MotorParado`—.
    pub(super) abierta: AtomicBool,
    /// La tabla `job_id → mango`, bajo su candado.
    pub(super) estado: Mutex<Estado>,
}

impl Interior {
    /// Si la puerta de parada sigue abierta.
    pub(super) fn sigue_abierta(&self) -> bool {
        self.abierta.load(Ordering::SeqCst)
    }

    /// Cierra la puerta de parada. De un solo sentido: no hay reanudación.
    pub(super) fn cerrar_la_puerta(&self) {
        self.abierta.store(false, Ordering::SeqCst);
    }
}

/// La tabla de lo planificado.
///
/// La **puerta de parada** no vive aquí: está en [`Interior::abierta`], porque su cierre no puede
/// depender de adquirir este candado. Lo que sí comparten es el candado de [`Interior::estado`],
/// donde [`super::disparo`] consulta la puerta **en la misma sección crítica que el encolado** —así
/// un encolado o termina antes del cierre o no empieza— y donde la reconciliación ajusta la tabla.
pub(super) struct Estado {
    /// La tabla `job_id → mango` del reloj nativo.
    ///
    /// Es la **única autoridad** sobre lo planificado: el reloj no ofrece ninguna señal de estado
    /// utilizable —`remove` de un mango inexistente devuelve `Ok(())`—, así que el origen de
    /// verdad es esta tabla y el reloj solo la obedece.
    pub(super) planificadas: HashMap<Uuid, Uuid>,
    /// La expresión **normalizada** con la que se planificó cada tarea, por su identificador.
    ///
    /// No es una segunda autoridad sobre lo planificado —esa es [`Estado::planificadas`]—, es la
    /// memoria de **con qué** se planificó: el reloj no deja leer el calendario de una tarea
    /// planificada, así que sin esto no habría forma de distinguir «sigue igual» de «cambió la
    /// expresión» y la reconciliación reemplazaría a ciegas —o no reemplazaría nunca—. Guarda la
    /// forma normalizada, que es la que se le dio al reloj, no la que tecleó el usuario.
    pub(super) expresiones: HashMap<Uuid, String>,
}

impl Estado {
    /// Aplica a la tabla el desenlace de `quitar`, y devuelve el error si no se confirmó.
    ///
    /// # El invariante: nada sale de la tabla hasta que el reloj confirma
    ///
    /// La entrada de [`Estado::planificadas`] **no se quita antes** de retirar el mango del reloj.
    /// Quitarla primero y encontrarse un `remove` que falla perdería el único mango con el que esa
    /// tarea se puede retirar: seguiría planificada en el reloj —disparando— y ni la reconciliación
    /// ni la rama `Retirar` de [`super::disparo`] podrían encontrarla, porque buscan por `job_id` en
    /// esta tabla. Quitarla solo con el `Ok` convierte el fallo en algo **reintentable**: la entrada
    /// se queda con su mango y la siguiente pasada lo vuelve a intentar.
    ///
    /// Es un único sitio para las tres retiradas —la tarea que sobra, el reemplazo por cambio de
    /// expresión y el `TareaQueNoEsta` del disparo—, que es lo que impide que el invariante se
    /// aplique en una y se olvide en otra.
    pub(super) fn aplicar_retirada(
        &mut self,
        job_id: Uuid,
        desenlace: Result<(), ErrorDelReloj>,
    ) -> Result<(), ErrorDelReloj> {
        match desenlace {
            Ok(()) => {
                self.planificadas.remove(&job_id);
                self.expresiones.remove(&job_id);
                Ok(())
            }
            // No se toca la tabla: la entrada —y su mango— se queda para el reintento.
            Err(error) => Err(error),
        }
    }
}

impl Planificador {
    /// Planifica las tareas activas y arranca el reloj nativo.
    ///
    /// Lee `backup_jobs`, planifica cada tarea con `enabled = 1` y su expresión, y **no** planifica
    /// ninguna con `enabled = 0`. Una tarea cuya expresión no parsea se registra como un error que
    /// la nombra —con su identificador y con la expresión—, se salta, y no impide ni el arranque ni
    /// que las demás se planifiquen.
    ///
    /// Se llama **detrás** del motor: su única operación es llamarlo, y uno que arrancara antes
    /// tendría una ventana en la que el reloj dispara contra un motor que todavía no acepta
    /// trabajo.
    ///
    /// # Errors
    ///
    /// [`ErrorDelPlanificador::Base`] si la base no se puede leer, y
    /// [`ErrorDelPlanificador::Reloj`] si el reloj no se puede arrancar o añadir una tarea.
    pub async fn arrancar(
        motor: Motor,
        base: SqlitePool,
        ajustes: Ajustes,
    ) -> Result<Self, ErrorDelPlanificador> {
        // El reloj nativo, sin persistencia: sus tareas viven en memoria y el almacén es
        // `backup_jobs`.
        let reloj = NativeScheduler::nuevo();

        let planificador = Self(Arc::new(Interior {
            motor,
            tareas: Tareas::nuevo(base),
            ajustes,
            reloj: Mutex::new(reloj),
            abierta: AtomicBool::new(true),
            estado: Mutex::new(Estado {
                planificadas: HashMap::new(),
                expresiones: HashMap::new(),
            }),
        }));

        // La planificación inicial, con el mismo código que la pasada periódica: cierra la ventana
        // entre «nada planificado» y «todo planificado» **antes** de que el reloj empiece a tickar,
        // para no encontrarse esa ventana abierta.
        planificador.sincronizar().await?;

        // Y el reloj, a andar. Se arranca después de planificar: un reloj arrancado antes tendría
        // una ventana en la que dispara contra un motor que todavía no acepta trabajo.
        planificador.0.reloj.lock().await.arrancar().await;

        // Y constancia de que el reloj está en marcha: es la marca con la que se afirma, en el
        // registro del proceso, que el planificador quedó vivo **antes** de empezar a escuchar.
        tracing::info!(
            tareas = planificador.planificadas().len(),
            "planificador en marcha"
        );

        Ok(planificador)
    }

    /// **El cuerpo** de una pasada de sincronización: lee las tareas y ajusta la planificación.
    ///
    /// Es lo que comparten [`Planificador::arrancar`] —que propaga cualquier desenlace— y
    /// [`Planificador::reconciliar`] —que convierte un fallo del reloj en un aviso y sigue—, para
    /// que la pasada inicial y la periódica no puedan divergir.
    async fn sincronizar(&self) -> Result<(), ErrorDelPlanificador> {
        let tareas = self
            .0
            .tareas
            .listar()
            .await
            .map_err(|causa| ErrorDelPlanificador::Base { causa })?;

        // La puerta primero, el reloj después: es el orden de cerrojo que declara
        // [`Interior::reloj`], y con él un disparo que retira una tarea no se cruza con esta pasada.
        let mut estado = self.0.estado.lock().await;

        // La puerta, que ya no vive en `estado` sino en la bandera atómica: si está cerrada, no se
        // planifica nada.
        if !self.0.sigue_abierta() {
            return Ok(());
        }

        // Lo que **tiene que** estar planificado: solo las tareas activas cuya expresión se
        // entiende. Una expresión que no parsea se registra nombrando tarea y expresión —es lo
        // único que la distingue en un registro con veinte tareas— y se salta: la errata de una
        // tarea no puede convertirse en la ausencia de todas las copias.
        let mut deseadas: HashMap<Uuid, Agenda> = HashMap::new();

        for tarea in &tareas {
            if !tarea.enabled {
                continue;
            }

            match validar(&tarea.cron_expression) {
                Ok(agenda) => {
                    deseadas.insert(tarea.id, agenda);
                }
                Err(error) => {
                    tracing::error!(
                        job_id = %tarea.id,
                        cron_expression = %tarea.cron_expression,
                        motivo = %error.motivo(),
                        "la expresión de la tarea no se puede interpretar: se salta su planificación"
                    );
                }
            }
        }

        let mut reloj = self.0.reloj.lock().await;

        // 1. Retirar lo que sobra y lo que **cambió**. Se recorre una copia de las claves para
        //    poder mutar la tabla mientras se itera.
        let planificadas: Vec<Uuid> = estado.planificadas.keys().copied().collect();

        for job_id in planificadas {
            match deseadas.get(&job_id) {
                // Ya no se desea: la tarea se desactivó o se borró.
                None => {
                    // Se lee el mango **sin quitarlo**: solo sale de la tabla si el reloj confirma.
                    if let Some(mango) = estado.planificadas.get(&job_id).copied()
                        && let Err(error) =
                            estado.aplicar_retirada(job_id, reloj.remove(&mango).await)
                    {
                        tracing::warn!(
                            job_id = %job_id,
                            %error,
                            "el mango de una tarea que sobra no se ha podido quitar del reloj: se \
                             reintentará en la siguiente pasada"
                        );
                    }
                }
                // Sigue deseada: solo se reemplaza si la expresión cambió.
                Some(agenda) => {
                    let igual = estado
                        .expresiones
                        .get(&job_id)
                        .is_some_and(|con_la_que_esta| con_la_que_esta == agenda.normalizada());

                    if !igual {
                        // **El mango viejo se retira antes de volver a añadir**: si se perdiera sin
                        // quitarse, la planificación duplicada no daría ningún error —dispararía dos
                        // veces—. Y solo se quita de la tabla si el reloj confirma: si el `remove`
                        // falla, la entrada se queda —paso 2 verá el `job_id` todavía en la tabla y
                        // **no** añadirá la nueva planificación— y la siguiente pasada lo reintenta.
                        if let Some(mango_viejo) = estado.planificadas.get(&job_id).copied()
                            && let Err(error) =
                                estado.aplicar_retirada(job_id, reloj.remove(&mango_viejo).await)
                        {
                            tracing::warn!(
                                job_id = %job_id,
                                %error,
                                "el mango viejo no se ha podido quitar del reloj al reemplazar la \
                                 planificación: no se planifica la nueva hasta confirmarlo, y se \
                                 reintentará en la siguiente pasada"
                            );
                        }
                    }
                }
            }
        }

        // 2. Añadir lo que falta —lo nuevo y lo que se acaba de retirar para reemplazar—.
        let pendientes: Vec<Uuid> = deseadas.keys().copied().collect();
        for job_id in pendientes {
            if estado.planificadas.contains_key(&job_id) {
                continue;
            }

            let agenda = deseadas
                .remove(&job_id)
                .expect("el job_id salió de las claves de `deseadas`");
            let normalizada = agenda.normalizada().to_owned();
            let mango = self.planificar_una(&mut reloj, job_id, agenda).await?;
            estado.planificadas.insert(job_id, mango);
            estado.expresiones.insert(job_id, normalizada);
        }

        Ok(())
    }

    /// Monta el job de una tarea y lo añade al reloj, devolviendo su mango.
    ///
    /// El cuerpo del disparo es **una sola línea** —[`Planificador::disparar`]—; se prueba el
    /// cuerpo y no el cierre, porque un vencimiento real del calendario no se puede esperar en una
    /// prueba.
    ///
    /// # El identificador con el que dispara el cierre es el de Kesto, capturado
    ///
    /// El `Uuid` que el reloj entrega al cierre es ahora el `job_id` de la tarea de Kesto, que es
    /// lo único que el disparo necesita. Tratar el mango del reloj como `job_id` fue el fallo real
    /// que llevaba cada tick hasta `TareaQueNoEsta` sin crear nunca una ejecución. Por eso el
    /// identificador de verdad viaja **guardado en el job** y se entrega al disparo al vencer.
    async fn planificar_una(
        &self,
        reloj: &mut NativeScheduler,
        job_id: Uuid,
        agenda: Agenda,
    ) -> Result<Uuid, ErrorDelPlanificador> {
        // El clon que viaja al cierre comparte el estado: un disparo ve la planificación viva y la
        // parada lo alcanza.
        let planificador = self.clone();

        let disparo: Disparo = Arc::new(move |id| {
            let planificador = planificador.clone();
            Box::pin(async move {
                planificador.disparar(id).await;
            })
        });

        // La expresión que se le da al reloj es la **normalizada**: la misma que la validación dio
        // por buena, para que lo validado y lo evaluado sean la misma cadena. Ya viene dentro de
        // `agenda`, construida por `validar`.
        Ok(reloj.añadir(job_id, agenda, disparo).await)
    }

    /// **Una sola pasada** de reconciliación: lo que la vigilancia periódica llama.
    ///
    /// Se expone para que las pruebas fuercen una pasada sin esperar un período real, y para que el
    /// arranque cierre la ventana entre «nada planificado» y «todo planificado» con el mismo código
    /// que la pasada periódica.
    ///
    /// Añade lo que falta, retira lo que sobra y reemplaza lo que cambió —incluido un cambio de
    /// `cron_expression`, que se reemplaza **retirando antes el mango viejo**—. Si el planificador
    /// ya está parado, no hace nada.
    ///
    /// # Errors
    ///
    /// [`ErrorDelPlanificador::Base`] si la lectura de `backup_jobs` falla.
    pub async fn reconciliar(&self) -> Result<(), ErrorDelPlanificador> {
        match self.sincronizar().await {
            Ok(()) => Ok(()),
            // El fallo de base **sí** se cuenta: el requisito solo declara ese desenlace para esta
            // operación, y quien la vigila decide qué hacer con él.
            Err(ErrorDelPlanificador::Base { causa }) => Err(ErrorDelPlanificador::Base { causa }),
            // Un fallo del reloj no se propaga: la reconciliación es periódica y el siguiente
            // período volverá a intentarlo; caerse por él dejaría al reloj sin sincronizar para
            // siempre por un fallo pasajero.
            Err(error @ ErrorDelPlanificador::Reloj { .. }) => {
                tracing::warn!(
                    %error,
                    "el reloj del planificador ha fallado al reconciliar: se reintentará en el \
                     siguiente período"
                );
                Ok(())
            }
        }
    }

    /// Lanza la reconciliación periódica.
    ///
    /// La llama `main` **después** de [`Planificador::arrancar`], nunca desde él: así
    /// [`Planificador::reconciliar`] sigue siendo una sola pasada que una prueba puede forzar, y la
    /// vigilancia es solo el envoltorio que la repite cada [`Ajustes::periodo`].
    pub fn vigilar(&self) {
        let planificador = self.clone();
        let periodo = a_std(self.0.ajustes.periodo);

        tokio::spawn(async move {
            let mut intervalo = tokio::time::interval(periodo);

            // El primer tick de `interval` es inmediato; se consume sin reconciliar porque
            // [`Planificador::arrancar`] ya cerró la ventana inicial con una pasada.
            intervalo.tick().await;

            loop {
                intervalo.tick().await;

                if planificador.esta_parado() {
                    return;
                }

                if let Err(error) = planificador.reconciliar().await {
                    tracing::warn!(
                        %error,
                        "la reconciliación periódica no ha podido leer la base"
                    );
                }
            }
        });
    }

    /// La parada, idempotente, de un solo sentido y **siempre acotada**.
    ///
    /// Lo primero que hace es **cerrar la puerta** —la bandera atómica [`Interior::abierta`]—, y lo
    /// hace **antes de cualquier `await`**: el cierre no puede depender de adquirir un candado dentro
    /// del plazo. Si el candado [`Interior::estado`] está tomado más que `plazo` —un disparo que lo
    /// retiene durante `encolar` mientras una escritura espera el `BUSY_TIMEOUT` de la base—, una
    /// puerta que solo se cerrara al adquirirlo se quedaría abierta para siempre, y los ticks
    /// posteriores encolarían tras la parada. Cerrada aquí, no se planifica ni se encola nada nuevo
    /// a partir de ese instante.
    ///
    /// Después **drena** lo que el módulo tuviera en vuelo: espera al disparo en vuelo adquiriendo el
    /// candado [`Interior::estado`] y toma el candado del reloj para apagarlo. Las **dos** esperas
    /// están acotadas por `plazo`; lo que no termine dentro de él se registra y el apagado continúa,
    /// porque un `docker stop` no puede convertirse en una espera indefinida que el orquestador
    /// resuelva con un `SIGKILL`.
    ///
    /// El plazo acota **el drenaje del módulo**, no el cierre —ya garantizado—: el apagado del bucle
    /// también se ejecuta dentro de él, para que la parada esté acotada de principio a fin.
    pub async fn parar(&self, plazo: Duration) {
        // **Lo primero: cerrar la puerta, antes de cualquier `await`.** Un solo sentido: de abierta a
        // cerrada y nunca al revés, porque el bucle no se reanuda. A partir de aquí, cualquier
        // disparo que llegue —incluido el vencimiento que el bucle pueda disparar tras el apagado—
        // encuentra la puerta cerrada y no encola.
        self.0.cerrar_la_puerta();
        let plazo_std = a_std(plazo);

        // Y ahora, el drenaje acotado: adquirir el candado espera al disparo que estuviera en vuelo,
        // de modo que `parar` no vuelve con un encolado a medias. Si vence, se registra y se
        // continúa: la puerta ya está cerrada, que es la garantía que importa.
        match tokio::time::timeout(plazo_std, self.0.estado.lock()).await {
            Ok(_guardia) => {}
            Err(_) => {
                tracing::warn!(
                    plazo = ?plazo,
                    "el planificador no ha drenado su disparo en vuelo dentro del plazo: se apaga \
                     igualmente (la puerta ya está cerrada)"
                );
            }
        }

        // Y el reloj nativo, con **el mismo plazo**: se apaga dentro del plazo, de modo que ni el
        // candado ni la espera al bucle puedan alargar la parada; si vence, se registra y se
        // continúa, porque la puerta ya está cerrada.
        match tokio::time::timeout(plazo_std, async {
            let reloj = self.0.reloj.lock().await;
            reloj.apagar().await
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(%error, "el reloj nativo no se ha apagado limpiamente");
            }
            Err(_) => {
                tracing::warn!(
                    plazo = ?plazo,
                    "el apagado del reloj no ha terminado dentro del plazo: se continúa con el \
                     apagado (la puerta ya está cerrada)"
                );
            }
        }

        // Y la constancia de la parada, para que el apagado ordenado sea legible en el registro: es
        // la marca con la que se afirma que el reloj se paró **antes** que el motor.
        tracing::info!("planificador parado");
    }

    /// Las tareas que el planificador tiene planificadas, por su identificador.
    ///
    /// Es lo que este módulo puede observar de lo planificado, porque el reloj **no** ofrece
    /// ninguna forma de enumerar sus tareas: el único mango es el `Uuid` que devolvió `add`, y vive
    /// en la tabla [`Estado::planificadas`].
    pub fn planificadas(&self) -> Vec<Uuid> {
        // Lectura **sin esperar**: la tabla vive tras un candado asíncrono y esta firma es síncrona.
        // Si el candado está tomado —un disparo o una reconciliación en vuelo—, no se puede leer y se
        // devuelve lo conservador: nada. Los llamantes del proceso nunca observan el candado tomado;
        // la prueba, tampoco. La **puerta**, en cambio, no se lee de aquí: es la bandera atómica de
        // [`Interior::abierta`], y [`Planificador::esta_parado`] la lee sin candado.
        match self.0.estado.try_lock() {
            Ok(estado) => {
                let mut ids: Vec<Uuid> = estado.planificadas.keys().copied().collect();
                ids.sort();
                ids
            }
            Err(_) => Vec::new(),
        }
    }

    /// Si el planificador ha dejado de aceptar trabajo.
    ///
    /// Es lo que hace observable «dejar de planificar»: al empezar la parada, o al ver el
    /// planificador que el motor está parado, la puerta se cierra y esto pasa a ser verdadero. Un
    /// solo sentido: no hay reanudación.
    ///
    /// Es **síncrono y fiable**: lee la bandera atómica [`Interior::abierta`], sin tomar ningún
    /// candado, así que dice la verdad aunque haya un disparo o una reconciliación en vuelo. El
    /// `try_lock` de antes devolvía `false` con el candado tomado —«mientras trabaja no está
    /// parado»—, que era una segunda autoridad de la puerta y además mentía justo en el caso que
    /// importaba: con la parada vencida por el plazo, el bucle de [`Planificador::vigilar`] no
    /// habría terminado nunca.
    pub fn esta_parado(&self) -> bool {
        !self.0.sigue_abierta()
    }

    /// Los mangos vivos en el reloj, uno por tarea planificada.
    ///
    /// Es un gancho **interno** —`pub(super)`, no superficie pública— para que la prueba del
    /// reemplazo pueda comprobar que un cambio de expresión no acumula mangos. El reloj no deja
    /// enumerar sus tareas, así que lo único que se puede afirmar es lo que la tabla autoriza. Solo
    /// lo usa la prueba —el proceso no tiene por qué enumerar mangos—, así que se compila con las
    /// pruebas.
    #[cfg(test)]
    pub(super) fn mangos(&self) -> Vec<Uuid> {
        match self.0.estado.try_lock() {
            Ok(estado) => {
                let mut mangos: Vec<Uuid> = estado.planificadas.values().copied().collect();
                mangos.sort();
                mangos
            }
            Err(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use time::Duration;
    use uuid::Uuid;

    use super::Planificador;
    use crate::scheduler::pruebas::{Aparato, CRON_DE_CINCO};
    use crate::test_support::capturar_log;

    /// El plazo del drenaje de una parada de prueba.
    ///
    /// Es acotado y con nombre, como todos los plazos del repositorio: la parada acota **su**
    /// drenaje, así que con nada en vuelo vuelve muy por debajo de este número. Existe para que una
    /// avería del drenaje no convierta la prueba en un cuelgue.
    const PLAZO_DE_PARADA: Duration = Duration::seconds(1);

    /// El plazo con el que una prueba espera un vencimiento **real** del reloj.
    ///
    /// Un calendario de cada segundo dispara en cuanto el bucle del reloj da su vuelta, así que
    /// dispara muy por debajo de este número. Existe para acotar un cuelgue, no para dar margen a
    /// una ejecución lenta.
    const PLAZO_DEL_TICK: std::time::Duration = std::time::Duration::from_secs(5);

    /// Cada cuánto se mira si el tick ya creó la ejecución.
    const INTERVALO_DE_MIRADA: std::time::Duration = std::time::Duration::from_millis(50);

    /// Y el plazo con el que la prueba espera a que esa ejecución llegue a su estado terminal.
    ///
    /// No mide el tick: se espera porque el trabajador del motor corre en un hilo aparte que
    /// escribe en la base a través del runtime, y soltar el runtime con una ejecución a medias hace
    /// saltar al hilo que escribe. El abridor de este aparato tarda un segundo en decir que no
    /// abre, así que vuelve muy por debajo de este número.
    const PLAZO_DEL_FINAL: std::time::Duration = std::time::Duration::from_secs(30);

    /// La próxima ejecución que el reloj tiene calculada para un mango, en texto; `None` si el reloj
    /// no tiene ninguna tarea con ese mango.
    ///
    /// Es una observación del **reloj privado** —el mismo que el módulo guarda tras su candado—, y
    /// el instante se queda en texto a propósito: lo que estas pruebas necesitan es distinguir una
    /// próxima ejecución de otra —o de ninguna—, no leer el instante.
    async fn proxima_del_reloj(planificador: &Planificador, mango: Uuid) -> Option<String> {
        let reloj = planificador.0.reloj.lock().await;

        reloj
            .proxima_de(mango)
            .await
            .expect("el reloj tiene que poder decir cuándo dispara su tarea")
            .map(|cuando| format!("{cuando:?}"))
    }

    /// 2 — Escenario 1: una tarea activa queda planificada al arrancar, con su próxima ejecución.
    ///
    /// Las dos mitades se afirman por separado porque son dos hechos distintos: que la tarea esté en
    /// la tabla de lo planificado, y que el reloj le haya calculado el tick. Un `add` que se hubiera
    /// quedado sin mango no dejaría mango, y uno que hubiera entrado sin próxima ejecución
    /// sería una tarea que nunca dispara.
    #[tokio::test]
    async fn una_tarea_activa_queda_planificada_al_arrancar() {
        let aparato = Aparato::nuevo().await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;

        let planificador = aparato.planificador().await;

        assert!(
            planificador.planificadas().contains(&tarea),
            "la tarea activa no quedó planificada: {:?}",
            planificador.planificadas()
        );

        let mangos = planificador.mangos();
        assert_eq!(
            mangos.len(),
            1,
            "la tarea planificada no tiene un mango vivo en el reloj: {mangos:?}"
        );
        assert!(
            proxima_del_reloj(&planificador, mangos[0]).await.is_some(),
            "la tarea está planificada pero el reloj no le ha calculado su próxima ejecución"
        );
    }

    /// 2 — Escenario 2: una tarea desactivada no se planifica ni crea ninguna ejecución suya.
    ///
    /// «Llega la hora» no se puede esperar en una prueba, así que lo que se afirma es que la tarea
    /// **no está en el reloj** —de donde se sigue que ningún tick puede dispararla— y que no hay
    /// ninguna ejecución suya, ni siquiera después de una reconciliación.
    #[tokio::test]
    async fn una_tarea_desactivada_no_se_planifica_ni_crea_ejecucion() {
        let aparato = Aparato::nuevo().await;
        let activa = aparato.tarea(CRON_DE_CINCO, true).await;
        let desactivada = aparato.tarea(CRON_DE_CINCO, false).await;

        let planificador = aparato.planificador().await;

        assert!(
            planificador.planificadas().contains(&activa),
            "la tarea activa de la misma base no quedó planificada: {:?}",
            planificador.planificadas()
        );
        assert!(
            !planificador.planificadas().contains(&desactivada),
            "una tarea con `enabled = 0` quedó planificada: {:?}",
            planificador.planificadas()
        );

        planificador
            .reconciliar()
            .await
            .expect("una reconciliación sin fallo de base no puede dar error");

        assert!(
            !planificador.planificadas().contains(&desactivada),
            "la reconciliación planificó una tarea con `enabled = 0`"
        );
        assert_eq!(
            aparato.ejecuciones_de(&desactivada).await,
            0,
            "una tarea desactivada creó una ejecución"
        );
    }

    /// 2 — Escenario 3: una expresión inválida se registra —nombrando tarea y expresión— y las
    /// demás siguen planificadas.
    ///
    /// El error se lee del registro porque ahí no hay petición ni campo que nombrar: es lo único que
    /// distingue una expresión mala de otra en un registro con veinte tareas, así que tiene que
    /// nombrar **las dos cosas**. Y el arranque tiene que completarse: la errata de una tarea no
    /// puede dejar sin reloj a las copias buenas.
    #[tokio::test]
    async fn una_expresion_invalida_se_registra_y_las_demas_siguen_planificadas() {
        let aparato = Aparato::nuevo().await;
        let buena_una = aparato.tarea(CRON_DE_CINCO, true).await;
        let mala = aparato.tarea("a b c d e", true).await;
        let buena_dos = aparato.tarea("30 4 * * *", true).await;

        let (buffer, guardia) = capturar_log();

        let planificador = aparato.planificador().await;

        let registro = buffer.texto();
        drop(guardia);

        assert!(
            registro.contains(&mala.to_string()),
            "el error de la expresión inválida no nombra la tarea: {registro}"
        );
        assert!(
            registro.contains("a b c d e"),
            "el error de la expresión inválida no nombra la expresión: {registro}"
        );
        assert!(
            planificador.planificadas().contains(&buena_una)
                && planificador.planificadas().contains(&buena_dos),
            "las dos tareas válidas no quedaron planificadas: {:?}",
            planificador.planificadas()
        );
        assert!(
            !planificador.planificadas().contains(&mala),
            "la tarea con la expresión inválida quedó planificada: {:?}",
            planificador.planificadas()
        );
    }

    /// 3 — Escenario 1: un alta entra en la planificación en la siguiente reconciliación.
    ///
    /// La tarea se da de alta **después** de arrancar y el planificador no se vuelve a arrancar: es
    /// la reconciliación la que la recoge, que es la sincronización sin reiniciar el proceso.
    #[tokio::test]
    async fn una_tarea_nueva_entra_en_la_planificacion_al_reconciliar() {
        let aparato = Aparato::nuevo().await;
        let planificador = aparato.planificador().await;

        assert!(
            planificador.planificadas().is_empty(),
            "el planificador arrancó con tareas que no existían: {:?}",
            planificador.planificadas()
        );

        let nueva = aparato.tarea(CRON_DE_CINCO, true).await;

        planificador
            .reconciliar()
            .await
            .expect("una reconciliación sin fallo de base no puede dar error");

        assert!(
            planificador.planificadas().contains(&nueva),
            "la tarea dada de alta después de arrancar no entró en la planificación: {:?}",
            planificador.planificadas()
        );
    }

    /// 3 — Escenario 2: un borrado sale de la planificación en la siguiente reconciliación.
    ///
    /// El borrado **no pasa por la API** y no avisa a nadie: es justo el caso que la reconciliación
    /// existe para cubrir, y el que una notificación desde el manejador no vería.
    #[tokio::test]
    async fn un_borrado_sale_de_la_planificacion_al_reconciliar() {
        let aparato = Aparato::nuevo().await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        assert!(planificador.planificadas().contains(&tarea));

        assert!(
            aparato
                .tareas
                .borrar(&tarea)
                .await
                .expect("el borrado de la tarea no puede fallar"),
            "la tarea de la prueba tenía que existir para borrarse"
        );

        planificador
            .reconciliar()
            .await
            .expect("una reconciliación sin fallo de base no puede dar error");

        assert!(
            !planificador.planificadas().contains(&tarea),
            "la tarea borrada siguió planificada: {:?}",
            planificador.planificadas()
        );
        assert!(
            planificador.mangos().is_empty(),
            "el borrado dejó un mango vivo en el reloj: {:?}",
            planificador.mangos()
        );
        assert_eq!(
            aparato.ejecuciones_de(&tarea).await,
            0,
            "una tarea borrada no puede tener ejecuciones nuevas"
        );
    }

    /// 3 — Escenario 3: un cambio de cron reemplaza la planificación sin duplicarla.
    ///
    /// Se mide doble: que la tarea sigue planificada **una sola vez** —el fallo que este escenario
    /// existe para evitar es el disparo doble, que no da ningún error—, y que la próxima ejecución
    /// es la de la expresión **nueva**. El mango viejo tiene que dejar de estar en el reloj: es la
    /// única forma de que dos planificaciones no convivan.
    #[tokio::test]
    async fn un_cambio_de_cron_reemplaza_la_planificacion_sin_duplicarla() {
        let aparato = Aparato::nuevo().await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        let mangos_de_antes = planificador.mangos();
        assert_eq!(
            mangos_de_antes.len(),
            1,
            "la tarea tenía que estar planificada una sola vez: {mangos_de_antes:?}"
        );
        let mango_viejo = mangos_de_antes[0];
        let antes = proxima_del_reloj(&planificador, mango_viejo).await;
        assert!(
            antes.is_some(),
            "la tarea planificada no tenía próxima ejecución"
        );

        // La edición no pasa por la API.
        aparato.cambiar_cron(&tarea, "30 4 * * *").await;

        planificador
            .reconciliar()
            .await
            .expect("una reconciliación sin fallo de base no puede dar error");

        let mangos_de_despues = planificador.mangos();
        assert_eq!(
            mangos_de_despues.len(),
            1,
            "el cambio de expresión dejó la tarea planificada dos veces: {mangos_de_despues:?}"
        );
        assert!(
            planificador.planificadas().contains(&tarea),
            "la tarea con la expresión nueva dejó de estar planificada"
        );

        let mango_nuevo = mangos_de_despues[0];
        assert_ne!(
            mango_nuevo, mango_viejo,
            "el reemplazo reutilizó el mango viejo"
        );
        assert!(
            proxima_del_reloj(&planificador, mango_viejo)
                .await
                .is_none(),
            "el mango de la planificación vieja sigue vivo en el reloj"
        );
        let despues = proxima_del_reloj(&planificador, mango_nuevo).await;
        assert!(
            despues.is_some(),
            "la planificación nueva no tiene próxima ejecución"
        );
        assert_ne!(
            despues, antes,
            "la próxima ejecución no es la de la expresión nueva: sigue siendo la de la vieja"
        );
    }

    /// 3 — Escenario 4: desactivar retira la planificación y reactivar la devuelve.
    #[tokio::test]
    async fn desactivar_retira_la_planificacion_y_reactivar_la_devuelve() {
        let aparato = Aparato::nuevo().await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        assert!(planificador.planificadas().contains(&tarea));

        aparato.cambiar_enabled(&tarea, false).await;
        planificador
            .reconciliar()
            .await
            .expect("una reconciliación sin fallo de base no puede dar error");

        assert!(
            !planificador.planificadas().contains(&tarea),
            "una tarea con `enabled = 0` siguió planificada: {:?}",
            planificador.planificadas()
        );

        aparato.cambiar_enabled(&tarea, true).await;
        planificador
            .reconciliar()
            .await
            .expect("una reconciliación sin fallo de base no puede dar error");

        assert!(
            planificador.planificadas().contains(&tarea),
            "la tarea reactivada no volvió a la planificación: {:?}",
            planificador.planificadas()
        );
    }

    /// 6 — Escenario 2: la parada del planificador no crea ninguna ejecución nueva.
    ///
    /// «Cuando transcurre el tiempo que habría hecho disparar» no se puede esperar en una prueba, así
    /// que se provoca el vencimiento de la única forma que el contrato admite: llamando a `disparar`,
    /// que es exactamente lo que hará el cierre del reloj —y que la medición de la fase 0 mostró que
    /// puede llegar **después** de la parada—. Lo que se mide es el recuento de filas: con la puerta
    /// cerrada no hay ninguna.
    #[tokio::test]
    async fn tras_parar_no_se_crea_ninguna_ejecucion_nueva() {
        let aparato = Aparato::nuevo().await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        assert!(planificador.planificadas().contains(&tarea));

        planificador.parar(PLAZO_DE_PARADA).await;

        assert!(
            planificador.esta_parado(),
            "tras parar, el planificador sigue aceptando trabajo"
        );

        // El vencimiento que el bucle puede disparar después de la parada: la puerta cerrada lo deja
        // sin efecto.
        planificador.disparar(tarea).await;

        assert_eq!(
            aparato.ejecuciones_de(&tarea).await,
            0,
            "la parada del planificador dejó una ejecución creada"
        );
    }

    /// 2 y 5 — Un vencimiento **real** del reloj crea la ejecución de la tarea que le toca.
    ///
    /// Todas las demás pruebas del módulo llaman a [`Planificador::disparar`] directamente —que es
    /// el cuerpo del cierre del reloj— y por eso ninguna ve cómo el cierre **elige el
    /// identificador** con el que dispara. Ese hueco dejó pasar un fallo real: el cierre recibía el
    /// `Uuid` que el reloj entregaba y lo trataba como `job_id` de Kesto, así que cada tick acababa
    /// en `TareaQueNoEsta` y **nunca** nacía una ejecución.
    ///
    /// Esta es la que atraviesa el vencimiento de verdad: planifica una tarea de cada segundo, deja
    /// correr el reloj y espera a que aparezca la fila de **esa** tarea. Es lenta por construcción —el
    /// bucle del reloj mide el tiempo con el reloj de pared, así que no se puede pausar—, y por eso
    /// espera con un plazo acotado en vez de con un `sleep` fijo.
    #[tokio::test]
    async fn un_tick_real_del_reloj_crea_la_ejecucion_de_su_tarea() {
        let aparato = Aparato::nuevo().await;
        // `* * * * * *` es «cada segundo»: el vencimiento llega sin esperar una hora del calendario.
        let tarea = aparato.tarea("* * * * * *", true).await;

        let planificador = aparato.planificador().await;

        let limite = Instant::now() + PLAZO_DEL_TICK;
        while aparato.ejecuciones_de(&tarea).await == 0 {
            assert!(
                Instant::now() < limite,
                "el vencimiento del reloj no creó la ejecución de su tarea en {PLAZO_DEL_TICK:?}: \
                 el disparo del reloj no llega a la tarea de Kesto"
            );
            tokio::time::sleep(INTERVALO_DE_MIRADA).await;
        }

        // Y se espera a que la ejecución termine, para no soltar el runtime con el trabajador a
        // medias: el abridor de este aparato tarda un segundo en decir que no abre.
        let limite = Instant::now() + PLAZO_DEL_FINAL;
        loop {
            let estado = aparato.estado_de_la_ejecucion(&tarea).await;

            if estado == "success" || estado == "failed" {
                break;
            }

            assert!(
                Instant::now() < limite,
                "la ejecución que creó el vencimiento no llegó a un estado terminal en \
                 {PLAZO_DEL_FINAL:?}: {estado}"
            );
            tokio::time::sleep(INTERVALO_DE_MIRADA).await;
        }

        planificador.parar(PLAZO_DE_PARADA).await;
    }

    /// El plazo, deliberadamente diminuto, con el que se fuerza que el drenaje de una parada venza:
    /// se toma el candado `estado` —como lo tendría un disparo en vuelo durante un `encolar` lento—
    /// y se para con este plazo, que no llega a adquirirlo.
    const PLAZO_QUE_VENCE: Duration = Duration::milliseconds(10);

    /// 6 — La puerta queda cerrada aunque el drenaje venza el plazo.
    ///
    /// Es el caso que el diseño anterior no cubría: si el candado `estado` está tomado más que el
    /// plazo —un `disparar` que lo retiene durante `motor.encolar(..).await`, con una escritura que
    /// puede esperar el `BUSY_TIMEOUT` de la base—, una puerta que solo se cerrara al adquirirlo se
    /// quedaría abierta **para siempre**: los vencimientos posteriores encolarían tras la parada y
    /// [`Planificador::esta_parado`] seguiría diciendo que no. Aquí se comprueba que el cierre **no
    /// depende** de adquirir el candado dentro del plazo.
    #[tokio::test]
    async fn la_puerta_queda_cerrada_aunque_el_drenaje_venza_el_plazo() {
        let aparato = Aparato::nuevo().await;
        let planificador = aparato.planificador().await;

        // El candado `estado`, tomado como lo tendría un disparo en vuelo: `parar` no podrá
        // adquirirlo dentro del plazo.
        let guardia = planificador.0.estado.lock().await;

        planificador.parar(PLAZO_QUE_VENCE).await;

        assert!(
            planificador.esta_parado(),
            "la puerta quedó abierta porque el plazo venció antes de adquirir el candado"
        );

        drop(guardia);
    }

    /// 6 — Un disparo posterior a una parada que venció el plazo tampoco encola.
    ///
    /// Es la mitad que importa del requisito 6: el vencimiento que el reloj dispare **después** de
    /// `shutdown()` no puede crear ninguna ejecución. Con la puerta que solo se cerraba al adquirir
    /// el candado, este disparo encontraba el planificador abierto y encolaba.
    #[tokio::test]
    async fn un_disparo_tras_una_parada_que_vencio_el_plazo_no_encola() {
        let aparato = Aparato::nuevo().await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        let guardia = planificador.0.estado.lock().await;
        planificador.parar(PLAZO_QUE_VENCE).await;
        drop(guardia);

        planificador.disparar(tarea).await;

        assert_eq!(
            aparato.ejecuciones_de(&tarea).await,
            0,
            "un disparo tras una parada que venció el plazo creó una ejecución"
        );
    }

    /// Los avisos del reloj no se pierden aunque lleguen antes de que el bucle esté esperando.
    ///
    /// `añadir`, `remove` y `apagar` notifican **después** de soltar el candado de `jobs`, así que
    /// es posible que el bucle todavía no haya registrado su `notified()`. Con `notify_waiters()`
    /// el aviso se perdería y el bucle podría dormirse para siempre con un job nuevo o con la
    /// parada ya ordenada; `notify_one()` guarda un permiso que se consume al registrarse.
    #[tokio::test]
    async fn un_aviso_antes_de_registrar_el_waiter_no_se_pierde() {
        use std::sync::Arc;

        use super::{Disparo, NativeScheduler};

        let reloj = NativeScheduler::nuevo();
        let disparo: Disparo = Arc::new(|_job_id| Box::pin(async {}));
        let agenda = super::validar("* * * * * *").expect("una agenda de cada segundo es válida");

        // El reloj no tiene bucle en marcha: los tres avisos ocurren antes de registrar un waiter.
        let mango = reloj.añadir(Uuid::new_v4(), agenda, disparo).await;
        tokio::time::timeout(std::time::Duration::from_secs(1), reloj.notify.notified())
            .await
            .expect("el aviso del alta se perdió sin waiter registrado");

        reloj
            .remove(&mango)
            .await
            .expect("retirar el mango del reloj no puede fallar");
        tokio::time::timeout(std::time::Duration::from_secs(1), reloj.notify.notified())
            .await
            .expect("el aviso de la retirada se perdió sin waiter registrado");

        reloj
            .apagar()
            .await
            .expect("apagar un reloj sin bucle en marcha no puede fallar");
        tokio::time::timeout(std::time::Duration::from_secs(1), reloj.notify.notified())
            .await
            .expect("el aviso de la parada se perdió sin waiter registrado");
    }

    /// El invariante contra el **mango huérfano**: la tabla no se toca hasta que el reloj confirma.
    ///
    /// Con el `remove` nativo, que no trae almacén en memoria, el fallo se dobla aquí: se llama a
    /// [`Estado::aplicar_retirada`] —la única puerta por la que las tres retiradas tocan la
    /// tabla— con un `Err` construido a mano. Lo que se fija es el invariante, no el camino: si la
    /// retirada no se confirma, la entrada **y su mango** se quedan, para que la siguiente pasada
    /// pueda reintentarla; quitarla antes la dejaría planificada y sin nadie capaz de retirarla.
    #[test]
    fn una_retirada_que_el_reloj_no_confirma_deja_el_mango_en_la_tabla() {
        use std::collections::HashMap;

        use super::{ErrorDelReloj, Estado};

        let job_id = Uuid::new_v4();
        let mango = Uuid::new_v4();

        let mut estado = Estado {
            planificadas: HashMap::from([(job_id, mango)]),
            expresiones: HashMap::from([(job_id, CRON_DE_CINCO.to_owned())]),
        };

        let fallo = estado.aplicar_retirada(
            job_id,
            Err(ErrorDelReloj::Operacion {
                motivo: "fallo simulado".to_owned(),
            }),
        );

        assert!(
            fallo.is_err(),
            "una retirada que el reloj no confirma no puede darse por hecha"
        );
        assert_eq!(
            estado.planificadas.get(&job_id),
            Some(&mango),
            "la retirada no confirmada perdió el mango: la tarea seguiría planificada y sin forma \
             de retirarla"
        );
        assert!(
            estado.expresiones.contains_key(&job_id),
            "la retirada no confirmada borró la expresión: la siguiente pasada no podría reintentar"
        );

        // Y cuando el reloj sí confirma, la entrada sale.
        assert!(
            estado.aplicar_retirada(job_id, Ok(())).is_ok(),
            "una retirada confirmada no puede dar error"
        );
        assert!(
            estado.planificadas.is_empty() && estado.expresiones.is_empty(),
            "la retirada confirmada no quitó la entrada de la tabla: {:?}",
            estado.planificadas
        );
    }
}
