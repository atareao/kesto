//! El puente entre el `stream` de la extracción y el origen de lectura de la biblioteca.
//!
//! La extracción entrega bytes **de forma asíncrona y a trozos** —el `stream` de un `exec`
//! de la API de Docker— y [`rustic_core::ReadSource`] pide un lector **bloqueante** que
//! además tiene que ser `Sync + Send`. Entre las dos cosas hay un hilo, un canal **acotado** y
//! un lector, y cada una de esas piezas tapa un fallo que no es una hipótesis: está medido.
//!
//! 1. **El canal es `sync_channel` y acotado.** Sin tope, un productor rápido contra un
//!    consumidor lento acumula **el volcado entero** en memoria, y la memoria retenida pasa a
//!    ser una función del tamaño de la base. Con el tope, el productor espera. La capacidad la
//!    elige el motor: este módulo no sabe cuánta memoria hay.
//! 2. **El final no viaja por el canal: es una señal que no se puede perder.** Medido: un solo
//!    `try_send` del mensaje de final se encontró el canal **lleno** justo al terminar el volcado
//!    —con un mebibyte entero ya leído—, el final se perdió y el volcado se descartó. Así que el
//!    final se declara con un [`AtomicBool`] que el productor pone **después** de empujar el
//!    último byte ([`ProductorDelVolcado::terminar`]) y que el lector consulta cuando el canal se
//!    cierra. Sin esa señal el cierre es ambiguo: el productor que termina bien y el que muere por
//!    un fallo del `exec` dejan los dos el canal cerrado, y el segundo caso significaría
//!    **archivar un volcado truncado como si fuera válido** —una copia que existe, ocupa sitio y
//!    no se puede restaurar—. Por eso el lector traduce un cierre con la señal en falso a un
//!    [`std::io::Error`], **NUNCA** a un fin de fichero: devolver `Ok(0)` ahí es el peor fallo
//!    posible de este módulo, porque la biblioteca lo leería como «el volcado se acabó» y
//!    archivaría lo que llegó.
//! 3. **El lector espera con plazo, no con confianza en el productor.** La espera es
//!    [`recv_timeout`](std::sync::mpsc::Receiver::recv_timeout) con una inactividad máxima que
//!    entra por parámetro, y **no** `recv()`: el canal se cierra cuando muere el **último**
//!    emisor, así que mientras uno viva —aunque sea uno retenido en un campo y sin que nadie lo
//!    use— `recv()` no vuelve. Medido: con el extremo de envío guardado en el túnel mientras el
//!    productor moría, el lector se quedó esperando y `archive()` **no volvió nunca**; hubo que
//!    matar la ejecución a los 400 s. Con el plazo, un productor vivo y avanzando no lo dispara,
//!    uno atascado falla con un motivo propio que nombra el volcado y el plazo, y uno muerto deja
//!    de colgar el archivado. Y no queda en manos de un aviso: el extremo del productor no se
//!    puede clonar y declarar el final lo consume, así que tampoco puede quedarse vivo por
//!    descuido.
//! 4. **`entries()` solo se puede llamar una vez, y el error del segundo intento protege al
//!    llamador.** El receptor se saca del [`Mutex`] con `.take()` —el trait exige `Sync` y un
//!    [`Receiver`] no lo es, así que el candado no se puede evitar: es el mismo truco que usa el
//!    `ChildStdoutSource` de la propia biblioteca—, y una segunda llamada devuelve un iterador
//!    con un **error** dentro, nunca uno vacío. Lo que ese error **no** es, es una barrera
//!    contra el archivado: es lo que dice el punto siguiente. Un origen es de un solo uso, y
//!    cada intento de copia construye su propio túnel con [`tunel_del_volcado`].
//! 5. **Ni el error del lector ni el del segundo `entries()` detienen la copia: la biblioteca
//!    los descarta con un aviso.** Está medido leyendo el `archiver.rs` de la biblioteca de
//!    copia: los dos entran por un `filter_map` que, en su rama de error, hace
//!    `warn!("ignoring error: …")` y sigue, así que `archive()` devuelve `Ok` y **guarda el
//!    snapshot igualmente** —sin el volcado, porque la entrada que falló no llega al árbol—. Y
//!    está medido el peor caso, que es peor que un aviso: productor muerto y un padre que **sí**
//!    tiene el nodo del volcado dan un `archive()` en `Ok` **sin ninguna línea de aviso** y con
//!    el árbol del padre, de modo que un volcado que no se generó se convierte en una copia
//!    válida de los datos anteriores. La consecuencia es la que gobierna este módulo: de un `Ok`
//!    de `archive()` **no** se deduce que el volcado esté completo, y la única verdad fiable es
//!    [`EntregaDelVolcado::completa()`], que el motor tiene que consultar por su cuenta antes de
//!    dar una copia por buena. Ver el comentario del `read` del [`LectorDeVolcado`]: es el aviso
//!    que más importa de todo el fichero.
//!
//! # Por qué hay un plazo aquí **y** otro en la extracción
//!
//! Son dos esperas distintas y cada plazo acota la suya: el de la extracción acota el `stream`
//! del `exec` —cuánto puede callarse el proceso del volcado sin dar señales de vida— y este acota
//! el **canal** —cuánto puede estar el lector sin recibir nada—. El segundo no es una precaución
//! de más: `archive()` es una llamada **bloqueante**, y sin un plazo puede no volver nunca, que
//! es el desenlace que este change entero existe para evitar —la ejecución se queda en `running`
//! y el worker secuencial de su repositorio queda ocupado para siempre—.

use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustic_core::repofile::{Metadata, Node, NodeType};
use rustic_core::{ErrorKind, ReadSource, ReadSourceEntry, RusticError, RusticResult};

use super::Error;

/// Las dos mitades de un origen de volcado, más la señal de si el volcado llegó entero.
///
/// Se construyen juntas con [`tunel_del_volcado`] y no se pueden construir por separado: un
/// origen sin su productor es un origen que nunca terminará, y un productor sin su origen es
/// un productor que se bloquea para siempre en cuanto el canal se llena.
///
/// El uso normal es **desestructurarlo**, porque cada pieza se va a un sitio distinto y el
/// productor hay que **moverlo** a su hilo. No va en un bloque de código compilable a propósito:
/// este crate no tiene destino de biblioteca, así que un ejemplo en el rustdoc no lo compila
/// nadie y envejecería mintiendo.
///
/// ```text
/// let TunelDelVolcado { origen, productor, entrega, avance } =
///     tunel_del_volcado(nombre, capacidad, inactividad_maxima)?;
///
/// let hilo = std::thread::spawn(move || {          // 1. el productor, en su hilo
///     for trozo in trozos { productor.enviar(trozo)?; }
///     productor.terminar();
/// });
///
/// let resultado = repositorio.archive(origen, …);  // 2. el archivado, en **este** hilo
/// hilo.join()?;                                    // 3. la unión, y después
/// if !entrega.completa() { /* la copia NO está completa, diga lo que diga ese `Ok` */ }
/// let leidos = avance.bytes();                     // …y el avance, mientras corre o al final
/// ```
///
/// # El productor va en **otro hilo**, y esto no es un consejo
///
/// El archivado lee del canal **en el hilo que llama a `archive()`**. Si el productor empujara
/// desde ese mismo hilo, se bloquearía en el `send` de [`ProductorDelVolcado::enviar`] esperando
/// hueco en un canal acotado mientras el consumidor espera al productor: **bloqueo mutuo
/// permanente**. Ese interbloqueo sigue ahí —el plazo de inactividad no lo cura, porque el plazo
/// caduca en el lector mientras el productor sigue esperando hueco—, así que el orden correcto es
/// siempre el mismo:
///
/// 1. el productor, en su hilo —el que drena el `exec` de la extracción—;
/// 2. el archivado, en el hilo que llama a `archive()`, leyendo de
///    [`TunelDelVolcado::origen`];
/// 3. la unión de los dos al final —`join()` del hilo del productor— y, **después**,
///    [`EntregaDelVolcado::completa()`].
///
/// Lo que cambió respecto a la primera versión de esta API es la clase de cuelgue que queda:
/// retener el extremo de envío —en un campo, mientras se archiva, sin que nadie lo use— ya no
/// puede colgar el archivado, porque el plazo del lector lo corta; y un productor que se cae sin
/// declarar el final ya no se puede confundir con uno que terminó bien. Lo único que hay que
/// seguir haciendo a mano es no meter el productor en el hilo del archivado.
///
/// # `capacidad` está en trozos, no en bytes
///
/// El canal retiene hasta `capacidad` **mensajes**, así que la memoria retenida es
/// `capacidad × el tamaño de un trozo`, no una cantidad de bytes: quien elija el número tiene
/// que multiplicar. Por eso el tope entra por parámetro en vez de estar aquí dentro, y por eso
/// el defecto no existe: elegirlo es decidir cuánta memoria puede gastar el motor.
/// `capacidad == 0` es un **canal de encuentro**: el `enviar` del productor no vuelve hasta que
/// haya una lectura en curso, así que no retiene memoria ninguna y frena al productor a la
/// velocidad exacta del archivado.
///
/// # `inactividad_maxima` acota **una espera**, no el volcado
///
/// El lector espera al trozo siguiente como mucho ese plazo, y cada lectura vuelve a empezar a
/// contar: lo que acota es el silencio, no la duración. Un volcado que avanza tarda lo que tenga
/// que tardar y no puede fallar por tardar; uno cuyo productor se calla —atascado o muerto con el
/// extremo de envío vivo en alguna parte— falla con [`Error::VolcadoEstancado`] en vez de dejar
/// `archive()` sin volver. El plazo lo elige el motor, que es quien conoce su presupuesto de
/// tiempo, y el módulo no propone ninguno. `Duration::ZERO` se acepta y no se corrige: significa
/// «solo lo que ya esté en el canal», porque cada espera caduca en el acto, así que elegirlo es
/// elegir fallar —y falla en alto, con el error de estancamiento, que es la dirección segura—.
///
/// # Si el archivado se rinde, el `enviar` del productor falla
///
/// Cuando el archivado termina antes de tiempo —se rinde, o ya no lee más— el lector se suelta,
/// el canal se desconecta y el `enviar` del productor devuelve un **error**. No es un fallo suyo
/// ni una razón para entrar en pánico: significa «el archivado se rindió», y lo que toca es
/// terminar el bucle y morir. Un productor que entrara en pánico ahí dejaría su hilo envenenado
/// y la unión del punto 3 se colgaría, con lo que el bloqueo que se quería evitar volvería por
/// la puerta de atrás.
pub struct TunelDelVolcado {
    /// La mitad que se le entrega a la biblioteca de copia.
    pub origen: OrigenDelVolcado,
    /// La mitad por la que el productor empuja los bytes, y que hay que mover a su hilo.
    pub productor: ProductorDelVolcado,
    /// La señal de si el volcado llegó **entero**, para consultarla cuando el archivado vuelva.
    ///
    /// Ver [`EntregaDelVolcado`], que es también donde está escrito por qué es un tipo propio y
    /// no un [`Arc<AtomicBool>`] suelto.
    pub entrega: EntregaDelVolcado,
    /// La cuenta de los bytes que el productor ha entregado al canal.
    ///
    /// Ver [`AvanceDelVolcado`], que es también donde está escrito por qué es un tipo propio y
    /// no un [`Arc<AtomicU64>`] suelto: es el otro acceso que tiene que sobrevivir al
    /// movimiento del productor a su hilo.
    pub avance: AvanceDelVolcado,
}

