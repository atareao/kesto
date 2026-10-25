//! El puente entre el `stream` de la extracción y el origen de lectura de la biblioteca.
//!
//! La extracción entrega bytes **de forma asíncrona y a trozos** —el `stream` de un `exec`
//! de la API de Docker— y [`rustic_core::ReadSource`] pide un lector **bloqueante** que
//! además tiene que ser `Sync + Send`. Entre las dos cosas hay un hilo, un canal **acotado** y
//! un lector, y cada una de esas tres piezas tapa un fallo concreto:
//!
//! 1. **El canal es `sync_channel` y acotado.** Sin tope, un productor rápido contra un
//!    consumidor lento acumula **el volcado entero** en memoria, y la memoria retenida pasa a
//!    ser una función del tamaño de la base. Con el tope, el productor espera. La capacidad la
//!    elige el motor: este módulo no sabe cuánta memoria hay.
//! 2. **`Fin` existe para poder distinguir un final bueno de un productor que se cayó.** Sin
//!    él, el cierre del canal es ambiguo: el productor que termina bien y el que muere por un
//!    fallo del `exec` dejan los dos el canal cerrado, y el segundo caso significaría
//!    **archivar un volcado truncado como si fuera válido** —una copia que existe, ocupa sitio
//!    y no se puede restaurar—. Por eso el productor manda [`MensajeDelVolcado::Fin`] cuando
//!    acaba bien, y el lector traduce un canal cerrado sin `Fin` a un [`std::io::Error`],
//!    **NUNCA** a un fin de fichero: devolver `Ok(0)` ahí es el peor fallo posible de este
//!    módulo, porque la biblioteca lo leería como «el volcado se acabó» y archivaría lo que
//!    llegó.
//! 3. **`entries()` solo se puede llamar una vez, y el error del segundo intento protege al
//!    llamador.** El receptor se saca del [`Mutex`] con `.take()` —el trait exige `Sync` y un
//!    [`Receiver`] no lo es, así que el candado no se puede evitar: es el mismo truco que usa el
//!    `ChildStdoutSource` de la propia biblioteca—, y una segunda llamada devuelve un iterador
//!    con un **error** dentro, nunca uno vacío. Lo que ese error **no** es, es una barrera
//!    contra el archivado: es lo que dice el punto siguiente. Un origen es de un solo uso, y
//!    cada intento de copia construye su propio túnel con [`tunel_del_volcado`].
//! 4. **Ni el error del lector ni el del segundo `entries()` detienen la copia: la biblioteca
//!    los descarta con un aviso.** Está medido leyendo el `archiver.rs` de la biblioteca de
//!    copia: los dos entran por un `filter_map` que, en su rama de error, hace
//!    `warn!("ignoring error: …")` y sigue, así que `archive()` devuelve `Ok` y **guarda el
//!    snapshot igualmente** —sin el volcado, porque la entrada que falló no llega al árbol—. La
//!    consecuencia es la que gobierna este módulo: de un `Ok` de `archive()` **no** se deduce
//!    que el volcado esté completo, y la única verdad fiable está del lado del productor. Por
//!    eso el túnel lleva [`TunelDelVolcado::fin_consumido()`], que es lo que el motor tiene que
//!    consultar por su cuenta antes de dar una copia por buena. Ver el comentario del `read`
//!    del [`LectorDeVolcado`]: es el aviso que más importa de todo el fichero.

use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use rustic_core::repofile::{Metadata, Node, NodeType};
use rustic_core::{ErrorKind, ReadSource, ReadSourceEntry, RusticError, RusticResult};

/// Error del trato con la biblioteca de copia.
///
/// Es el tipo que la segunda mitad de este módulo hará crecer: aquí solo está lo que el puente
/// puede rechazar **antes** de llegar a la biblioteca, que es la única clase de fallo que este
/// módulo puede convertir en un error en vez de en un pánico.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// El nombre del volcado no sirve como nombre de fichero.
    #[error("el nombre del volcado no puede estar vacío ni llevar separadores: {nombre:?}")]
    NombreDeVolcadoInvalido { nombre: String },
}

