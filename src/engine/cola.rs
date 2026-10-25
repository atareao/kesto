//! La cola de trabajos: un buzón por repositorio y **un** trabajador secuencial detrás.
//!
//! Lo que se serializa es el **repositorio**, no la tarea, y el motivo está medido: `rustic_core`
//! toma un **bloqueo exclusivo del repositorio**, así que dos copias contra el mismo repositorio
//! lanzadas a la vez no corren más —se bloquean entre sí, y la que pierde espera sin que nada lo
//! diga—. Dos tareas que escriben en repositorios distintos, en cambio, no se tocan: hacerlas
//! esperar sería tiempo perdido.
//!
//! La estructura es un mapa `repository_id → buzón`, y el buzón se crea **perezosamente**, la
//! primera vez que se encola algo para ese repositorio: los repositorios que no tienen trabajo no
//! tienen hilo. El primero que encola crea el trabajador y los siguientes le mandan por su canal.
//!
//! # El trabajador ocioso no consume CPU
//!
//! Se queda **bloqueado en la recepción** del canal, así que mientras no haya nada que hacer no
//! hace nada: ni sondeo, ni espera activa, ni un temporizador. Encolar lo despierta. Es la mitad
//! del requisito, y la otra mitad es que el canal esté acotado: `try_send` y nunca `send`, porque
//! quien encola viene del runtime de la aplicación —una petición HTTP— y bloquear ahí un hilo de
//! Tokio por una cola llena es el fallo que este change entero existe para no cometer.
//!
//! # ⚠️ El trabajador **no crea ningún runtime**
//!
//! Es un [`std::thread`] pelado que entra en el de la aplicación con el [`tokio::runtime::Handle`]
//! **solo** para la base, y llama a Rustic fuera de todo `block_on`. No es una preferencia: está
//! medido que llamar a Rustic desde dentro de un runtime entra en **pánico** —«Cannot start a
//! runtime from within a runtime.»—, así que un runtime creado aquí dentro mataría la primera
//! copia. Y el pánico no se lleva al trabajador: ver [`atender`].

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::ejecucion;
use super::{Contexto, ErrorDelMotor};
use crate::db::entities::Ejecucion;

/// Capacidad de la cola de un repositorio, en ejecuciones.
///
/// Con la guarda del `409` ya puesta —una ejecución `pending` o `running` por tarea— y sesenta y
/// cuatro de fondo por repositorio, llenarla es un caso extremo: harían falta sesenta y cuatro
/// tareas distintas apuntando al mismo repositorio y esperando a la vez. El número no se elige
/// para que quepa mucho, sino para que **no se note**: lo que se retiene son identificadores y
/// fechas, no volcados, y lo que se protege es que un encolado no crezca sin tope si una copia se
/// atasca.
pub(crate) const CAPACIDAD_DE_LA_COLA: usize = 64;

/// El motivo con el que se apunta una ejecución cuyo trabajador se cayó con un pánico.
///
/// No se publica el texto del pánico —es arbitrario y podría llevar cualquier cosa a una columna
/// que el panel muestra—, pero tampoco se calla: la fila dice que hubo un pánico, y el registro
/// dice lo mismo con más detalle.
const MOTIVO_DEL_PANICO: &str = "la copia se ha interrumpido por un pánico del trabajador: \
                                  mira el registro del proceso para ver qué lo provocó";

/// La cola del motor: los buzones y si sigue aceptando trabajo.
pub(crate) struct Cola {
    /// El estado de la cola, detrás de **un solo** candado.
    ///
    /// La parada y los buzones van juntos y bajo el mismo candado a propósito: entre «el motor no
    /// está parado» y «creo el buzón» cabe una parada, y el trabajador que naciera en ese hueco no
    /// lo pararía nadie —nadie tiene su buzón— y se quedaría con una ejecución en la mano hasta
    /// que el proceso acabara. Con las dos cosas en la misma sección crítica, ese hueco no existe.
    estado: Mutex<EstadoDeLaCola>,
}

/// Lo que hay dentro del candado de la cola.
struct EstadoDeLaCola {
    /// `true` cuando el motor ha dejado de aceptar trabajo.
    parada: bool,
    /// Los buzones, uno por repositorio con trabajo.
    buzones: HashMap<Uuid, Buzon>,
}

/// Lo que hace falta para hablarle a un trabajador y para esperarlo.
struct Buzon {
    /// El extremo de envío de su canal. Soltarlo es lo que cierra el canal y termina al trabajador.
    emisor: SyncSender<Ejecucion>,
    /// La señal que el trabajador manda al salir de su bucle.
    terminado: Receiver<()>,
    /// El hilo que lo corre.
    hilo: JoinHandle<()>,
}