/// La señal de que el volcado se entregó **entero**.
///
/// La pone el lector —y **solo** en el final bueno: canal cerrado **y** la señal del productor
/// puesta—, y la consulta quien archivó. Existe porque no hay otra forma de saberlo: el lector
/// vive dentro de un [`ReadSourceEntry`] que la biblioteca se queda, así que el motor nunca lo
/// ve, y el error del lector **no** sirve de señal —la biblioteca lo descarta con un aviso y
/// `archive()` devuelve `Ok` igualmente, ver el encabezado del módulo—. Esta marca es lo que
/// permite que el motor **no** dé por buena una copia cuyo archivado terminó en `Ok` pese a que
/// el volcado murió a mitad, o a que el productor se olvidó de declarar el final.
///
/// # Por qué es un tipo y no un `Arc<AtomicBool>` suelto
///
/// Porque el acceso tiene que sobrevivir al **movimiento** de [`TunelDelVolcado::productor`]: el
/// extremo del productor se mueve al hilo que produce, y después de ese movimiento ya no se puede
/// pedir `&self` del túnel —`error[E0382]: borrow of partially moved value`—, así que un acceso
/// del estilo `TunelDelVolcado::entrega_completa(&self)` sería inservible justo en el flujo para
/// el que existe. Envolver el [`Arc<AtomicBool>`] deja el acceso donde sí se puede llamar: en el
/// valor de la señal, que se saca del túnel al desestructurarlo o se usa por su campo.
#[derive(Clone)]
pub struct EntregaDelVolcado(Arc<AtomicBool>);

impl EntregaDelVolcado {
    /// ¿Se entregó el volcado **entero**? Falso mientras el lector no llegue al final bueno.
    ///
    /// Mira la señal que el lector marca al ver el canal cerrado **y** el final declarado por el
    /// productor. Un `Ok` de `archive()` **no** implica que esto sea `true`: son dos preguntas
    /// distintas, y esta es la única que habla del volcado. Y hay que preguntarlo **después** de
    /// que el archivado haya vuelto —y de que el hilo del productor se haya unido—: antes, un
    /// `false` no significa «truncado», significa «todavía no».
    pub fn completa(&self) -> bool {
        // `Acquire` y no `Relaxed`: la marca la escribe el hilo del archivado y la lee el del
        // motor. Nada viaja con ella —no hay datos que publicar—, así que `Relaxed` bastaría
        // por lo que hace el hardware; el par `Release`/`Acquire` se usa porque el coste es
        // nulo y deja el traspaso entre hilos sin depender de ese razonamiento.
        self.0.load(Ordering::Acquire)
    }

    /// Marca la entrega como completa. Solo la llama el lector, en el final bueno.
    fn marcar(&self) {
        // `Release`, que es la otra mitad del `Acquire` de [`completa`](Self::completa).
        self.0.store(true, Ordering::Release);
    }
}

impl std::fmt::Debug for EntregaDelVolcado {
    /// Publica el valor de la señal y nada más: no hay nada más que publicar.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntregaDelVolcado")
            .field("completa", &self.completa())
            .finish()
    }
}

/// La cuenta de los bytes que el productor ha empujado al canal.
///
/// Es lo que el motor publica como `bytes_processed` de un volcado, y su razón de vivir aquí
/// es que este es el **único** punto por el que pasa cada byte del volcado: el motor, desde
/// fuera, solo vería el total cuando el archivado acabara… y no lo vería siquiera, porque el
/// tamaño de un volcado por `stdout` no se conoce hasta que termina. La cuenta nace donde los
/// bytes pasan.
///
/// Y es **en vivo**: crece mientras el volcado corre y se puede consultar desde otro hilo, que
/// es lo que el motor necesita para publicar el avance sin esperar al final de nadie.
///
/// # Qué cuenta exactamente
///
/// Lo **entregado al canal**, y nada más: un trozo que el productor no llegó a empujar porque
/// el archivado se rindió no suma —el `enviar` devolvió error— y un trozo vacío tampoco —no se
/// envía—. Por eso la suma va **después** de que el canal acepte el trozo: lo que se cuenta es
/// lo entregado, no lo intentado.
///
/// Lo que **no** cuenta, y conviene no leerlo al revés nunca: no son los bytes que han llegado
/// al repositorio. Eso no lo observa ningún sitio desde aquí, y publicarlo como si fuera eso
/// sería describir un hecho que este puente no mide.
///
/// # Por qué es un tipo y no un `Arc<AtomicU64>` suelto
///
/// Por la misma razón que [`EntregaDelVolcado`], y está contada allí con detalle: el acceso
/// tiene que sobrevivir al **movimiento** de [`TunelDelVolcado::productor`] al hilo que produce,
/// y después de ese movimiento ya no se puede pedir `&self` del túnel
/// —`error[E0382]: borrow of partially moved value`—. Envolver el [`Arc<AtomicU64>`] deja el
/// acceso donde sí se puede llamar: en el valor, que se saca del túnel al desestructurarlo.
#[derive(Clone)]
pub struct AvanceDelVolcado(Arc<AtomicU64>);

impl AvanceDelVolcado {
    /// Los bytes que el productor ha entregado al canal hasta ahora.
    ///
    /// Crece mientras el volcado corre y se puede consultar desde otro hilo: es lo que el motor
    /// publica **en vivo** como `bytes_processed`. Lo que devuelve nunca retrocede, y lo que
    /// devuelva **antes** de que el productor acabe es un avance, no un total.
    pub fn bytes(&self) -> u64 {
        // `Relaxed` y no `Acquire`: esto no es una señal que sincronice nada —no publica datos
        // que otro hilo vaya a usar—, es un contador que se lee para pintar. Lo único que se
        // afirma de él es que no retrocede, y eso lo da la operación atómica por sí sola. La
        // señal de la entrega, en cambio, usa `Acquire`/`Release` a propósito: ver
        // [`EntregaDelVolcado`].
        self.0.load(Ordering::Relaxed)
    }

    /// Suma un trozo **ya entregado** al canal. Solo lo llama
    /// [`ProductorDelVolcado::enviar`], y solo después de que el canal lo acepte.
    fn sumar(&self, bytes: u64) {
        self.0.fetch_add(bytes, Ordering::Relaxed);
    }
}

impl std::fmt::Debug for AvanceDelVolcado {
    /// Publica la cuenta y nada más: no hay nada más que publicar, y desde luego no el volcado.
    ///
    /// Se escribe a mano y no se deriva por la misma razón que el de
    /// [`EntregaDelVolcado`]: lo que hay aquí dentro es un contador compartido, y lo que un
    /// diagnóstico necesita saber de él es cuánto lleva contado.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AvanceDelVolcado")
            .field("bytes", &self.bytes())
            .finish()
    }
}

/// El extremo por el que el productor empuja los bytes: el **único** que existe.
///
/// No se puede clonar a propósito. El canal se cierra cuando muere el **último** emisor, y
/// mientras uno viva el lector espera —hasta el plazo, ahora, pero espera—; que este tipo sea el
/// único que tiene el extremo, y que [`terminar`](Self::terminar) lo consuma, es lo que hace
/// imposible el cuelgue por un emisor retenido. Añadirle `Clone` no rompería ninguna prueba de
/// comportamiento: rompería esa propiedad, y la prueba `el_extremo_del_productor_no_se_puede_clonar`
/// existe justo para cazarlo.
pub struct ProductorDelVolcado {
    /// El nombre del volcado, para poder nombrarlo en el motivo de un fallo.
    nombre: String,
    /// El extremo de envío, que es lo que cierra el canal al morir.
    emisor: SyncSender<Vec<u8>>,
    /// La señal que [`terminar`](Self::terminar) pone y que el lector consulta al ver el cierre.
    terminado_bien: Arc<AtomicBool>,
    /// La cuenta de lo entregado, que el motor consulta **en vivo**.
    ///
    /// Es la otra punta de [`AvanceDelVolcado`]: se guarda aquí porque este es el único sitio
    /// que escribe en el canal, y se entrega también en el túnel porque quien la consulta
    /// —el motor— no tiene el productor: lo movió a su hilo.
    avance: AvanceDelVolcado,
}

impl std::fmt::Debug for ProductorDelVolcado {
    /// Publica el nombre del volcado y nada de lo que viaja por el canal.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductorDelVolcado")
            .field("nombre", &self.nombre)
            .finish_non_exhaustive()
    }
}

impl ProductorDelVolcado {
    /// Empuja un trozo de volcado. **Bloquea** si el canal está lleno: esa espera es la
    /// contrapresión que acota la memoria, no un problema.
    ///
    /// Devuelve error solo si el archivado se ha rendido y ya nadie lee
    /// ([`Error::ElArchivadoSeRindio`]): no es un fallo del productor ni una razón para entrar en
    /// pánico —significa «el archivado se rindió»—, y lo que toca es terminar el bucle y morir.
    ///
    /// Un trozo **vacío** no se envía: el canal solo transporta datos, y un mensaje sin datos no
    /// puede significar nada —el final ya no viaja por aquí—. Descartarlo en el único sitio que
    /// escribe en el canal es lo que permite que el lector trate cualquier mensaje recibido como
    /// progreso, sin tener que decidir qué hacer con un vacío. Y tampoco cuenta: lo que se cuenta
    /// es lo **entregado**, y por aquí no se entregó nada.
    ///
    /// La cuenta va **después** del `send`, no antes: el `send` puede fallar —el archivado se
    /// rindió—, y un trozo que no llegó al canal no es un byte entregado. Contarlo antes haría
    /// que el motor publicara un avance que el repositorio no vio, y hacia arriba, que es la
    /// dirección peor.
    pub fn enviar(&self, trozo: Vec<u8>) -> Result<(), Error> {
        if trozo.is_empty() {
            return Ok(());
        }

        // La longitud se lee antes de mover el trozo al canal, que es lo único que el `send`
        // consume. No es una copia: es un `usize`.
        let bytes = trozo.len() as u64;

        self.emisor
            .send(trozo)
            .map_err(|_| Error::ElArchivadoSeRindio {
                nombre: self.nombre.clone(),
            })?;

        self.avance.sumar(bytes);

        Ok(())
    }

    /// Declara el volcado **terminado bien** y cierra el canal. Consume: después de esto no
    /// queda ningún emisor, que es lo que hace que el lector deje de esperar.
    ///
    /// El orden de las dos cosas es el que importa: primero la señal —con `Release`, para que el
    /// lector, que la lee con `Acquire` cuando ve el canal cerrado, la vea puesta— y después el
    /// `drop` del extremo, que es lo que cierra el canal. Al revés, el lector podría ver el canal
    /// cerrado antes que la señal y dar por truncado un volcado que llegó entero.
    ///
    /// No devuelve `Result`: aquí no hay nada que pueda fallar. Que el archivado se haya rendido
    /// se consulta donde se consulta todo, en [`EntregaDelVolcado::completa()`], que se queda en
    /// falso justo porque nadie leyó.
    ///
    /// Y el defecto va en la dirección segura: quien termina su bucle **sin** llamar a esto
    /// —porque murió, o porque se olvidó— deja la señal en falso, y el lector tratará el cierre
    /// del canal como un volcado **truncado**. Falla cerrado. Lo que sí es una entrega completa
    /// es declarar el final tras cero bytes: aquí solo se sabe cuántos bytes llegaron, y quien
    /// decide si un volcado vacío es un problema es la extracción, que mira el código de salida y
    /// el `stderr` del proceso.
    pub fn terminar(self) {
        self.terminado_bien.store(true, Ordering::Release);

        // El `drop` de `self` —al final de esta función, con el extremo de envío dentro— es lo
        // que cierra el canal. Y el orden no es negociable: primero la señal, después el cierre.
    }
}