/// Las dos mitades de un origen de volcado, y la señal de si el volcado llegó entero.
///
/// Se construyen juntas con [`tunel_del_volcado`] y no se pueden construir por separado: un
/// origen sin su productor es un origen que nunca terminará, y un productor sin su origen es
/// un productor que se bloquea para siempre en cuanto el canal se llena.
///
/// # El productor va en **otro hilo**, y esto no es un consejo
///
/// El archivado lee del canal **en el hilo que llama a `archive()`**. Si el productor empujara
/// desde ese mismo hilo, se bloquearía en `send` esperando hueco en un canal acotado mientras
/// el consumidor espera al productor: **bloqueo mutuo permanente**. El desenlace es el que este
/// módulo existe para evitar, y peor, porque no hay error ninguno: la ejecución se queda en
/// `running` y el worker de su repositorio queda ocupado para siempre. El orden correcto es
/// siempre el mismo:
///
/// 1. el productor, en su hilo —el que drena el `exec` de la extracción—;
/// 2. el archivado, en el hilo que llama a `archive()`, leyendo de
///    [`TunelDelVolcado::origen`];
/// 3. la unión de los dos al final —`join()` del hilo del productor— y, **después**,
///    [`TunelDelVolcado::fin_consumido()`].
///
/// # `capacidad` está en trozos, no en bytes
///
/// El canal retiene hasta `capacidad` **mensajes**, así que la memoria retenida es
/// `capacidad × el tamaño de un trozo`, no una cantidad de bytes: quien elija el número tiene
/// que multiplicar. Por eso el tope entra por parámetro en vez de estar aquí dentro, y por eso
/// el defecto no existe: elegirlo es decidir cuánta memoria puede gastar el motor.
/// `capacidad == 0` es un **canal de encuentro**: el `send` del productor no vuelve hasta que
/// haya una lectura en curso, así que no retiene memoria ninguna y frena al productor a la
/// velocidad exacta del archivado.
///
/// # Si el archivado se rinde, el `send` del productor falla
///
/// Cuando el archivado termina antes de tiempo —se rinde, o ya no lee más— el lector se suelta,
/// el canal se desconecta y el `send` del productor devuelve un **error**. No es un fallo suyo
/// ni una razón para entrar en pánico: significa «el archivado se rindió», y lo que toca es
/// terminar el bucle y morir. Un productor que entrara en pánico ahí dejaría su hilo envenenado
/// y la unión del punto 3 se colgaría, con lo que el bloqueo que se quería evitar volvería por
/// la puerta de atrás.
pub struct TunelDelVolcado {
    /// La mitad que se le entrega a la biblioteca de copia.
    pub origen: OrigenDelVolcado,
    /// La mitad por la que el productor empuja los bytes.
    pub productor: SyncSender<MensajeDelVolcado>,
    /// Se pone a `true` cuando el lector ha **consumido** el mensaje de final, es decir
    /// cuando el volcado se entregó entero.
    ///
    /// Existe porque no hay otra forma de saberlo: el lector vive dentro de un
    /// [`ReadSourceEntry`] que la biblioteca se queda, así que el motor nunca lo vería, y el
    /// error del lector **no** sirve de señal —la biblioteca lo descarta con un aviso y
    /// `archive()` devuelve `Ok` igualmente, ver el encabezado del módulo—. Esta marca es lo
    /// que permite que el motor **no** dé por buena una copia cuyo archivado terminó en `Ok`
    /// pese a que el volcado murió a mitad. La marca la pone el lector —que recibe su copia del
    /// origen— y la consulta el motor por este mismo campo: es el mismo `Arc` en las dos puntas,
    /// así que leerla **después** de que el archivado haya terminado es lo único que hace falta.
    pub fin_consumido: Arc<AtomicBool>,
}

impl TunelDelVolcado {
    /// ¿Se entregó el volcado entero? Falso mientras el lector no haya consumido el final.
    ///
    /// Mira el [`field@TunelDelVolcado::fin_consumido`] que el lector marca al recibir
    /// [`MensajeDelVolcado::Fin`]. Un `Ok` de `archive()` **no** implica que esto sea `true`:
    /// son dos preguntas distintas, y esta es la única que habla del volcado.
    pub fn fin_consumido(&self) -> bool {
        // `Acquire` y no `Relaxed`: la marca la escribe el hilo del archivado y la lee el del
        // motor. Nada viaja con ella —no hay datos que publicar—, así que `Relaxed` bastaría
        // por lo que hace el hardware; el par `Release`/`Acquire` se usa porque el coste es
        // nulo y deja el traspaso entre hilos sin depender de ese razonamiento.
        self.fin_consumido.load(Ordering::Acquire)
    }
}