/// Un trabajador del que ya solo queda esperar: su emisor está soltado.
///
/// Es un tipo propio y no un [`Buzon`] porque la diferencia es la que hace que `parar` funcione:
/// mientras el emisor viva, el trabajador **no** sale de su recepción, y esperarlo sería esperar a
/// un hilo que no tiene por qué terminar. Soltar el emisor es lo que cierra el canal, y a partir
/// de ahí sí hay algo que esperar.
struct FinDelTrabajador {
    /// La señal de que el trabajador ha salido de su bucle.
    terminado: Receiver<()>,
    /// El hilo, para unirlo cuando ya señaló.
    hilo: JoinHandle<()>,
}

impl Cola {
    /// Una cola vacía que acepta trabajo.
    pub(crate) fn nueva() -> Self {
        Self {
            estado: Mutex::new(EstadoDeLaCola {
                parada: false,
                buzones: HashMap::new(),
            }),
        }
    }

    /// ¿Ha dejado el motor de aceptar trabajo?
    ///
    /// Es la comprobación que [`Cola::encolar`] repite bajo el candado: ésta es la de antes —la que
    /// evita crear una fila para nada— y la de dentro es la que decide de verdad.
    pub(crate) fn esta_parada(&self) -> bool {
        self.estado().parada
    }

    /// Encola una ejecución en la cola de su repositorio, creando su trabajador si no lo había.
    ///
    /// Nunca bloquea: el envío es un `try_send` y una cola llena se cuenta como un error con nombre
    /// propio. Quien llama —la petición HTTP del disparo manual— tiene que poder contestar, y
    /// bloquear un hilo del runtime esperando hueco es justo lo que no se puede hacer.
    pub(crate) fn encolar(
        &self,
        contexto: &Contexto,
        repository_id: Uuid,
        ejecucion_a_encolar: Ejecucion,
    ) -> Result<(), ErrorDelMotor> {
        let mut estado = self.estado();

        // Bajo el candado, que es donde la decisión es de verdad: la comprobación de fuera puede
        // haber quedado vieja.
        if estado.parada {
            return Err(ErrorDelMotor::MotorParado);
        }

        // `entry` y no «leer y luego insertar»: el trabajador se crea una sola vez por repositorio
        // aunque dos encolados entren a la vez, y así no hay ni un `expect` ni una carrera.
        let buzon = match estado.buzones.entry(repository_id) {
            std::collections::hash_map::Entry::Occupied(entrada) => entrada.into_mut(),
            std::collections::hash_map::Entry::Vacant(hueco) => {
                hueco.insert(Buzon::nuevo(contexto, repository_id)?)
            }
        };

        buzon
            .emisor
            .try_send(ejecucion_a_encolar)
            .map_err(|causa| match causa {
                mpsc::TrySendError::Full(_) => {
                    ErrorDelMotor::ColaDelRepositorioLlena { repository_id }
                }
                // El trabajador de ese repositorio ya no está: no puede pasar mientras el buzón
                // siga en el mapa —el receptor solo se suelta al salir del bucle, y el bucle sale
                // cuando el canal se cierra, que exige que este emisor muera—, pero si pasara, lo
                // que no se puede es decir «la cola está llena», que mandaría al operador a mirar
                // lo que no es.
                mpsc::TrySendError::Disconnected(_) => ErrorDelMotor::SinQuienAtiendaLaCola {
                    repository_id,
                    motivo: "su trabajador ya no está".to_owned(),
                },
            })
    }