/// Construye las dos mitades de un túnel de volcado, más la señal de la entrega completa.
///
/// `nombre` es la identidad del volcado —no existe en ningún sistema de ficheros, así que su
/// único nombre es el que le da quien lo pide—, `capacidad` es el número de trozos que puede
/// retener el canal antes de que el productor tenga que esperar, e `inactividad_maxima` es el
/// plazo que el lector espera al trozo siguiente antes de dar el volcado por estancado. Los dos
/// números entran por parámetro **a propósito**: la capacidad la elige quien conoce el
/// presupuesto de memoria y el plazo quien conoce el presupuesto de tiempo.
///
/// # Precondiciones del nombre
///
/// El nombre hace de vez de dos cosas a la vez: es el `path` de la entrada que se le entrega a
/// la biblioteca **y** el nombre del nodo del volcado en el snapshot. Por eso no vale
/// cualquier cadena, y aquí se rechaza lo que la biblioteca no puede digerir:
///
/// - **vacío** es un `panic` dentro de la biblioteca, no un error suyo: un `Path` vacío
///   no tiene padre y su camino hacia el árbol hace `expect("file path should have a parent!")`;
/// - **`.`** y **`..`** no nombran nada y darían una entrada sin nombre;
/// - con **`/`** o **`\`** el nombre deja de ser un nombre y pasa a ser un camino: la entrada
///   se partiría en trozos y el nodo llevaría un separador dentro de su propio nombre.
///
/// Lo que sí vale es un nombre normal —`volcado-de-tienda.sql`—: se archiva como un fichero
/// suelto y la biblioteca le da por padre el camino vacío, que es la raíz del snapshot.
///
/// Un origen es de **un solo uso**: cada intento de copia construye su propio túnel, porque
/// [`ReadSource::entries`] solo se puede pedir una vez.
///
/// # El productor tiene que correr en otro hilo, y el lector tiene su plazo
///
/// Con el canal acotado, un productor que corra en el mismo hilo que llama a `archive()` se
/// bloquea en el `send` esperando hueco mientras el consumidor espera al productor, y el bloqueo
/// es mutuo y permanente: el plazo del lector no lo cura, porque el plazo caduca en el lector y
/// el productor sigue esperando hueco. El orden es: productor en su hilo, archivado en el suyo,
/// unión al final. Está contado con más detalle en el rustdoc de [`TunelDelVolcado`], que es el
/// que hay que leer antes de escribir al otro lado.
///
/// Del mismo sitio salen las otras dos trampas del productor: cuando el archivado se rinde, el
/// `enviar` devuelve un **error**, y ese error significa «el archivado se rindió» —el bucle tiene
/// que terminar—, no un fallo del productor; entrar en pánico ahí deja el hilo envenenado y la
/// unión se cuelga. Y terminar el bucle **sin** [`ProductorDelVolcado::terminar`] hace que el
/// volcado se dé por truncado: el silencio no es un final.
///
/// # Errors
///
/// Devuelve [`Error::NombreDeVolcadoInvalido`] si el nombre no sirve como nombre de fichero.
/// Se valida aquí, una vez, y no en [`ReadSource::entries`]: allí ya sería tarde —la biblioteca
/// filtra el error y archiva lo que haya— y esto es una precondición del motor, no un fallo de
/// la copia.
pub fn tunel_del_volcado(
    nombre: impl Into<String>,
    capacidad: usize,
    inactividad_maxima: Duration,
) -> Result<TunelDelVolcado, Error> {
    let nombre = nombre.into();
    validar_nombre_de_volcado(&nombre)?;

    // `sync_channel` y no `channel`: es la diferencia entre un canal que acota y uno que no.
    let (emisor, receptor) = sync_channel(capacidad);

    // Las dos señales: la del productor, que el lector consulta al ver el canal cerrado, y la de
    // la entrega completa, que el lector marca y el motor consulta. Nacen aquí y a partir de este
    // momento van por caminos distintos —el origen se le entrega a la biblioteca, el productor a
    // su hilo y las señales al motor—, que es lo que hace que estas cuatro líneas sean la única
    // costura entre las tres partes. Y con ellas la **cuenta de lo entregado**, que es la tercera
    // pieza que va por caminos distintos: la escribe el productor y la lee el motor en vivo.
    let terminado_bien = Arc::new(AtomicBool::new(false));
    let entrega = EntregaDelVolcado(Arc::new(AtomicBool::new(false)));
    let avance = AvanceDelVolcado(Arc::new(AtomicU64::new(0)));

    Ok(TunelDelVolcado {
        origen: OrigenDelVolcado {
            nombre: nombre.clone(),
            receptor: Mutex::new(Some(receptor)),
            terminado_bien: Arc::clone(&terminado_bien),
            entrega: entrega.clone(),
            inactividad_maxima,
        },
        productor: ProductorDelVolcado {
            nombre,
            emisor,
            terminado_bien,
            avance: avance.clone(),
        },
        entrega,
        avance,
    })
}

/// Comprueba que el nombre del volcado sirve como nombre de fichero.
///
/// El motivo de cada rechazo está en el rustdoc de [`tunel_del_volcado`]: aquí solo se decide,
/// porque este es el único punto del módulo en el que un nombre malo todavía se puede convertir
/// en un error en vez de en un pánico de la biblioteca o en un nombre raro dentro de un
/// snapshot.
fn validar_nombre_de_volcado(nombre: &str) -> Result<(), Error> {
    // `.` y `..` van con el vacío porque los tres son el mismo fallo: una entrada sin nombre.
    let sin_nombre = nombre.is_empty() || nombre == "." || nombre == "..";
    let es_un_camino = nombre.contains('/') || nombre.contains('\\');

    if sin_nombre || es_un_camino {
        return Err(Error::NombreDeVolcadoInvalido {
            nombre: nombre.to_owned(),
        });
    }

    Ok(())
}

/// La mitad que implementa [`ReadSource`]: lo que la biblioteca de copia recibe.
///
/// Cabe en un `Sync + Send` —que es lo que el trait exige— porque lo único que comparte con
/// otros hilos es el [`Mutex`] con el receptor dentro y las dos señales atómicas.
pub struct OrigenDelVolcado {
    /// El nombre con el que el motor pidió el volcado: es su única identidad, porque no hay
    /// ningún fichero detrás. Ya viene validado por [`tunel_del_volcado`].
    nombre: String,
    /// El receptor, dentro de un candado y de un `Option`.
    ///
    /// El candado no es un adorno: [`ReadSource`] exige `Sync` y un [`Receiver`] no lo es. Y el
    /// `Option` está porque [`ReadSource::entries`] recibe `&self` mientras que el receptor hay
    /// que **sacarlo** para entregárselo al lector, así que la operación es un `.take()`.
    receptor: Mutex<Option<Receiver<Vec<u8>>>>,
    /// La señal que el productor pone al declarar el final, para entregársela al lector.
    ///
    /// Vive también aquí —y no solo en el [`ProductorDelVolcado`]— porque el lector se construye
    /// en [`ReadSource::entries`], que es lo único que este lado puede hacer: sin esta copia del
    /// `Arc`, el lector no tendría forma de saber si el cierre del canal es un final o una caída.
    terminado_bien: Arc<AtomicBool>,
    /// La señal de la entrega completa, que comparte con el túnel para entregársela al lector.
    ///
    /// Misma razón que la anterior, y misma dirección: el lector la **marca** y el motor la
    /// consulta, así que las dos puntas van por caminos distintos desde [`tunel_del_volcado`].
    entrega: EntregaDelVolcado,
    /// El plazo que el lector espera al trozo siguiente. Lo eligió el motor.
    inactividad_maxima: Duration,
}

impl OrigenDelVolcado {
    /// El nombre con el que el motor pidió este volcado: es la identidad que el puente
    /// **declara**, y la que el nodo tendrá dentro del árbol de la instantánea.
    ///
    /// Existe porque el nombre del origen y el de la copia vivían en dos sitios y nada
    /// obligaba a que coincidieran: [`Copia::nombre`](crate::rustic::Copia) es lo que la
    /// instantánea **declara** contener y este es lo que el árbol **contiene de verdad**,
    /// así que si divergen la instantánea miente sobre su propio contenido —el mismo fallo
    /// que el módulo existe para no cometer, en versión de metadatos—. Publicarlo es lo que
    /// permite compararlos antes de archivar:
    /// [`archivar_volcado`](crate::rustic::Repositorio::archivar_volcado) lo hace y rechaza
    /// la copia entera si no coinciden.
    ///
    /// Lo que valida [`tunel_del_volcado`] no cubre esto: allí se comprueba que el nombre
    /// **sirva** como nombre de fichero, no que sea el que la copia dice.
    pub fn nombre(&self) -> &str {
        &self.nombre
    }
}

impl std::fmt::Debug for OrigenDelVolcado {
    /// Publica el nombre del volcado y nada del receptor ni de lo que viaja por él.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrigenDelVolcado")
            .field("nombre", &self.nombre)
            .finish_non_exhaustive()
    }
}

/// El lector del volcado: lo que `ReadSourceEntry::open` entrega a la biblioteca.
///
/// Implementa [`Read`] a mano, y no se implementa [`rustic_core::ReadSourceOpen`] porque ese
/// trait tiene un *blanket impl* para todo `Read + Send + 'static`: cumplir el segundo es
/// cumplir los dos.
pub struct LectorDeVolcado {
    /// De dónde salen los trozos. Ya no lo tiene el origen: se entregó con `.take()`.
    receptor: Receiver<Vec<u8>>,
    /// El trozo parcial del mensaje anterior: lo que no cupo en el búfer del consumidor.
    ///
    /// Es el estado que hace que este lector sea correcto: cuando el consumidor pide 5 bytes y
    /// el mensaje trae 100, los otros 95 **se guardan aquí** para la lectura siguiente. Tirarlos
    /// entregaría un volcado incompleto y la copia parecería buena.
    sobrante: Vec<u8>,
    /// Cuántos bytes de [`LectorDeVolcado::sobrante`] ya se entregaron.
    ///
    /// Se avanza el índice en vez de recortar el vector: mover el trozo entero en cada lectura
    /// convertiría una lectura de 5 bytes en una copia de todo lo que queda.
    consumido: usize,
    /// El nombre del volcado, para poder nombrarlo en el motivo de un fallo.
    nombre: String,
    /// La señal del productor: `true` cuando declaró el final **antes** de cerrar el canal.
    ///
    /// Es la marca que distingue un final bueno —`Ok(0)`— de un productor que se murió a mitad
    /// —un [`std::io::Error`]—. Lo que decide es esta señal, no el estado del canal: los dos
    /// productores dejan el canal cerrado.
    terminado_bien: Arc<AtomicBool>,
    /// La señal que este lector marca al llegar al final bueno, para que el motor la consulte.
    entrega: EntregaDelVolcado,
    /// El plazo máximo que una lectura espera al trozo siguiente, elegido por el motor.
    inactividad_maxima: Duration,
}