/// Lo que viaja por el túnel.
///
/// `Fin` no es un adorno del protocolo: sin él, el cierre del canal es ambiguo y el lector no
/// puede distinguir «el volcado se acabó» de «el productor se cayó». Ver el encabezado del
/// módulo.
pub enum MensajeDelVolcado {
    /// Un trozo de datos.
    Datos(Vec<u8>),
    /// El productor terminó **bien** y no queda nada más.
    Fin,
}

impl std::fmt::Debug for MensajeDelVolcado {
    /// Publica la **forma** del mensaje, nunca sus bytes.
    ///
    /// No es [`derive`](std::fmt::Debug) a propósito: un `Datos` es la **copia del volcado**, o
    /// sea las filas de las tablas con sus datos personales, así que un `Debug` derivado
    /// convertiría un `tracing::debug!(?mensaje)` de una línea en el volcado escrito en el
    /// registro del contenedor. Lo que hay que saber para diagnosticar es cuánto lleva, y eso es
    /// lo único que sale por aquí.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Datos(datos) => write!(f, "Datos({} bytes)", datos.len()),
            Self::Fin => f.write_str("Fin"),
        }
    }
}

/// Construye las dos mitades de un túnel de volcado.
///
/// `nombre` es la identidad del volcado —no existe en ningún sistema de ficheros, así que su
/// único nombre es el que le da quien lo pide—, y `capacidad` es el número de trozos que
/// puede retener el canal antes de que el productor tenga que esperar. La capacidad entra por
/// parámetro **a propósito**: la elige el motor, que es quien conoce el presupuesto de memoria.
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
/// # El productor tiene que correr en otro hilo
///
/// Con el canal acotado, un productor que corra en el mismo hilo que llama a `archive()` se
/// bloquea en `send` esperando hueco mientras el consumidor espera al productor, y el bloqueo
/// es mutuo y permanente. El orden es: productor en su hilo, archivado en el suyo, unión al
/// final. Y `capacidad` está en **trozos**, no en bytes: la memoria retenida es
/// `capacidad × el tamaño de un trozo`, y `capacidad == 0` es un canal de encuentro. Está
/// contado con más detalle en el rustdoc de [`TunelDelVolcado`], que es el que hay que leer
/// antes de escribir al otro lado.
///
/// Del mismo sitio sale la otra trampa del productor: cuando el archivado se rinde, el `send`
/// devuelve un **error**, y ese error significa «el archivado se rindió» —el bucle tiene que
/// terminar—, no un fallo del productor. Entrar en pánico ahí deja el hilo envenenado y la
/// unión se cuelga.
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
) -> Result<TunelDelVolcado, Error> {
    let nombre = nombre.into();
    validar_nombre_de_volcado(&nombre)?;

    // `sync_channel` y no `channel`: es la diferencia entre un canal que acota y uno que no.
    let (productor, receptor) = sync_channel(capacidad);

    // La marca del final: la pone el lector cuando consume el `Fin` y la consulta el motor
    // cuando el archivado ya terminó, así que las dos puntas del `Arc` nacen aquí y a partir de
    // este momento van por caminos distintos —el origen se le entrega a la biblioteca, el túnel
    // se queda en el motor—. Es la única línea que las une.
    let fin_consumido = Arc::new(AtomicBool::new(false));

    Ok(TunelDelVolcado {
        origen: OrigenDelVolcado {
            nombre,
            receptor: Mutex::new(Some(receptor)),
            fin_consumido: Arc::clone(&fin_consumido),
        },
        productor,
        fin_consumido,
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
/// otros hilos es el [`Mutex`] con el receptor dentro y la marca del final, que es atómica.
pub struct OrigenDelVolcado {
    /// El nombre con el que el motor pidió el volcado: es su única identidad, porque no hay
    /// ningún fichero detrás. Ya viene validado por [`tunel_del_volcado`].
    nombre: String,
    /// El receptor, dentro de un candado y de un `Option`.
    ///
    /// El candado no es un adorno: [`ReadSource`] exige `Sync` y un [`Receiver`] no lo es. Y el
    /// `Option` está porque [`ReadSource::entries`] recibe `&self` mientras que el receptor hay
    /// que **sacarlo** para entregárselo al lector, así que la operación es un `.take()`.
    receptor: Mutex<Option<Receiver<MensajeDelVolcado>>>,
    /// La marca del final que comparte con el túnel, para entregársela al lector.
    ///
    /// Vive también aquí —y no solo en el [`TunelDelVolcado`]— porque el lector se construye en
    /// [`ReadSource::entries`], que es lo único que este lado puede hacer: sin esta copia del
    /// `Arc`, el lector no tendría forma de alcanzar la punta que el motor va a consultar.
    fin_consumido: Arc<AtomicBool>,
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
    receptor: Receiver<MensajeDelVolcado>,
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
    /// `true` cuando el productor mandó [`MensajeDelVolcado::Fin`].
    ///
    /// Es la marca que distingue un final bueno —`Ok(0)`— de un productor que se cayó —un
    /// [`std::io::Error`]—. Lo que decide es el mensaje, no el estado del canal. **No** basta
    /// para saber si el volcado llegó entero desde fuera del módulo: para eso está
    /// [`LectorDeVolcado::fin_consumido`], que además cruza el hilo.
    terminado: bool,
    /// El mismo [`field@TunelDelVolcado::fin_consumido`] que tiene el túnel, para poder marcarlo.
    ///
    /// Vive aquí porque el lector es lo único que ve el final: el motor tiene la otra punta del
    /// `Arc` y así se entera de que el volcado se entregó entero sin poder leer nada del
    /// volcado. Ver el comentario de `Fin` en el `read` de [`Read`].
    fin_consumido: Arc<AtomicBool>,
}

impl Read for LectorDeVolcado {
    /// Entrega hasta `buf.len()` bytes del volcado, esperando al productor si hace falta.
    ///
    /// Dos desenlaces devuelven `Ok(0)`: haber recibido [`MensajeDelVolcado::Fin`], y el búfer
    /// vacío —que el contrato de [`Read`] exige contestar así aunque no se haya consumido nada, y
    /// que por eso no marca la señal del final—. Un canal cerrado sin `Fin` devuelve un error:
    /// **confundirlo con un fin de fichero archiva un volcado truncado como si fuera válido**.
    ///
    /// # Y ese error no detiene la copia
    ///
    /// El error es lo correcto por parte del lector, pero **la biblioteca de copia no lo
    /// propaga**: está medido en su `archiver.rs`, donde los errores que salen del archivado de
    /// ficheros entran por un `filter_map` que hace `warn!("ignoring error: …")` y sigue. O
    /// sea que este error **se descarta con un aviso** y `archive()` devuelve `Ok`, con el
    /// snapshot guardado y sin el volcado dentro —la entrada que falló no llega al árbol—. Quien
    /// archiva **no puede** deducir de un `Ok` de `archive()` que el volcado esté completo: la
    /// única verdad fiable está del lado del productor, y la forma de consultarla es
    /// [`TunelDelVolcado::fin_consumido()`], que este `read` marca justo en `Fin` y en ningún
    /// otro sitio. Sin esa comprobación, un volcado que murió a mitad y una copia buena son
    /// indistinguibles desde fuera.
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // El contrato de `Read` obliga a contestar `Ok(0)` con un búfer vacío, y además pedir un
        // mensaje que no cabe en ningún sitio sería retenerlo sin destino. Es el único `Ok(0)`
        // que no significa «el volcado se acabó», y también el único que **no** marca
        // `fin_consumido`: aquí no se ha consumido nada.
        if buf.is_empty() {
            return Ok(0);
        }

        loop {
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

            // 2. El único fin de fichero legítimo.
            if self.terminado {
                return Ok(0);
            }

            match self.receptor.recv() {
                Ok(MensajeDelVolcado::Datos(datos)) => {
                    // Un mensaje vacío no es un final: se salta y se sigue esperando. Devolver
                    // `Ok(0)` aquí diría «el volcado se acabó» sin que se haya acabado.
                    if datos.is_empty() {
                        continue;
                    }

                    let a_entregar = datos.len().min(buf.len());
                    buf[..a_entregar].copy_from_slice(&datos[..a_entregar]);

                    if a_entregar < datos.len() {
                        // Lo que no cupo no se tira: se guarda para la lectura siguiente.
                        self.sobrante = datos;
                        self.consumido = a_entregar;
                    }

                    return Ok(a_entregar);
                }
                Ok(MensajeDelVolcado::Fin) => {
                    self.terminado = true;
                    // Y se marca en el túnel, que es la única punta que el motor puede mirar:
                    // el lector se queda dentro de la biblioteca, así que su `terminado` es
                    // privado y no llega a ninguna parte. Es aquí, y solo aquí, donde la marca
                    // puede ponerse: este es el único sitio del módulo que ve un final bueno.
                    // `Release` por el mismo motivo que el `Acquire` del consultor.
                    self.fin_consumido.store(true, Ordering::Release);
                    return Ok(0);
                }
                // El canal se cerró sin `Fin`: el productor murió antes de acabar —el `exec` de
                // la extracción se cayó, el hilo se fue con un pánico— y lo que hay es un
                // volcado **truncado**. Devolver `Ok(0)` aquí es el peor fallo posible de este
                // módulo: la biblioteca lo leería como «el volcado se acabó» y daría por buena
                // una copia que existe, ocupa sitio y no se puede restaurar.
                //
                // El error es la respuesta correcta del lector, pero **no** es una alarma que
                // llegue a ninguna parte: la biblioteca lo descarta con un
                // `warn!("ignoring error: …")` y sigue, así que el archivado acaba en `Ok` con
                // el snapshot guardado sin el volcado dentro. Quien escriba el archivado no
                // puede deducir de ese `Ok` que el volcado esté completo: lo que tiene que
                // mirar es [`TunelDelVolcado::fin_consumido()`], que aquí se queda en `false`.
                // Ver el comentario de [`Read::read`] para el detalle medido.
                Err(_) => return Err(self.canal_cerrado_sin_fin()),
            }
        }
    }
}

impl LectorDeVolcado {
    /// El motivo de un productor que se cayó sin mandar [`MensajeDelVolcado::Fin`].
    ///
    /// Nombra el volcado y dice qué se teme —que está truncado—, porque el operador que lea
    /// esto tiene que poder decidir si reintenta la copia o mira el contenedor de la base.
    fn canal_cerrado_sin_fin(&self) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!(
                "el productor del volcado `{}` cerró el canal sin enviar el final: el volcado \
                 puede estar truncado, y archivarlo daría por buena una copia incompleta",
                self.nombre
            ),
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
            terminado: false,
            // El mismo `Arc` que el túnel: es el hilo que hay entre el lector y el motor.
            fin_consumido: Arc::clone(&self.fin_consumido),
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
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::TrySendError;

    use rustic_core::ReadSource as _;
    use rustic_core::ReadSourceOpen;
    use rustic_core::RusticResult;
    use rustic_core::repofile::NodeType;

    use super::{
        Error, LectorDeVolcado, MensajeDelVolcado, OrigenDelVolcado, TunelDelVolcado,
        tunel_del_volcado,
    };

    /// Un túnel de prueba, ya construido.
    ///
    /// El `expect` de la validación del nombre vive aquí y no en cada prueba porque lo que estas
    /// pruebas fijan es el puente: la validación tiene las suyas más abajo, con sus nombres
    /// imposibles uno por uno.
    fn tunel_de_prueba(nombre: &str, capacidad: usize) -> TunelDelVolcado {
        tunel_del_volcado(nombre, capacidad).expect("el nombre de esta prueba tiene que ser válido")
    }

    /// El nombre que un rechazo señala como inválido.
    ///
    /// Recibe el `Result` entero en vez de usar un `expect_err` porque ese método pide que el
    /// acierto sea `Debug`, y el túnel no lo lleva: lo único que publicaría de sí mismo es el
    /// nombre que el propio error ya trae consigo.
    ///
    /// El `match` va **sin comodín**: si la segunda mitad añade casos a [`Error`], el compilador
    /// señala este ayudante y quien lo toque tendrá que decidir qué espera de los casos nuevos,
    /// en vez de que un `_` los diera por buenos en silencio.
    fn nombre_rechazado(tunel: Result<TunelDelVolcado, Error>) -> String {
        match tunel {
            Err(Error::NombreDeVolcadoInvalido { nombre }) => nombre,
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
                    .send(MensajeDelVolcado::Datos(trozo))
                    .expect("el origen sigue vivo, así que el canal no puede estar cerrado");
            }
            productor
                .send(MensajeDelVolcado::Fin)
                .expect("el canal no puede cerrarse antes del final");
        });

        let mut lector = lector_de(&origen).expect("el lector tiene que abrirse");
        let mut leido = Vec::new();
        lector
            .read_to_end(&mut leido)
            .expect("con `Fin` al final, leer no puede fallar");

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
                    .send(MensajeDelVolcado::Datos(trozo))
                    .expect("el origen sigue vivo, así que el canal no puede estar cerrado");
            }
            productor.send(MensajeDelVolcado::Fin).expect("y el final");
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

    /// **4.** La memoria **no crece** con el tamaño del volcado.
    ///
    /// El productor deja de poder enviar en cuanto el canal llega a su capacidad, y eso es la
    /// diferencia entre una memoria acotada y una que es función del tamaño de la base. Se
    /// mide **contando** y no cronometrando: el productor intenta enviar con `try_send` —que no
    /// espera— mientras el consumidor no lee nada, así que el número de envíos que caben es
    /// exactamente el tope del canal. Con un canal sin tope el productor no pararía nunca, y
    /// el techo de la prueba —muy por encima de la capacidad— lo haría caer.
    #[test]
    fn el_canal_acotado_frena_al_productor() {
        let capacidad = 4;
        let TunelDelVolcado {
            origen, productor, ..
        } = tunel_de_prueba("volcado-enorme.sql", capacidad);

        // El consumidor no lee: nadie abre el origen. Lo único que puede frenar al productor
        // es el tope del canal.
        let contador = std::thread::spawn(move || {
            // Solo para no colgar la prueba si el canal no estuviera acotado: está por encima
            // de la capacidad, así que un canal sin tope lo alcanza y la aserción de abajo cae.
            let techo = capacidad * 4 + 16;
            let mut enviados = 0_usize;

            while enviados <= techo {
                match productor.try_send(MensajeDelVolcado::Datos(vec![0_u8; 64])) {
                    Ok(()) => enviados += 1,
                    Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => break,
                }
            }

            enviados
        });

        let enviados = contador.join().expect("el hilo productor no puede caerse");

        assert!(
            enviados >= capacidad,
            "el canal no llegó a llenarse —{enviados} mensajes de {capacidad}—: la prueba no \
             midió lo que cree medir"
        );
        assert!(
            enviados <= capacidad + 1,
            "el productor metió {enviados} mensajes de 64 bytes con el canal lleno (capacidad \
             {capacidad}): el canal no está acotado y el volcado se acumula en memoria"
        );

        // El origen sigue vivo: el productor paró por el **tope** del canal y no por
        // encontrarlo cerrado, que también habría dado una cuenta pequeña sin significar nada.
        drop(origen);
    }

    /// **5.** Un canal cerrado sin `Fin` es un error, **no** un fin de fichero.
    ///
    /// Es la prueba que impide que el lector entregue su parte muda: si devolviera `Ok(0)` aquí,
    /// la biblioteca leería «el volcado se acabó» y daría por buena una copia incompleta que
    /// existe, ocupa sitio y no se puede restaurar.
    ///
    /// Y fija además el lado que de verdad se puede consultar desde fuera: con el productor
    /// caído, la marca del final se queda en `false`, que es lo que el motor mirará para saber
    /// que ese volcado no llegó entero.
    #[test]
    fn un_canal_cerrado_sin_fin_es_un_error_y_no_un_fin_de_fichero() {
        let TunelDelVolcado {
            origen,
            productor,
            fin_consumido,
        } = tunel_de_prueba("volcado-truncado.sql", 4);

        productor
            .send(MensajeDelVolcado::Datos(b"parcial".to_vec()))
            .expect("el canal tiene sitio de sobra");

        // El productor se cae sin mandar `Fin`: es el caso de un `exec` que murió a medias.
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
            "un cierre sin `Fin` tiene que ser un error: devolver `Ok(0)` aquí archiva un \
             volcado truncado como si fuera válido",
        );

        let motivo = error.to_string();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::UnexpectedEof,
            "el fallo de un cierre sin `Fin` tiene que clasificarse como un final inesperado, \
             no como otra cosa"
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
            "una lectura posterior volvió a darse por buena tras un cierre sin `Fin`"
        );

        // La marca del final se mira aquí en el `Arc` a pelo y no con el acceso del túnel
        // —`fin_consumido()`— porque hay que **soltar** el productor para cerrar el canal, y eso
        // desarma el túnel: un campo movido deja el resto fuera de alcance. El acceso tiene sus
        // dos pruebas más abajo.
        assert!(
            !fin_consumido.load(Ordering::Acquire),
            "un productor que se cayó sin mandar el final dejó la marca del fin consumido puesta: \
             el motor daría por buena una copia que no llegó entera"
        );
    }

    /// **6.** `entries()` dos veces da un **error** y no un iterador vacío.
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

    /// **7.** `entries()` da **exactamente una** entrada, con su camino y su nodo de fichero.
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

    /// **8.** El fin **no** está consumido mientras el volcado no haya terminado.
    ///
    /// Es la mitad que hace útil la marca: si valiera `true` desde el principio, el motor no
    /// tendría ninguna señal —una marca que siempre dice «sí» es un adorno—. Se comprueba en los
    /// dos momentos en los que el archivado puede estar antes del final: antes de leer nada y con
    /// bytes ya entregados pero el volcado todavía abierto.
    #[test]
    fn el_fin_no_esta_consumido_mientras_el_volcado_va_a_medias() {
        let tunel = tunel_de_prueba("volcado-a-medias.sql", 4);

        assert!(
            !tunel.fin_consumido(),
            "un túnel recién hecho ya venía con el fin consumido"
        );

        let primer_trozo = b"CREATE TABLE tienda (id INT);\n".to_vec();
        tunel
            .productor
            .send(MensajeDelVolcado::Datos(primer_trozo.clone()))
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
            !tunel.fin_consumido(),
            "el volcado estaba a medias y la marca del fin ya estaba puesta: el motor daría por \
             buena una copia que no llegó entera"
        );
    }

    /// **9.** El fin **sí** se marca cuando el lector consume el mensaje de final.
    ///
    /// Es la otra mitad, y la que el motor va a consultar. Se hace pasar al lector por el camino
    /// completo —leer hasta el final— para que la marca la ponga el propio `read` al encontrarse
    /// el `Fin`, y no una llamada de la prueba.
    #[test]
    fn el_fin_se_marca_cuando_el_lector_consume_el_final() {
        let tunel = tunel_de_prueba("volcado-entero.sql", 4);

        let esperado = b"CREATE TABLE tienda (id INT);\nINSERT INTO tienda VALUES (1);\n".to_vec();
        // El emisor se **clona**: así el túnel sigue entero —sus dos mitades y la marca— y se
        // puede consultar el acceso al final después de que el hilo haya terminado.
        let productor = tunel.productor.clone();
        let a_enviar = esperado.clone();

        let emisor = std::thread::spawn(move || {
            productor
                .send(MensajeDelVolcado::Datos(a_enviar))
                .expect("el canal no puede estar cerrado antes del final");
            productor
                .send(MensajeDelVolcado::Fin)
                .expect("el canal no puede cerrarse antes del final");
        });

        let mut lector = lector_de(&tunel.origen).expect("el lector tiene que abrirse");
        let mut leido = Vec::new();
        lector
            .read_to_end(&mut leido)
            .expect("con `Fin` al final, leer no puede fallar");

        emisor.join().expect("el hilo productor no puede caerse");

        assert_eq!(
            leido, esperado,
            "lo leído no es el volcado que se envió, así que la marca hablaría de otra cosa"
        );
        assert!(
            tunel.fin_consumido(),
            "el lector consumió el mensaje de final y la marca sigue en falso: el motor no \
             tendría forma de distinguir una copia buena de una que murió a mitad"
        );
    }

    /// **10.** Una lectura con el búfer **vacío** devuelve `0`, no espera y **no** consume nada.
    ///
    /// Es el único `Ok(0)` alcanzable sin haber recibido el final: el contrato de [`Read`] manda
    /// contestarlo con un búfer de longitud cero, y contestarlo **en el acto**. Se miden las dos
    /// cosas. Primero la inmediatez, con el productor todavía callado: una implementación que se
    /// pusiera a esperar un mensaje ahí no volvería nunca. Después que no se consume nada: la
    /// lectura siguiente, con un búfer de verdad, tiene que recibir el **primer** byte —si la
    /// lectura vacía se hubiera llevado por delante el mensaje, el volcado saldría corto por
    /// delante y se archivarían los bytes que quedaran como si fuera el volcado entero—.
    ///
    /// La inmediatez se mide con un hilo y un plazo, y no con una aserción directa, porque el
    /// defecto que se caza no da un fallo sino un **cuelgue**: sin plazo, la prueba se quedaría
    /// colgada en vez de decir qué pasó.
    #[test]
    fn una_lectura_con_bufer_vacio_no_espera_ni_consume_nada() {
        let tunel = tunel_de_prueba("volcado-sin-bufer.sql", 4);
        let lector = lector_de(&tunel.origen).expect("el lector tiene que abrirse");

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

        let devuelto = respuesta
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect(
                "la lectura con el búfer vacío se quedó esperando un mensaje que nadie había \
                 mandado: el contrato de `Read` obliga a contestar `0` en el acto",
            );

        assert_eq!(
            devuelto.expect("un búfer vacío no puede fallar"),
            0,
            "el contrato de `Read` obliga a contestar `0` con un búfer vacío"
        );
        assert!(
            !tunel.fin_consumido(),
            "la lectura con el búfer vacío se tomó por un final de fichero del volcado"
        );

        let mut lector = hilo_vacio
            .join()
            .expect("el hilo de la lectura vacía no puede caerse");

        // Ya con el volcado en marcha: el productor, en su hilo.
        let esperado = b"CREATE TABLE tienda (id INT);\n".to_vec();
        let productor = tunel.productor.clone();
        let a_enviar = esperado.clone();

        let emisor = std::thread::spawn(move || {
            productor
                .send(MensajeDelVolcado::Datos(a_enviar))
                .expect("el canal no puede estar cerrado antes del final");
            productor
                .send(MensajeDelVolcado::Fin)
                .expect("el canal no puede cerrarse antes del final");
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
            .expect("con `Fin` al final, leer no puede fallar");
        emisor.join().expect("el hilo productor no puede caerse");

        assert_eq!(
            [bufer[..entregados].as_ref(), resto.as_slice()].concat(),
            esperado,
            "entre la lectura vacía y las siguientes no salió el volcado entero"
        );
    }

    /// **11.** Un nombre de volcado **vacío** se rechaza antes de llegar a la biblioteca.
    ///
    /// No es cosmético: con el nombre vacío la biblioteca entra en **pánico** —su camino hacia el
    /// árbol hace `expect("file path should have a parent!")`, y un camino vacío no tiene padre—,
    /// así que rechazarlo aquí es la diferencia entre un error accionable y un hilo de archivado
    /// muerto a mitad de una copia.
    #[test]
    fn un_nombre_de_volcado_vacio_se_rechaza() {
        assert_eq!(
            nombre_rechazado(tunel_del_volcado("", 4)),
            "",
            "el error no señala el nombre vacío como el que rechazó"
        );
    }

    /// **12.** Un nombre de volcado que sea `.` se rechaza.
    ///
    /// `.` nombra al directorio en el que se está, no a un fichero: la entrada quedaría con un
    /// nombre que no nombra nada y el snapshot guardaría un volcado que nadie podría identificar.
    #[test]
    fn un_nombre_de_volcado_de_punto_se_rechaza() {
        assert_eq!(
            nombre_rechazado(tunel_del_volcado(".", 4)),
            ".",
            "el error no señala el punto como el nombre que rechazó"
        );
    }

    /// **13.** Un nombre de volcado que sea `..` se rechaza, por lo mismo que el punto: no nombra
    /// un fichero, nombra un salto de directorio.
    #[test]
    fn un_nombre_de_volcado_de_punto_punto_se_rechaza() {
        assert_eq!(
            nombre_rechazado(tunel_del_volcado("..", 4)),
            "..",
            "el error no señala los dos puntos como el nombre que rechazó"
        );
    }

    /// **14.** Un nombre con **separador** se rechaza: con él dentro el nombre deja de ser un
    /// nombre y pasa a ser un camino, con la entrada partida en trozos y un nodo que lleva un
    /// separador dentro de su propio nombre.
    #[test]
    fn un_nombre_de_volcado_con_barra_se_rechaza() {
        assert_eq!(
            nombre_rechazado(tunel_del_volcado("volcados/tienda.sql", 4)),
            "volcados/tienda.sql",
            "el error no señala el nombre con barra como el que rechazó"
        );
    }

    /// **15.** Y con el otro separador también: en otras plataformas el camino se parte igual, y
    /// aquí el nombre es el mismo en todas, así que se rechazan los dos.
    #[test]
    fn un_nombre_de_volcado_con_barra_invertida_se_rechaza() {
        assert_eq!(
            nombre_rechazado(tunel_del_volcado("volcados\\tienda.sql", 4)),
            "volcados\\tienda.sql",
            "el error no señala el nombre con barra invertida como el que rechazó"
        );
    }

    /// **16.** Un nombre normal **sí** pasa, y el túnel que sale de él vale.
    ///
    /// Es el otro lado de la validación: una puerta que lo rechazara todo sería tan inútil como
    /// una que no rechazara nada. Un nombre de verdad —con su punto, sus guiones y su extensión—
    /// no solo se acepta: llega al `path` de la entrada, que es lo que fija la prueba 7.
    #[test]
    fn un_nombre_de_volcado_normal_si_pasa() {
        let tunel = tunel_de_prueba("volcado-de-tienda-2026-10-25.sql", 4);

        assert!(
            !tunel.fin_consumido(),
            "un túnel recién construido no puede traer el fin ya consumido"
        );
    }
}