    /// Deja de aceptar trabajo, suelta los buzones y espera a los trabajadores con un plazo común.
    ///
    /// El plazo es **uno** para todos y no uno por trabajador: con uno por cabeza, veinte
    /// repositorios con trabajo en vuelo convertirían una parada de cinco segundos en cien. Aquí se
    /// calcula un instante límite y cada trabajador espera lo que le quede.
    ///
    /// Lo que no termine dentro de ese plazo se **registra** y el proceso acaba igualmente: esperar
    /// sin límite convertiría un `docker stop` en una espera que el orquestador resuelve con un
    /// `SIGKILL`, que es justo lo que la parada ordenada existía para evitar.
    ///
    /// Es **idempotente**: la segunda llamada no encuentra nada que soltar y vuelve en el acto.
    pub(crate) fn parar(&self, espera: Duration) {
        let Some(pendientes) = self.soltar() else {
            return;
        };

        let limite = Instant::now() + espera;
        let mut terminados = Vec::new();

        for trabajador in pendientes {
            let restante = limite.saturating_duration_since(Instant::now());

            match trabajador.terminado.recv_timeout(restante) {
                Ok(()) => terminados.push(trabajador.hilo),
                // Se cayó sin señalar: su hilo ya no está, así que unirlo no puede colgarse.
                Err(RecvTimeoutError::Disconnected) => terminados.push(trabajador.hilo),
                Err(RecvTimeoutError::Timeout) => tracing::warn!(
                    "un trabajador no ha terminado dentro del plazo de parada: su copia sigue en \
                     marcha y el proceso acaba sin esperarlo más"
                ),
            }
        }

        for hilo in terminados {
            if hilo.join().is_err() {
                tracing::warn!("un trabajador ha terminado con un pánico");
            }
        }

        tracing::info!("motor parado: no queda ningún trabajador en vuelo dentro del plazo");
    }

    /// Suelta los buzones y devuelve lo que queda por esperar, o `None` si ya estaba parada.
    ///
    /// Es la mitad que de verdad para el motor: soltar los buzones suelta los emisores, y con el
    /// canal cerrado cada trabajador sale de su recepción en cuanto acaba el trabajo que tenga en
    /// la mano. Lo que se devuelve **no** lleva emisor a propósito: ver [`FinDelTrabajador`].
    fn soltar(&self) -> Option<Vec<FinDelTrabajador>> {
        let mut estado = self.estado();

        if estado.parada {
            return None;
        }

        estado.parada = true;

        Some(
            std::mem::take(&mut estado.buzones)
                .into_values()
                .map(Buzon::soltar)
                .collect(),
        )
    }

    /// Toma el candado del estado **sin entrar en pánico** si otro hilo se cayó teniéndolo tomado.
    ///
    /// Misma decisión que en el resto del motor: el mapa de buzones sigue siendo válido tal y como
    /// quedó, y perder la cola entera por el pánico de otro hilo sería dejar el motor sin motor.
    fn estado(&self) -> MutexGuard<'_, EstadoDeLaCola> {
        match self.estado.lock() {
            Ok(guardado) => guardado,
            Err(veneno) => veneno.into_inner(),
        }
    }
}

impl Buzon {
    /// Crea el canal acotado, lanza el trabajador y devuelve su buzón.
    fn nuevo(contexto: &Contexto, repository_id: Uuid) -> Result<Self, ErrorDelMotor> {
        let (emisor, receptor) = mpsc::sync_channel(CAPACIDAD_DE_LA_COLA);
        // El canal del aviso es de capacidad cero y se usa una sola vez, al salir del bucle: no es
        // un canal de datos, es una señal de «he terminado», y con capacidad cero el emisor no
        // retiene nada.
        let (aviso, terminado) = mpsc::channel();
        let contexto = contexto.clone();

        let hilo = std::thread::Builder::new()
            .name(format!("kesto-trabajador-{repository_id}"))
            .spawn(move || trabajo_secuencial(contexto, receptor, aviso))
            .map_err(|causa| ErrorDelMotor::SinQuienAtiendaLaCola {
                repository_id,
                motivo: format!("no se ha podido crear el hilo del trabajador: {causa}"),
            })?;

        Ok(Self {
            emisor,
            terminado,
            hilo,
        })
    }

    /// Consume el buzón dejando solo lo que hace falta para esperarlo.
    ///
    /// El `drop(emisor)` explícito no es estilo: es **lo que cierra el canal**, y sin él el
    /// trabajador seguiría bloqueado en su recepción esperando trabajo que ya no va a llegar.
    fn soltar(self) -> FinDelTrabajador {
        let Self {
            emisor,
            terminado,
            hilo,
        } = self;

        drop(emisor);

        FinDelTrabajador { terminado, hilo }
    }
}

/// El bucle de un trabajador: atiende su cola **de una en una**, en el orden en que se encoló.
///
/// Se queda bloqueado en la recepción mientras no hay nada, así que no consume CPU, y sale del
/// bucle cuando el canal se cierra —el motor soltó su buzón— avisando por `aviso` de que ha
/// terminado. Ese aviso es lo que espera la parada, y va **al final**, después del último trabajo.
fn trabajo_secuencial(contexto: Contexto, receptor: Receiver<Ejecucion>, aviso: mpsc::Sender<()>) {
    while let Ok(ejecucion) = receptor.recv() {
        atender(&contexto, &ejecucion);
    }

    // El canal se cerró: el motor soltó el buzón de este repositorio. Se avisa y se sale, y si el
    // aviso no lo recoge nadie —el proceso está acabando— tampoco pasa nada.
    let _ = aviso.send(());
}