impl Read for LectorDeVolcado {
    /// Entrega hasta `buf.len()` bytes del volcado, esperando al productor si hace falta.
    ///
    /// **Solo dos desenlaces devuelven `Ok(0)`**: el búfer vacío —que el contrato de [`Read`]
    /// exige contestar así aunque no se haya consumido nada, y que por eso **no** marca la señal
    /// de la entrega— y el **final bueno**, que es el canal cerrado **con** la señal del
    /// productor puesta. Todo lo demás es error, y cada uno nombra el volcado:
    ///
    /// - un canal cerrado **sin** la señal es un volcado **truncado**
    ///   ([`std::io::ErrorKind::UnexpectedEof`]): el productor murió —el `exec` de la extracción
    ///   se cayó, el hilo se fue con un pánico— o se olvidó de declarar el final. Devolver `Ok(0)`
    ///   aquí **archiva un volcado truncado como si fuera válido**, que es el peor fallo posible
    ///   de este módulo;
    /// - y dejar pasar `inactividad_maxima` sin recibir un solo byte es un **estancamiento**
    ///   ([`std::io::ErrorKind::TimedOut`], [`Error::VolcadoEstancado`]), que es lo que impide
    ///   que `archive()` —una llamada **bloqueante**— se quede esperando para siempre a un
    ///   productor atascado o muerto cuyo extremo de envío siga vivo en alguna parte.
    ///
    /// El canal solo lleva trozos **con** datos: [`ProductorDelVolcado::enviar`] descarta los
    /// vacíos y es el único que escribe, así que un mensaje recibido es siempre progreso y no hay
    /// ningún caso de «mensaje que no dice nada» que haya que desambiguar.
    ///
    /// # Y ese error no detiene la copia
    ///
    /// El error es lo correcto por parte del lector, pero **la biblioteca de copia no lo
    /// propaga**: está medido en su `archiver.rs`, donde los errores que salen del archivado de
    /// ficheros entran por un `filter_map` que hace `warn!("ignoring error: …")` y sigue. O sea
    /// que este error **se descarta con un aviso** y `archive()` devuelve `Ok`, con el snapshot
    /// guardado y sin el volcado dentro —la entrada que falló no llega al árbol—. Y en el peor
    /// caso medido ni siquiera hay aviso: con el nodo del volcado ya presente en un snapshot
    /// padre, el árbol del snapshot nuevo queda igual que el del padre y el `Ok` sale limpio, de
    /// modo que un volcado que no se generó se convierte en una copia válida de los datos
    /// anteriores. Quien archiva **no puede** deducir de un `Ok` de `archive()` que el volcado
    /// esté completo: la única verdad fiable está del lado del productor, y la forma de
    /// consultarla es [`EntregaDelVolcado::completa()`], que este `read` marca justo en el final
    /// bueno y en ningún otro sitio. Sin esa comprobación, un volcado que murió a mitad y una
    /// copia buena son indistinguibles desde fuera.
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // El contrato de `Read` obliga a contestar `Ok(0)` con un búfer vacío, y además pedir un
        // mensaje que no cabe en ningún sitio sería retenerlo sin destino. Es el único `Ok(0)`
        // que no significa «el volcado se acabó», y también el único que **no** marca la entrega
        // como completa: aquí no se ha consumido nada. Y contesta en el acto: ni siquiera espera
        // al plazo, que es lo que fija la prueba del búfer vacío.
        if buf.is_empty() {
            return Ok(0);
        }

        // 1. Primero lo que sobró del mensaje anterior. Antes de pedir otro, siempre: al
        //    revés, el trozo parcial se perdería.
        if self.consumido < self.sobrante.len() {
            let pendientes = &self.sobrante[self.consumido..];
            let a_entregar = pendientes.len().min(buf.len());

            buf[..a_entregar].copy_from_slice(&pendientes[..a_entregar]);
            self.consumido += a_entregar;

            if self.consumido == self.sobrante.len() {
                self.sobrante.clear();
                self.consumido = 0;
            }

            return Ok(a_entregar);
        }

        // 2. Con plazo y no con `recv()`: mientras viva un emisor —aunque sea uno retenido y
        //    sin usar— `recv()` no vuelve, y `archive()` es bloqueante. El plazo se cuenta
        //    desde aquí en cada lectura, así que lo que acota es el silencio del productor y
        //    no la duración del volcado: uno lento que avanza no falla nunca.
        match self.receptor.recv_timeout(self.inactividad_maxima) {
            Ok(datos) => {
                // Un mensaje con datos: el trozo vacío no llega hasta aquí, porque `enviar`
                // no lo manda. Lo que no quepa en el búfer no se tira.
                let a_entregar = datos.len().min(buf.len());
                buf[..a_entregar].copy_from_slice(&datos[..a_entregar]);

                if a_entregar < datos.len() {
                    self.sobrante = datos;
                    self.consumido = a_entregar;
                }

                Ok(a_entregar)
            }
            Err(RecvTimeoutError::Timeout) => Err(self.volcado_estancado()),
            Err(RecvTimeoutError::Disconnected) => {
                // El canal se cerró. Lo que decide es la señal del productor, nunca el estado
                // del canal: los dos productores —el que declara el final y el que se cae—
                // dejan el canal cerrado igual.
                if self.terminado_bien.load(Ordering::Acquire) {
                    // El final bueno: el productor declaró que había acabado y soltó el canal,
                    // y el canal se ha drenado entero —el receptor entrega lo que quede en cola
                    // antes de decir que está desconectado—. Es aquí, y solo aquí, donde la
                    // señal de la entrega completa puede ponerse: este es el único sitio del
                    // módulo que ve un final bueno, y el motor no tiene otra forma de enterarse.
                    self.entrega.marcar();
                    Ok(0)
                } else {
                    // El productor murió antes de acabar —el `exec` de la extracción se cayó,
                    // el hilo se fue con un pánico— o se olvidó de declarar el final: lo que
                    // hay es un volcado **truncado**. Devolver `Ok(0)` aquí es el peor fallo
                    // posible de este módulo: la biblioteca lo leería como «el volcado se
                    // acabó» y daría por buena una copia que existe, ocupa sitio y no se
                    // puede restaurar.
                    //
                    // El error es la respuesta correcta del lector, pero **no** es una alarma
                    // que llegue a ninguna parte: la biblioteca lo descarta con un
                    // `warn!("ignoring error: …")` —o sin aviso siquiera, en el peor caso
                    // medido— y sigue, así que el archivado acaba en `Ok` con el snapshot
                    // guardado sin el volcado dentro. Quien escriba el archivado no puede
                    // deducir de ese `Ok` que el volcado esté completo: lo que tiene que
                    // mirar es [`EntregaDelVolcado::completa()`], que aquí se queda en falso.
                    // Ver el comentario de [`Read::read`] para el detalle medido.
                    Err(self.canal_cerrado_sin_final())
                }
            }
        }
    }
}

impl LectorDeVolcado {
    /// El motivo de un productor que cerró el canal sin declarar el final.
    ///
    /// Nombra el volcado y dice qué se teme —que está truncado—, porque el operador que lea
    /// esto tiene que poder decidir si reintenta la copia o mira el contenedor de la base.
    fn canal_cerrado_sin_final(&self) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!(
                "el productor del volcado `{}` soltó el canal sin declarar el final: el volcado \
                 puede estar truncado, y archivarlo daría por buena una copia incompleta",
                self.nombre
            ),
        )
    }

    /// El motivo de un productor que lleva demasiado tiempo sin entregar un solo byte.
    ///
    /// Reutiliza el error del módulo ([`Error::VolcadoEstancado`]) en vez de componer otro texto:
    /// el mismo desenlace se lee igual desde el productor —que ve el error del módulo— y desde el
    /// registro de la biblioteca, que ve este [`std::io::Error`].
    fn volcado_estancado(&self) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            Error::VolcadoEstancado {
                nombre: self.nombre.clone(),
                inactividad: self.inactividad_maxima,
            },
        )
    }
}

impl ReadSource for OrigenDelVolcado {
    type Open = LectorDeVolcado;
    // El iterador es un `Vec::IntoIter` porque [`ReadSource::entries`] tiene que contestar con
    // **un** tipo, y hay dos desenlaces que devolver: la entrada o un error. Con un `Vec` de
    // cero o de un elemento, los dos caben en el mismo tipo.
    type Iter = std::vec::IntoIter<RusticResult<ReadSourceEntry<Self::Open>>>;

    /// El tamaño del volcado: **desconocido**, y por eso `Ok(None)`.
    ///
    /// No es un hueco que rellenar más tarde: nadie sabe cuánto va a pesar un `mariadb-dump`
    /// hasta que termina —la biblioteca hace lo mismo con su `StdinSource`—, y devolver `0`
    /// diría «un volcado vacío», que es lo contrario. El `size()` de un directorio sí lo sabe,
    /// pero eso es otra clase de origen. Esta respuesta es la causa directa de que `percentage`
    /// sea anulable en el evento del §5: un porcentaje sin denominador es una cifra inventada.
    ///
    /// El tipo de vuelta admite error por la firma del trait; aquí no hay nada que falle.
    fn size(&self) -> RusticResult<Option<u64>> {
        Ok(None)
    }

    /// Entrega el volcado como **una** entrada, y **solo se puede llamar una vez**.
    ///
    /// El receptor sale del [`Mutex`] con `.take()`: es el precio de que `entries(&self)` reciba
    /// `&self` mientras que el receptor se entrega al lector. Es el mismo truco que usa el
    /// `ChildStdoutSource` de la biblioteca con su `Mutex<Child>`.
    ///
    /// La segunda llamada devuelve un iterador de un elemento que **es un error**, nunca uno
    /// vacío. A quien protege ese error es **al llamador**, no al archivado: si el motor pide
    /// las entradas dos veces es que se ha equivocado él —un origen es de un solo uso, y cada
    /// intento de copia construye su propio túnel con [`tunel_del_volcado`]—, y un motor mal
    /// escrito tiene que enterarse. Lo que **no** es, es una barrera: si esa segunda llamada
    /// ocurriera con el archivado en marcha, la biblioteca descartaría el error con un aviso y
    /// seguiría con **cero entradas**, archivando un snapshot vacío y dándolo por bueno. Ver el
    /// punto 4 del encabezado del módulo.
    ///
    /// El nombre del volcado ya está validado por [`tunel_del_volcado`], así que aquí se puede
    /// usar tal cual como `path` de la entrada y como nombre del nodo.
    fn entries(&self) -> Self::Iter {
        let receptor = match self.receptor.lock() {
            Ok(mut guardado) => guardado.take(),
            // Un candado envenenado quiere decir que otro hilo se cayó teniéndolo tomado. El
            // receptor sigue siendo tan válido como era, así que se recupera y se sigue: aquí no
            // se entra en pánico —seguir con un `.unwrap()` convertiría el pánico de otro hilo
            // en un volcado que no se archiva— y tampoco se disfraza de error, porque no lo es.
            Err(veneno) => veneno.into_inner().take(),
        };

        let Some(receptor) = receptor else {
            return vec![Err(error_de_un_solo_uso(&self.nombre))].into_iter();
        };

        // El volcado no existe en ningún sistema de ficheros de Kesto, así que su camino es su
        // nombre: es el que el motor le puso al pedirlo y el que aparecerá en el snapshot.
        let path = PathBuf::from(&self.nombre);
        let node = Node::new_node(
            OsStr::new(self.nombre.as_str()),
            NodeType::File,
            Metadata::default(),
        );

        let lector = LectorDeVolcado {
            receptor,
            sobrante: Vec::new(),
            consumido: 0,
            nombre: self.nombre.clone(),
            // Las dos señales y el plazo viajan con el lector: es el hilo que hay entre él y el
            // motor, en un sentido para la señal de la entrega y en el otro para la del final.
            terminado_bien: Arc::clone(&self.terminado_bien),
            entrega: self.entrega.clone(),
            inactividad_maxima: self.inactividad_maxima,
        };

        vec![Ok(ReadSourceEntry {
            path,
            node,
            open: Some(lector),
        })]
        .into_iter()
    }
}