/// Atiende una ejecución **sin dejar que un pánico se lleve por delante al trabajador**.
///
/// El `catch_unwind` no es un adorno y no está aquí por prudencia genérica: está medido que llamar
/// a Rustic desde dentro de un runtime entra en pánico, así que la forma más fácil de romper este
/// motor es un `block_on` mal puesto. Sin la red, ese fallo mata el hilo, la fila se queda en
/// `running` para siempre —el panel la pinta como «en marcha» y nadie se entera de que murió— y la
/// cola de ese repositorio se queda sin nadie que la atienda. Con la red, la ejecución acaba en
/// `failed` con su `finished_at` y el trabajador sigue vivo para la siguiente.
///
/// Lo que **no** se hace con el pánico es publicar su texto: es arbitrario y acabaría en una
/// columna que el panel muestra. Se apunta un motivo propio y el detalle se queda en el registro,
/// donde la red de secretos no hace falta porque nadie lo lee desde fuera.
fn atender(contexto: &Contexto, ejecucion: &Ejecucion) {
    let intento = panic::catch_unwind(AssertUnwindSafe(|| {
        ejecucion::ejecutar(contexto, ejecucion);
    }));

    if let Err(payload) = intento {
        let detalle = texto_del_panico(&payload);

        tracing::error!(
            execution_id = %ejecucion.id,
            job_id = %ejecucion.job_id,
            detalle,
            "la ejecución se ha interrumpido por un pánico: se apunta como fallida y el \
             trabajador sigue con la siguiente"
        );

        ejecucion::fallar_si_no_termino(contexto, &ejecucion.id, MOTIVO_DEL_PANICO);
    }
}

/// El texto de un pánico, para el registro y **solo** para el registro.
///
/// Un `panic!` con formato llega como [`String`] y uno sin él como `&'static str`; cualquier otra
/// cosa —un payload propio— se nombra a sí misma sin publicarse, porque no hay forma de saber qué
/// lleva dentro.
fn texto_del_panico(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(texto) = payload.downcast_ref::<&'static str>() {
        return (*texto).to_owned();
    }

    if let Some(texto) = payload.downcast_ref::<String>() {
        return texto.clone();
    }

    "un pánico sin texto".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{Cola, texto_del_panico};

    /// Una cola recién hecha acepta trabajo; una parada, no.
    ///
    /// Es la mitad de la parada que no necesita ni base ni trabajador: lo que se fija aquí es que
    /// el estado de la cola cambia **una sola vez** y que la segunda parada no vuelve a soltar
    /// nada —la idempotencia del contrato—. Encolar de verdad se mide en las pruebas del motor,
    /// que son las que tienen base.
    #[test]
    fn una_cola_se_para_una_sola_vez() {
        let cola = Cola::nueva();

        assert!(!cola.esta_parada(), "una cola recién hecha acepta trabajo");

        // La primera parada no tiene nada que soltar —no se encoló nada— y aun así deja la cola
        // cerrada: es lo que impide que un `encolar` posterior cree un trabajador que nadie
        // pararía.
        cola.parar(std::time::Duration::from_millis(10));
        assert!(
            cola.esta_parada(),
            "después de parar, la cola tiene que dejar de aceptar trabajo"
        );

        // Y la segunda no hace nada: `soltar` devuelve `None` y no hay nada que esperar.
        cola.parar(std::time::Duration::from_millis(10));
        assert!(cola.esta_parada(), "la segunda parada reabrió la cola");
    }

    /// El texto de un pánico se recoge en las dos formas que Rust le da, y una desconocida no se
    /// publica tal cual.
    #[test]
    fn el_texto_de_un_panico_se_recoge_en_sus_dos_formas() {
        let estatico: Box<dyn std::any::Any + Send> = Box::new("un texto de pánico");
        let formateado: Box<dyn std::any::Any + Send> = Box::new(String::from("otro texto"));

        assert_eq!(texto_del_panico(&estatico), "un texto de pánico");
        assert_eq!(texto_del_panico(&formateado), "otro texto");

        let propio: Box<dyn std::any::Any + Send> = Box::new(7_u8);
        assert_eq!(
            texto_del_panico(&propio),
            "un pánico sin texto",
            "un payload que no es texto tiene que decirse sin publicarse"
        );
    }
}