/// El error de pedir las entradas de un origen que ya las entregó.
///
/// Protege al **llamador** y no al archivado, y por eso el motivo importa: el único sitio donde
/// este texto se va a leer es el `warn!("ignoring error: …")` con el que la biblioteca descarta
/// el error —y, con él, toda la entrada—, así que tiene que decir por sí solo que quien se
/// equivocó fue quien pidió las entradas dos veces y no el volcado.
fn error_de_un_solo_uso(nombre: &str) -> Box<RusticError> {
    RusticError::new(
        ErrorKind::Internal,
        format!(
            "el origen del volcado `{nombre}` ya entregó su entrada: las entradas de un volcado \
             se piden una sola vez, y una segunda llamada no puede devolver un origen vacío"
        ),
    )
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::marker::PhantomData;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use rustic_core::ReadSource as _;
    use rustic_core::ReadSourceOpen;
    use rustic_core::RusticResult;
    use rustic_core::repofile::NodeType;

    use super::{
        Error, LectorDeVolcado, OrigenDelVolcado, ProductorDelVolcado, TunelDelVolcado,
        tunel_del_volcado,
    };

    /// El plazo de las pruebas que **no** miden el plazo.
    ///
    /// Un minuto: tan por encima de lo que tardan que si alguna lo tocara sería que el puente
    /// está atascado, no que la prueba va despacio.
    const PLAZO_DE_PRUEBA: Duration = Duration::from_secs(60);

    /// Un túnel de prueba, ya construido, con el plazo largo de [`PLAZO_DE_PRUEBA`].
    ///
    /// El `expect` de la validación del nombre vive aquí y no en cada prueba porque lo que estas
    /// pruebas fijan es el puente: la validación tiene las suyas más abajo, con sus nombres
    /// imposibles uno por uno.
    fn tunel_de_prueba(nombre: &str, capacidad: usize) -> TunelDelVolcado {
        tunel_con_plazo(nombre, capacidad, PLAZO_DE_PRUEBA)
    }

    /// El mismo túnel, con el plazo que la prueba necesite medir.
    fn tunel_con_plazo(
        nombre: &str,
        capacidad: usize,
        inactividad_maxima: Duration,
    ) -> TunelDelVolcado {
        tunel_del_volcado(nombre, capacidad, inactividad_maxima)
            .expect("el nombre de esta prueba tiene que ser válido")
    }

    /// El nombre que un rechazo señala como inválido.
    ///
    /// Recibe el `Result` entero en vez de usar un `expect_err` porque ese método pide que el
    /// acierto sea `Debug`, y el túnel no lo lleva: lo único que publicaría de sí mismo es el
    /// nombre que el propio error ya trae consigo.
    ///
    /// El `Err(otro)` es un **comodín**, y hay que decirlo porque la versión anterior de este
    /// comentario prometía lo contrario: `Error` tiene ahora **diez** variantes —las siete que
    /// trajo la segunda mitad de este módulo están en `error.rs`— y el compilador no señaló
    /// este ayudante al añadirlas, porque el comodín se las traga todas. Lo que este ayudante
    /// comprueba es una sola cosa —que un nombre rechazado no llegue como otro fallo—, y para
    /// eso el comodín basta; lo que **no** hace es avisar por compilación de una variante
    /// nueva, así que quien añada una que también signifique «el nombre no sirve» tiene que
    /// acordarse de mirar aquí.
    fn nombre_rechazado(tunel: Result<TunelDelVolcado, Error>) -> String {
        match tunel {
            Err(Error::NombreDeVolcadoInvalido { nombre }) => nombre,
            Err(otro) => panic!("ese nombre tenía que rechazarse, y salió otro error: {otro}"),
            Ok(_) => panic!("ese nombre tenía que rechazarse"),
        }
    }

    /// Abre el origen pasando por el **mismo** camino que la biblioteca de copia:
    /// `entries()` → la entrada → su `open` → `.open()`.
    ///
    /// Es lo que hace que estas pruebas fijen el contrato y no una comodidad propia: si la
    /// entrada no trajera cómo abrirse, la biblioteca no podría leer nada.
    fn lector_de(origen: &OrigenDelVolcado) -> RusticResult<LectorDeVolcado> {
        let mut entradas = origen.entries();
        let entrada = entradas
            .next()
            .expect("el origen tiene que dar una entrada")
            .expect("la entrada no puede ser un error");
        let abierto = entrada
            .open
            .expect("la entrada tiene que traer cómo abrirse");

        abierto.open()
    }

    /// **1.** El tamaño de un volcado por flujo se declara **desconocido**.
    ///
    /// Nadie sabe cuánto va a pesar un `mariadb-dump` hasta que termina, y un origen que
    /// inventara un total —o que dijera `0`— pintaría una barra de progreso sobre una cifra
    /// falsa. Es la causa directa de que `percentage` sea anulable en el evento del §5.
    #[test]
    fn el_tamano_de_un_volcado_no_se_conoce() {
        let tunel = tunel_de_prueba("volcado-de-prueba.sql", 4);

        let tamano = tunel
            .origen
            .size()
            .expect("preguntar el tamaño no puede fallar");

        assert_eq!(
            tamano, None,
            "un volcado por flujo no puede declarar un total: {tamano:?}"
        );
    }

    /// **2.** El lector entrega los bytes que el productor escribió, **en orden y completos**.
    ///
    /// Con varios trozos y un total conocido: lo leído tiene que ser exactamente lo escrito,
    /// byte a byte. Es la prueba que fija que el puente no reordena, no pierde y no duplica.
    #[test]
    fn el_lector_entrega_los_bytes_del_productor_en_orden() {
        let TunelDelVolcado {
            origen, productor, ..
        } = tunel_de_prueba("volcado-de-tienda.sql", 2);

        // Trozos de tamaño distinto: un trozo grande seguido de uno pequeño caza una
        // implementación que se conforme con entregar el búfer lleno.
        let trozos = [
            b"CREATE TABLE tienda (id INT);\n".to_vec(),
            b"INSERT INTO tienda VALUES (1);\n".to_vec(),
            b"INSERT INTO tienda VALUES (2);\n".to_vec(),
        ];
        let esperado: Vec<u8> = trozos.concat();
        let total = esperado.len();

        let emisor = std::thread::spawn(move || {
            for trozo in trozos {
                productor
                    .enviar(trozo)
                    .expect("el origen sigue vivo, así que el canal no puede estar cerrado");
            }
            productor.terminar();
        });

        let mut lector = lector_de(&origen).expect("el lector tiene que abrirse");
        let mut leido = Vec::new();
        lector
            .read_to_end(&mut leido)
            .expect("con el final declarado, leer no puede fallar");

        emisor.join().expect("el hilo productor no puede caerse");

        assert_eq!(
            leido.len(),
            total,
            "se leyeron {} bytes de {total}",
            leido.len()
        );
        assert_eq!(
            leido, esperado,
            "lo entregado no es lo escrito, o no está en el mismo orden"
        );
    }

    /// **3.** El trozo parcial **no se pierde**.
    ///
    /// El consumidor pide 7 bytes cada vez y los mensajes traen 97 y 131, así que casi ninguna
    /// lectura coincide con un mensaje entero y casi siempre sobra trozo que guardar para la
    /// vuelta siguiente. Es la prueba que caza el fallo clásico de este tipo de lector: tirar lo
    /// que sobra del mensaje, entregar un volcado incompleto y que la copia parezca buena.
    ///
    /// No se afirma que el búfer se llene en cada vuelta: un `read` que devuelve menos de lo que
    /// cabe es legal en [`Read`] y aquí es hasta conveniente —entrega lo que hay sin esperar al
    /// mensaje siguiente, lo que importa cuando el productor va despacio—. Lo que se afirma es
    /// que no se pierde ni un byte, que es la propiedad de verdad.
    #[test]
    fn el_trozo_parcial_no_se_pierde() {
        let TunelDelVolcado {
            origen, productor, ..
        } = tunel_de_prueba("volcado-desigual.sql", 2);

        let trozos = [vec![b'a'; 97], vec![b'b'; 131]];
        let esperado: Vec<u8> = trozos.concat();
        let total = esperado.len();

        let emisor = std::thread::spawn(move || {
            for trozo in trozos {
                productor
                    .enviar(trozo)
                    .expect("el origen sigue vivo, así que el canal no puede estar cerrado");
            }
            productor.terminar();
        });

        let mut lector = lector_de(&origen).expect("el lector tiene que abrirse");
        let mut leido = Vec::new();
        let mut bufer = [0_u8; 7];

        loop {
            let entregados = lector.read(&mut bufer).expect("leer no puede fallar");

            if entregados == 0 {
                break;
            }

            assert!(
                entregados <= bufer.len(),
                "una lectura entregó {entregados} bytes en un búfer de {}",
                bufer.len()
            );

            leido.extend_from_slice(&bufer[..entregados]);
        }

        emisor.join().expect("el hilo productor no puede caerse");

        assert_eq!(
            leido.len(),
            total,
            "se leyeron {} bytes de {total}: el trozo que sobraba de algún mensaje se perdió",
            leido.len()
        );
        assert_eq!(
            leido, esperado,
            "lo leído no coincide con lo escrito: algún trozo parcial se perdió o se desordenó"
        );
    }

    /// **4.** `enviar` **bloquea** cuando el canal está lleno y **no se pierde nada**.
    ///
    /// Fija las dos mitades que estaban rotas en la primera versión del puente. La primera, que
    /// el canal siga acotado: con el consumidor sin leer, el productor se frena en la capacidad
    /// —la cuenta de trozos entregados se queda clavada ahí— y la memoria retenida no crece con
    /// el volcado. La segunda, que nada se pierda: el productor manda más trozos que la capacidad
    /// con el `enviar` **bloqueante** y los bytes leídos cuadran exactos con los escritos.
    ///
    /// Se mide **contando** y no cronometrando, como la prueba que sustituye: la cuenta de
    /// entregas es un hecho, y el único reloj que aparece es el margen para que un canal **sin**
    /// tope tenga tiempo de desbocarse —con tope, el productor está esperando hueco y la cuenta
    /// no puede subir—.
    #[test]
    fn el_enviar_bloquea_cuando_el_canal_esta_lleno_y_no_pierde_nada() {
        let capacidad = 2;
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
            avance: _,
        } = tunel_de_prueba("volcado-acotado.sql", capacidad);

        let trozos: Vec<Vec<u8>> = (0..8).map(|i| vec![i as u8; 4096]).collect();
        let esperado: Vec<u8> = trozos.concat();
        let total = esperado.len();

        // El productor avisa de cada trozo que ha **entregado al canal**: es la cuenta que mide
        // cuánta memoria retiene el túnel.
        let entregados = Arc::new(AtomicUsize::new(0));
        let cuenta_del_hilo = Arc::clone(&entregados);

        let emisor = std::thread::spawn(move || {
            for trozo in trozos {
                productor
                    .enviar(trozo)
                    .expect("el origen sigue vivo, así que el canal no puede estar cerrado");
                cuenta_del_hilo.fetch_add(1, Ordering::SeqCst);
            }
            productor.terminar();
        });

        // Sin consumidor el canal se llena, y el productor se queda esperando hueco: por eso esta
        // espera acaba. Los topes existen para que la prueba no se cuelgue si algo va mal.
        let limite = Instant::now() + Duration::from_secs(10);
        while entregados.load(Ordering::SeqCst) < capacidad {
            assert!(
                !emisor.is_finished(),
                "el productor terminó con menos de {capacidad} trozos entregados: el canal no \
                 está frenando al productor"
            );
            assert!(
                Instant::now() < limite,
                "el canal no se llenó en 10 segundos: la prueba no puede seguir"
            );
            std::thread::yield_now();
        }

        // Y un canal **sin** tope habría dejado al productor terminar los ocho: el margen es para
        // que lo haga si puede. Con tope no puede, porque su `enviar` está esperando hueco.
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            entregados.load(Ordering::SeqCst),
            capacidad,
            "el productor entregó {} trozos con el canal lleno (capacidad {capacidad}): el canal \
             no acota nada y el volcado se acumula en memoria",
            entregados.load(Ordering::SeqCst)
        );

        // Ahora sí, el consumidor: tiene que recibir los ocho trozos, enteros y en orden.
        let mut lector = lector_de(&origen).expect("el lector tiene que abrirse");
        let mut leido = Vec::new();
        let mut bufer = [0_u8; 7];

        loop {
            let entregados = lector
                .read(&mut bufer)
                .expect("con el productor avanzando, leer no puede fallar");

            if entregados == 0 {
                break;
            }

            leido.extend_from_slice(&bufer[..entregados]);
        }

        emisor.join().expect("el hilo productor no puede caerse");

        assert_eq!(
            leido.len(),
            total,
            "se leyeron {} bytes de {total}: con el canal lleno se perdió lo que el productor \
             había entregado",
            leido.len()
        );
        assert_eq!(
            leido, esperado,
            "lo leído no coincide con lo escrito: el productor perdió o reordenó trozos con el \
             canal lleno"
        );
        assert_eq!(
            entregados.load(Ordering::SeqCst),
            8,
            "el productor no entregó los ocho trozos: alguno se quedó por el camino"
        );
        assert!(
            entrega.completa(),
            "el volcado se entregó entero y la señal sigue en falso"
        );
    }

    /// **5.** Un productor que termina **bien** deja leer todo y la entrega queda completa.
    ///
    /// Es el camino bueno de la mecánica nueva, entero: el productor empuja con `enviar`, declara
    /// el final con `terminar`, el lector llega al cierre del canal con la señal puesta y **ahí**
    /// marca la entrega, que es lo que el motor va a consultar. Sin esta prueba, la señal podría
    /// quedarse en falso para siempre sin que nada lo dijera.
    #[test]
    fn un_productor_que_termina_bien_deja_leer_todo_y_la_entrega_queda_completa() {
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
            avance: _,
        } = tunel_de_prueba("volcado-entero.sql", 4);

        assert!(
            !entrega.completa(),
            "un túnel recién hecho ya venía con la entrega completa"
        );

        let esperado = b"CREATE TABLE tienda (id INT);\nINSERT INTO tienda VALUES (1);\n".to_vec();
        let a_enviar = esperado.clone();

        let emisor = std::thread::spawn(move || {
            productor
                .enviar(a_enviar)
                .expect("el canal no puede estar cerrado antes del final");
            productor.terminar();
        });

        let mut lector = lector_de(&origen).expect("el lector tiene que abrirse");
        let mut leido = Vec::new();
        lector
            .read_to_end(&mut leido)
            .expect("con el final declarado, leer no puede fallar");

        emisor.join().expect("el hilo productor no puede caerse");

        assert_eq!(
            leido, esperado,
            "lo leído no es el volcado que se envió, así que la señal hablaría de otra cosa"
        );
        assert!(
            entrega.completa(),
            "el productor declaró el final y el lector consumió el cierre, pero la señal sigue \
             en falso: el motor daría por truncada una copia que llegó entera"
        );
    }

    /// **6.** Un productor que Muere sin declarar el final es un ERROR, **no** un fin de fichero.
    ///
    /// Es la prueba que impide que el lector entregue su parte muda: si devolviera `Ok(0)` aquí,
    /// la biblioteca leería «el volcado se acabó» y daría por buena una copia incompleta que
    /// existe, ocupa sitio y no se puede restaurar. El `expect_err` es la mitad de la prueba: el
    /// día que el error se convierta en `Ok(0)`, esto cae.
    ///
    /// Y fija el otro lado, que es el que el motor consulta: con el productor caído sin declarar
    /// el final, la señal de la entrega se queda en falso.
    #[test]
    fn un_productor_que_muere_sin_declarar_el_final_da_error_y_no_un_fin_de_fichero() {
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
            avance: _,
        } = tunel_de_prueba("volcado-truncado.sql", 4);

        productor
            .enviar(b"parcial".to_vec())
            .expect("el canal tiene sitio de sobra");

        // El productor se cae sin llamar a `terminar`: es el caso de un `exec` que murió a medias
        // y el de un motor al que se le olvidó declarar el final.
        drop(productor);

        let mut lector = lector_de(&origen).expect("el lector tiene que abrirse");
        let mut bufer = [0_u8; 32];

        let primeros = lector
            .read(&mut bufer)
            .expect("los bytes que sí llegaron se entregan igual");
        assert_eq!(
            &bufer[..primeros],
            b"parcial",
            "el trozo que sí llegó no se entregó"
        );

        let error = lector.read(&mut bufer).expect_err(
            "un productor que muere sin declarar el final tiene que ser un error: devolver \
             `Ok(0)` aquí archiva un volcado truncado como si fuera válido",
        );

        let motivo = error.to_string();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::UnexpectedEof,
            "el fallo de un cierre sin final declarado tiene que clasificarse como un final \
             inesperado, no como otra cosa"
        );
        assert!(
            motivo.contains("final"),
            "el motivo no dice que faltó el final: {motivo}"
        );
        assert!(
            motivo.contains("volcado-truncado.sql"),
            "el motivo no nombra el volcado del que se cayó el productor: {motivo}"
        );

        // Y no se olvida: las lecturas siguientes siguen siendo un error.
        assert!(
            lector.read(&mut bufer).is_err(),
            "una lectura posterior volvió a darse por buena tras un cierre sin final"
        );

        assert!(
            !entrega.completa(),
            "un productor que se cayó sin declarar el final dejó la señal de la entrega puesta: \
             el motor daría por buena una copia que no llegó entera"
        );
    }

    /// **7.** El canal **se cierra** cuando muere el productor y nadie más tiene un emisor.
    ///
    /// Es la prueba que fija el dato 2 del rediseño, y por eso el plazo es de una hora: lo que se
    /// mide es que el lector decide por el **cierre del canal** y no por el plazo. Si esperara al
    /// plazo, la prueba tardaría una hora; si esperara a un emisor que ya no existe, no volvería
    /// nunca. La lectura va en su hilo y la espera de la prueba tiene su propio tope, así que un
    /// fallo se ve como un fallo y no como un cuelgue de la suite entera.
    #[test]
    fn el_canal_se_cierra_al_soltar_el_extremo_del_productor() {
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
            avance: _,
        } = tunel_con_plazo("volcado-sin-final.sql", 4, Duration::from_secs(3600));

        // Nadie más tiene un emisor —el tipo no se puede clonar—, así que el canal queda cerrado.
        drop(productor);

        let mut lector = lector_de(&origen).expect("el lector tiene que abrirse");
        let (aviso, respuesta) = std::sync::mpsc::channel();

        let hilo = std::thread::spawn(move || {
            let mut bufer = [0_u8; 32];
            let devuelto = lector.read(&mut bufer);

            aviso
                .send(devuelto)
                .expect("la prueba sigue esperando la respuesta de la lectura");
        });

        let devuelto = respuesta.recv_timeout(Duration::from_secs(10)).expect(
            "la lectura no volvió: el canal cerrado no la despertó, así que esperaba al plazo de \
             una hora o a un emisor que ya no existe",
        );
        hilo.join().expect("el hilo de la lectura no puede caerse");

        let error = devuelto.expect_err(
            "un canal cerrado sin declarar el final es un volcado truncado: la lectura tiene que \
             fallar, y nunca devolver 0",
        );

        assert_eq!(
            error.kind(),
            std::io::ErrorKind::UnexpectedEof,
            "el desenlace de un canal cerrado sin final tiene que ser el del volcado truncado, no \
             el del estancamiento: {error}"
        );
        assert!(
            !entrega.completa(),
            "un canal que se cerró sin declarar el final dejó la entrega marcada como completa"
        );
    }

    /// **8.** Un productor que se calla más que el plazo da el error de **estancamiento**.
    ///
    /// Es el otro desenlace que la primera versión del puente no tenía: el productor está **vivo**
    /// —su extremo existe y el canal no se cierra— pero no entrega nada. Sin plazo, esa lectura no
    /// volvería nunca y `archive()` se quedaría colgado con el worker de su repositorio detrás; con
    /// plazo, falla nombrando el volcado y el plazo.
    ///
    /// La prueba **no puede colgarse**: la lectura va en su hilo y esta espera tiene su tope, muy
    /// por encima del plazo del túnel, así que un fallo se ve como un fallo.
    #[test]
    fn un_productor_callado_mas_que_el_plazo_da_error_de_estancamiento() {
        let plazo = Duration::from_millis(200);
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
            avance: _,
        } = tunel_con_plazo("volcado-atascado.sql", 4, plazo);

        let mut lector = lector_de(&origen).expect("el lector tiene que abrirse");
        let (aviso, respuesta) = std::sync::mpsc::channel();

        let hilo = std::thread::spawn(move || {
            let mut bufer = [0_u8; 32];
            let devuelto = lector.read(&mut bufer);

            aviso
                .send(devuelto)
                .expect("la prueba sigue esperando la respuesta de la lectura");
        });

        let devuelto = respuesta.recv_timeout(Duration::from_secs(30)).expect(
            "la lectura no volvió por el plazo: se quedó esperando a un productor callado con el \
             extremo de envío vivo, que es el cuelgue que este plazo existe para cortar",
        );
        hilo.join().expect("el hilo de la lectura no puede caerse");

        let error = devuelto.expect_err(
            "un productor callado más que el plazo tiene que dar error, y nunca un fin de fichero",
        );

        assert_eq!(
            error.kind(),
            std::io::ErrorKind::TimedOut,
            "el desenlace de una espera agotada tiene que clasificarse como un tiempo agotado, no \
             como otra cosa: {error}"
        );

        let motivo = error.to_string();
        assert!(
            motivo.contains("volcado-atascado.sql"),
            "el motivo del estancamiento no nombra el volcado: {motivo}"
        );
        assert!(
            motivo.contains(&format!("{plazo:?}")),
            "el motivo del estancamiento no nombra el plazo que se agotó ({plazo:?}): {motivo}"
        );
        assert!(
            !entrega.completa(),
            "una lectura estancada dejó la entrega marcada como completa"
        );

        // El productor vive hasta aquí a propósito: es lo que la prueba fija —nunca se declaró el
        // final y nadie cerró el canal— y soltarlo ahora es lo que lo cierra.
        drop(productor);
    }

    /// **9.** `entries()` dos veces da un **error** y no un iterador vacío.
    ///
    /// El receptor se entrega con `.take()`, así que la segunda llamada no tiene de dónde leer.
    /// Lo que hay que fijar aquí es que devuelve un error y **a quién** protege: al llamador, que
    /// es quien se equivocó —un origen es de un solo uso—. No protege al archivado: eso está
    /// medido en la biblioteca, que filtra este error con un aviso y sigue. Aun así, un iterador
    /// vacío sería peor que un error: un motor que pide las entradas dos veces tiene que
    /// enterarse, y un error es lo único que se lo puede decir.
    #[test]
    fn los_entries_no_se_pueden_pedir_dos_veces() {
        let TunelDelVolcado {
            origen, productor, ..
        } = tunel_de_prueba("volcado-de-prueba.sql", 4);

        let primera: Vec<_> = origen.entries().collect();
        assert_eq!(
            primera.len(),
            1,
            "la primera llamada tiene que dar una entrada"
        );
        assert!(
            primera[0].is_ok(),
            "la primera entrada no puede ser un error"
        );

        let segunda: Vec<_> = origen.entries().collect();

        assert!(
            !segunda.is_empty(),
            "la segunda llamada dio un iterador vacío: el motor que pidió las entradas dos veces \
             no tendría forma de enterarse de su error"
        );

        let error = segunda[0]
            .as_ref()
            .err()
            .expect("la segunda llamada tiene que dar un error, no una entrada");

        assert!(
            error.to_string().contains("volcado-de-prueba.sql"),
            "el motivo del segundo uso no nombra el volcado: {error}"
        );

        drop(productor);
    }

    /// **10.** `entries()` da **exactamente una** entrada, con su camino y su nodo de fichero.
    ///
    /// Un volcado es un solo fichero lógico: si el puente declarara un directorio o ninguna
    /// entrada, la biblioteca no tendría qué leer.
    #[test]
    fn los_entries_dan_una_entrada_con_su_nombre_y_su_nodo_de_fichero() {
        let TunelDelVolcado {
            origen, productor, ..
        } = tunel_de_prueba("volcado-de-tienda.sql", 4);

        let mut entradas = origen.entries();
        let entrada = entradas
            .next()
            .expect("el origen tiene que dar una entrada")
            .expect("la entrada no puede ser un error");

        assert!(
            entradas.next().is_none(),
            "un volcado es una sola entrada, y salieron más"
        );
        assert_eq!(
            entrada.path,
            PathBuf::from("volcado-de-tienda.sql"),
            "el camino de la entrada no es el nombre con el que se pidió el volcado"
        );
        assert_eq!(
            entrada.node.node_type,
            NodeType::File,
            "el nodo del volcado tiene que ser un fichero: lo que entra por aquí son bytes"
        );
        assert!(
            entrada.open.is_some(),
            "la entrada no trae cómo abrirse, así que la biblioteca no puede leer nada"
        );

        drop(productor);
    }

    /// **11.** La entrega **no** está completa mientras el volcado no haya terminado.
    ///
    /// Es la mitad que hace útil la señal: si valiera `true` desde el principio, el motor no
    /// tendría ninguna —una señal que siempre dice «sí» es un adorno—. Se comprueba en los dos
    /// momentos en los que el archivado puede estar antes del final: antes de leer nada y con
    /// bytes ya entregados pero el volcado todavía abierto. Y se comprueba por el **acceso** del
    /// tipo, que es lo que el motor va a usar.
    #[test]
    fn la_entrega_no_esta_completa_mientras_el_volcado_va_a_medias() {
        let tunel = tunel_de_prueba("volcado-a-medias.sql", 4);

        assert!(
            !tunel.entrega.completa(),
            "un túnel recién hecho ya venía con la entrega completa"
        );

        let primer_trozo = b"CREATE TABLE tienda (id INT);\n".to_vec();
        tunel
            .productor
            .enviar(primer_trozo.clone())
            .expect("el canal tiene sitio de sobra");

        let mut lector = lector_de(&tunel.origen).expect("el lector tiene que abrirse");
        let mut bufer = [0_u8; 128];
        let entregados = lector
            .read(&mut bufer)
            .expect("los bytes que sí llegaron se entregan igual");

        assert_eq!(
            &bufer[..entregados],
            primer_trozo.as_slice(),
            "el primer trozo del volcado no se entregó"
        );
        assert!(
            !tunel.entrega.completa(),
            "el volcado estaba a medias y la señal de la entrega ya estaba puesta: el motor daría \
             por buena una copia que no llegó entera"
        );
    }

    /// **12.** El extremo del productor **no se puede clonar**.
    ///
    /// Es la prueba de una propiedad que no se mide en tiempo de ejecución sino en el tipo: el
    /// canal se cierra cuando muere el **último** emisor, así que dos extremos vivos dejarían al
    /// lector esperando a un productor que ya no está —el cuelgue que el plazo tapa pero que aquí
    /// se cierra por construcción—. Añadir `#[derive(Clone)]` a [`ProductorDelVolcado`] no
    /// rompería ninguna prueba de comportamiento: rompería esta.
    ///
    /// El sondeo va en las dos direcciones a propósito: el control —un tipo que sí es `Clone`—
    /// existe para que la prueba no pueda pasar sin comprobar nada.
    #[test]
    fn el_extremo_del_productor_no_se_puede_clonar() {
        assert!(
            SondaDeClon::<String>::nueva().es_clonable(),
            "el sondeo no reconoce un tipo que sí es `Clone`, así que esta prueba estaría pasando \
             sin comprobar nada"
        );
        assert!(
            !SondaDeClon::<ProductorDelVolcado>::nueva().es_clonable(),
            "`ProductorDelVolcado` se puede clonar: con dos extremos vivos el canal no se cierra \
             cuando el productor termina, y el lector espera al productor que ya no está"
        );
    }

    /// Sondea, en tiempo de compilación, si un tipo implementa [`Clone`].
    ///
    /// El truco es un método **inherente** que solo existe si `T: Clone`, contra el mismo método
    /// declarado en un trait con implementación general para el sondeo: la resolución prefiere el
    /// inherente cuando se puede aplicar, y cae al del trait cuando no. Es lo que permite
    /// escribir, en una prueba normal y sin dependencias nuevas, algo que normalmente necesita una
    /// comprobación de compilación —y que se cae en cuanto alguien le ponga `Clone` al tipo
    /// sondeado—.
    struct SondaDeClon<T>(PhantomData<T>);

    impl<T> SondaDeClon<T> {
        /// La sonda, sin valor de tipo dentro: solo lleva el tipo en el nombre.
        const fn nueva() -> Self {
            SondaDeClon(PhantomData)
        }
    }

    /// La respuesta por defecto del sondeo: no es `Clone`.
    trait EsClonable {
        /// `true` solo si el tipo sondeado implementa [`Clone`].
        fn es_clonable(&self) -> bool {
            false
        }
    }

    impl<T> EsClonable for SondaDeClon<T> {}

    impl<T: Clone> SondaDeClon<T> {
        /// La respuesta cuando el tipo **sí** implementa [`Clone`].
        fn es_clonable(&self) -> bool {
            true
        }
    }

    /// **13.** Una lectura con el búfer **vacío** devuelve `0`, no espera y **no** consume nada.
    ///
    /// Es el otro `Ok(0)` que no significa «el volcado se acabó»: el contrato de [`Read`] manda
    /// contestarlo con un búfer de longitud cero, y contestarlo **en el acto**. Se miden las tres
    /// cosas. Primero la inmediatez: el plazo del túnel es de un minuto, así que una
    /// implementación que se pusiera a esperar ahí se vería —el tope de la prueba son cinco
    /// segundos— en vez de colgar la suite. Después que no se marca la entrega. Y por último que
    /// no se consume nada: la lectura siguiente, con un búfer de verdad, tiene que recibir el
    /// **primer** byte —si la lectura vacía se hubiera llevado por delante el mensaje, el volcado
    /// saldría corto por delante y se archivarían los bytes que quedaran como si fuera entero—.
    #[test]
    fn una_lectura_con_bufer_vacio_no_espera_ni_consume_nada() {
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
            avance: _,
        } = tunel_con_plazo("volcado-sin-bufer.sql", 4, Duration::from_secs(60));

        let lector = lector_de(&origen).expect("el lector tiene que abrirse");

        // Nadie ha mandado nada todavía: el canal está abierto y vacío.
        let (aviso, respuesta) = std::sync::mpsc::channel();
        let hilo_vacio = std::thread::spawn(move || {
            let mut lector = lector;
            let mut nada: [u8; 0] = [];
            let devuelto = lector.read(&mut nada);

            aviso
                .send(devuelto)
                .expect("la prueba sigue esperando la respuesta de la lectura vacía");

            // El lector vuelve por la unión: la prueba tiene que seguir leyendo de él.
            lector
        });

        let devuelto = respuesta.recv_timeout(Duration::from_secs(5)).expect(
            "la lectura con el búfer vacío se quedó esperando un mensaje que nadie había \
                 mandado: el contrato de `Read` obliga a contestar `0` en el acto",
        );

        assert_eq!(
            devuelto.expect("un búfer vacío no puede fallar"),
            0,
            "el contrato de `Read` obliga a contestar `0` con un búfer vacío"
        );
        assert!(
            !entrega.completa(),
            "la lectura con el búfer vacío se tomó por un final de fichero del volcado"
        );

        let mut lector = hilo_vacio
            .join()
            .expect("el hilo de la lectura vacía no puede caerse");

        // Ya con el volcado en marcha: el productor, en su hilo.
        let esperado = b"CREATE TABLE tienda (id INT);\n".to_vec();
        let a_enviar = esperado.clone();

        let emisor = std::thread::spawn(move || {
            productor
                .enviar(a_enviar)
                .expect("el canal no puede estar cerrado antes del final");
            productor.terminar();
        });

        // La lectura con un búfer de verdad, que tiene que recibir el **primer** byte.
        let mut bufer = [0_u8; 5];
        let entregados = lector
            .read(&mut bufer)
            .expect("con el volcado en marcha, leer no puede fallar");

        assert_eq!(
            &bufer[..entregados],
            &esperado[..entregados],
            "la primera lectura de verdad no recibió el principio del volcado: la lectura vacía \
             se lo había comido"
        );

        let mut resto = Vec::new();
        lector
            .read_to_end(&mut resto)
            .expect("con el final declarado, leer no puede fallar");
        emisor.join().expect("el hilo productor no puede caerse");

        assert_eq!(
            [bufer[..entregados].as_ref(), resto.as_slice()].concat(),
            esperado,
            "entre la lectura vacía y las siguientes no salió el volcado entero"
        );
        assert!(
            entrega.completa(),
            "el volcado se entregó entero y la señal sigue en falso"
        );
    }

    /// **14.** Un nombre de volcado **vacío** se rechaza antes de llegar a la biblioteca.
    ///
    /// No es cosmético: con el nombre vacío la biblioteca entra en **pánico** —su camino hacia el
    /// árbol hace `expect("file path should have a parent!")`, y un camino vacío no tiene padre—,
    /// así que rechazarlo aquí es la diferencia entre un error accionable y un hilo de archivado
    /// muerto a mitad de una copia.
    #[test]
    fn un_nombre_de_volcado_vacio_se_rechaza() {
        assert_eq!(
            nombre_rechazado(tunel_del_volcado("", 4, PLAZO_DE_PRUEBA)),
            "",
            "el error no señala el nombre vacío como el que rechazó"
        );
    }

    /// **15.** Un nombre de volcado que sea `.` se rechaza.
    ///
    /// `.` nombra al directorio en el que se está, no a un fichero: la entrada quedaría con un
    /// nombre que no nombra nada y el snapshot guardaría un volcado que nadie podría identificar.
    #[test]
    fn un_nombre_de_volcado_de_punto_se_rechaza() {
        assert_eq!(
            nombre_rechazado(tunel_del_volcado(".", 4, PLAZO_DE_PRUEBA)),
            ".",
            "el error no señala el punto como el nombre que rechazó"
        );
    }

    /// **16.** Un nombre de volcado que sea `..` se rechaza, por lo mismo que el punto: no nombra
    /// un fichero, nombra un salto de directorio.
    #[test]
    fn un_nombre_de_volcado_de_punto_punto_se_rechaza() {
        assert_eq!(
            nombre_rechazado(tunel_del_volcado("..", 4, PLAZO_DE_PRUEBA)),
            "..",
            "el error no señala los dos puntos como el nombre que rechazó"
        );
    }

    /// **17.** Un nombre con **separador** se rechaza: con él dentro el nombre deja de ser un
    /// nombre y pasa a ser un camino, con la entrada partida en trozos y un nodo que lleva un
    /// separador dentro de su propio nombre.
    #[test]
    fn un_nombre_de_volcado_con_barra_se_rechaza() {
        assert_eq!(
            nombre_rechazado(tunel_del_volcado("volcados/tienda.sql", 4, PLAZO_DE_PRUEBA)),
            "volcados/tienda.sql",
            "el error no señala el nombre con barra como el que rechazó"
        );
    }

    /// **18.** Y con el otro separador también: en otras plataformas el camino se parte igual, y
    /// aquí el nombre es el mismo en todas, así que se rechazan los dos.
    #[test]
    fn un_nombre_de_volcado_con_barra_invertida_se_rechaza() {
        assert_eq!(
            nombre_rechazado(tunel_del_volcado(
                "volcados\\tienda.sql",
                4,
                PLAZO_DE_PRUEBA
            )),
            "volcados\\tienda.sql",
            "el error no señala el nombre con barra invertida como el que rechazó"
        );
    }

    /// **19.** Un nombre normal **sí** pasa, y el túnel que sale de él vale.
    ///
    /// Es el otro lado de la validación: una puerta que lo rechazara todo sería tan inútil como
    /// una que no rechazara nada. Un nombre de verdad —con su punto, sus guiones y su extensión—
    /// no solo se acepta: llega al `path` de la entrada, que es lo que fija la prueba 10.
    #[test]
    fn un_nombre_de_volcado_normal_si_pasa() {
        let tunel = tunel_de_prueba("volcado-de-tienda-2026-10-25.sql", 4);

        assert!(
            !tunel.entrega.completa(),
            "un túnel recién construido no puede traer la entrega completa"
        );
        assert_eq!(
            tunel.origen.nombre, "volcado-de-tienda-2026-10-25.sql",
            "el nombre aceptado no es el que el origen va a usar como camino de la entrada"
        );
    }

    // ── El avance del volcado ────────────────────────────────────────────────
    //
    // Es la cuenta que el motor publica como `bytes_processed` **en vivo**, así que lo que
    // estas cuatro pruebas fijan es lo que el motor va a leer: dónde empieza, cuándo crece,
    // qué no cuenta y qué publica de sí misma.

    /// **20.** La cuenta empieza en cero y crece **exactamente** con lo entregado.
    ///
    /// La cuenta es lo que el panel pinta como bytes procesados, así que un contador que
    /// arrancara en otra cosa o que sumara de más sería una cifra falsa con la misma
    /// autoridad que una de verdad. Se mide contra el total enviado, y con trozos de tamaño
    /// distinto para que una implementación que sumara «un trozo» en vez de sus bytes no
    /// cuadre por casualidad.
    #[test]
    fn el_avance_empieza_en_cero_y_crece_con_lo_entregado() {
        let TunelDelVolcado {
            origen: _origen,
            productor,
            entrega: _entrega,
            avance,
        } = tunel_de_prueba("volcado-contado.sql", 8);

        assert_eq!(
            avance.bytes(),
            0,
            "un túnel recién construido no puede traer bytes contados"
        );

        let trozos = [vec![b'a'; 97], vec![b'b'; 1024], vec![b'c'; 3]];
        let total: u64 = trozos.iter().map(|trozo| trozo.len() as u64).sum();

        for trozo in trozos {
            productor
                .enviar(trozo)
                .expect("el canal tiene sitio de sobra, así que el envío no puede fallar");
        }

        assert_eq!(
            avance.bytes(),
            total,
            "la cuenta no es la suma de lo entregado: {} de {total}",
            avance.bytes()
        );

        drop(productor);
    }

    /// **21.** Un trozo **vacío** no cuenta y tampoco viaja.
    ///
    /// Las dos mitades importan: si contara, la cuenta subiría sin que se hubiera entregado
    /// nada, y si viajara, el lector tendría un mensaje que no significa nada que desambiguar
    /// —el final del volcado no viaja por el canal—. Se comprueba por el camino del lector:
    /// lo primero que recibe tiene que ser el trozo de verdad, no un hueco.
    #[test]
    fn un_trozo_vacio_no_cuenta_ni_viaja_por_el_canal() {
        let TunelDelVolcado {
            origen,
            productor,
            entrega: _entrega,
            avance,
        } = tunel_de_prueba("volcado-sin-trozo-vacio.sql", 4);

        productor
            .enviar(Vec::new())
            .expect("un trozo vacío no puede fallar: ni siquiera se envía");

        assert_eq!(
            avance.bytes(),
            0,
            "un trozo vacío sumó a la cuenta, que es contar lo que no se entregó"
        );

        let esperado = b"CREATE TABLE tienda (id INT);\n".to_vec();
        let a_enviar = esperado.clone();

        let emisor = std::thread::spawn(move || {
            productor
                .enviar(a_enviar)
                .expect("el canal no puede estar cerrado antes del final");
            productor.terminar();
        });

        let mut lector = lector_de(&origen).expect("el lector tiene que abrirse");
        let mut leido = Vec::new();
        lector
            .read_to_end(&mut leido)
            .expect("con el final declarado, leer no puede fallar");

        emisor.join().expect("el hilo productor no puede caerse");

        assert_eq!(
            leido, esperado,
            "lo que llegó al lector no es el trozo de verdad: el vacío viajó por el canal"
        );
        assert_eq!(
            avance.bytes(),
            esperado.len() as u64,
            "la cuenta no cuadra con lo que sí se entregó"
        );
    }

    /// **22.** La cuenta se puede leer **mientras** el productor empuja, desde otro hilo, y
    /// **no retrocede**.
    ///
    /// Es la propiedad que hace útil el contador para el panel: si solo se pudiera consultar al
    /// final, el motor no tendría nada que publicar en vivo y `bytes_processed` llegaría siempre
    /// tarde. Aquí el productor va en su hilo —donde va de verdad— y esta prueba lee el contador
    /// en un bucle, mirando todas las muestras en orden: un contador que bajara sería un panel
    /// que va hacia atrás.
    #[test]
    fn el_avance_se_puede_leer_mientras_el_productor_empuja_y_no_retrocede() {
        /// Trozos que empuja el productor.
        const TROZOS: usize = 8;
        /// Bytes de cada trozo.
        const BYTES: usize = 100;

        let TunelDelVolcado {
            origen: _origen,
            productor,
            entrega: _entrega,
            avance,
        } = tunel_de_prueba("volcado-leido-en-vivo.sql", TROZOS + 1);

        let total = (TROZOS * BYTES) as u64;

        let emisor = std::thread::spawn(move || {
            for _ in 0..TROZOS {
                productor
                    .enviar(vec![b'x'; BYTES])
                    .expect("el canal tiene sitio de sobra, así que el envío no puede fallar");
            }
            productor.terminar();
        });

        let limite = Instant::now() + Duration::from_secs(10);
        let mut anterior = 0;
        let mut muestras = 0;

        while avance.bytes() < total {
            let ahora = avance.bytes();

            assert!(
                ahora >= anterior,
                "la cuenta retrocedió, y un avance que baja es un panel que va hacia atrás: \
                 {anterior} -> {ahora}"
            );
            anterior = ahora;
            muestras += 1;

            assert!(
                Instant::now() < limite,
                "la cuenta no llegó al total en 10 segundos: {ahora} de {total}"
            );

            std::thread::yield_now();
        }

        emisor.join().expect("el hilo productor no puede caerse");

        assert_eq!(
            avance.bytes(),
            total,
            "la cuenta final no es la de lo entregado"
        );
        assert!(
            muestras > 0,
            "el bucle no llegó a leer la cuenta: la prueba no midió nada"
        );
        assert!(
            anterior <= total,
            "alguna muestra pasó del total: {anterior} de {total}"
        );
    }

    /// **23.** El `Debug` de la cuenta publica la cuenta y **nada** del volcado.
    ///
    /// Lo que hay dentro es un contador compartido, y lo único que alguien necesita saber de él
    /// en un diagnóstico es cuánto lleva contado. Se escribe a mano y no se deriva por eso
    /// mismo: un `Debug` derivado imprimiría el `Arc` y el interior del `AtomicU64`, y en un
    /// tipo que existe para acompañar a un volcado eso es un sitio más por el que el contenido
    /// puede salir.
    #[test]
    fn el_debug_del_avance_publica_la_cuenta_y_nada_del_volcado() {
        let distinguible = "contenido-distinguible-del-volcado-0002";
        let tunel = tunel_de_prueba("volcado-sin-publicar.sql", 4);

        tunel
            .productor
            .enviar(distinguible.as_bytes().to_vec())
            .expect("el canal tiene sitio de sobra");

        let en_debug = format!("{:?}", tunel.avance);

        assert!(
            en_debug.contains("AvanceDelVolcado"),
            "el Debug no dice ni de qué tipo es: {en_debug}"
        );
        assert!(
            en_debug.contains(&tunel.avance.bytes().to_string()),
            "el Debug no publica la cuenta, que es lo único que tiene que publicar: {en_debug}"
        );
        assert!(
            !en_debug.contains(distinguible),
            "el Debug de la cuenta publicó el contenido del volcado: {en_debug}"
        );
        assert!(
            !en_debug.contains("volcado-sin-publicar.sql"),
            "el Debug de la cuenta publicó el nombre del volcado: {en_debug}"
        );

        drop(tunel.productor);
    }
}
