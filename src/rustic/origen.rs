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
//! 3. **`entries()` solo se puede llamar una vez.** El receptor se saca del [`Mutex`] con
//!    `.take()` —el trait exige `Sync` y un [`Receiver`] no lo es, así que el candado no se
//!    puede evitar: es el mismo truco que usa el `ChildStdoutSource` de la propia
//!    biblioteca—, y una segunda llamada tiene que devolver un iterador con un **error**
//!    dentro, nunca uno vacío: un iterador vacío archiva un snapshot de cero entradas dado por
//!    bueno, que es justo lo contrario de lo que este módulo existe para impedir.

use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use rustic_core::repofile::{Metadata, Node, NodeType};
use rustic_core::{ErrorKind, ReadSource, ReadSourceEntry, RusticError, RusticResult};

/// Las dos mitades de un origen de volcado.
///
/// Se construyen juntas con [`tunel_del_volcado`] y no se pueden construir por separado: un
/// origen sin su productor es un origen que nunca terminará, y un productor sin su origen es
/// un productor que se bloquea para siempre en cuanto el canal se llena.
pub struct TunelDelVolcado {
    /// La mitad que se le entrega a la biblioteca de copia.
    pub origen: OrigenDelVolcado,
    /// La mitad por la que el productor empuja los bytes.
    pub productor: SyncSender<MensajeDelVolcado>,
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
pub fn tunel_del_volcado(nombre: impl Into<String>, capacidad: usize) -> TunelDelVolcado {
    // `sync_channel` y no `channel`: es la diferencia entre un canal que acota y uno que no.
    let (productor, receptor) = sync_channel(capacidad);

    TunelDelVolcado {
        origen: OrigenDelVolcado {
            nombre: nombre.into(),
            receptor: Mutex::new(Some(receptor)),
        },
        productor,
    }
}

/// La mitad que implementa [`ReadSource`]: lo que la biblioteca de copia recibe.
///
/// Cabe en un `Sync + Send` —que es lo que el trait exige— porque lo único compartido es el
/// [`Mutex`] con el receptor dentro.
pub struct OrigenDelVolcado {
    /// El nombre con el que el motor pidió el volcado: es su única identidad, porque no hay
    /// ningún fichero detrás.
    nombre: String,
    /// El receptor, dentro de un candado y de un `Option`.
    ///
    /// El candado no es un adorno: [`ReadSource`] exige `Sync` y un [`Receiver`] no lo es. Y el
    /// `Option` está porque [`ReadSource::entries`] recibe `&self` mientras que el receptor hay
    /// que **sacarlo** para entregárselo al lector, así que la operación es un `.take()`.
    receptor: Mutex<Option<Receiver<MensajeDelVolcado>>>,
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
    /// [`std::io::Error`]—. Lo que decide es el mensaje, no el estado del canal.
    terminado: bool,
}

impl Read for LectorDeVolcado {
    /// Entrega hasta `buf.len()` bytes del volcado, esperando al productor si hace falta.
    ///
    /// El único desenlace que devuelve `Ok(0)` es haber recibido [`MensajeDelVolcado::Fin`]. Un
    /// canal cerrado sin ese mensaje devuelve un error: **confundirlos archiva un volcado
    /// truncado como si fuera válido**, que es una copia que existe, ocupa sitio y no se puede
    /// restaurar.
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // El contrato de `Read` obliga a contestar `Ok(0)` con un búfer vacío, y además pedir un
        // mensaje que no cabe en ningún sitio sería retenerlo sin destino.
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
                    return Ok(0);
                }
                // El canal se cerró sin `Fin`: el productor murió antes de acabar —el `exec` de
                // la extracción se cayó, el hilo se fue con un pánico— y lo que hay es un
                // volcado **truncado**. Devolver `Ok(0)` aquí es el peor fallo posible de este
                // módulo: la biblioteca lo leería como «el volcado se acabó» y archivar un
                // volcado truncado como si fuera válido da una copia que existe, ocupa sitio y
                // no se puede restaurar. El error es del tipo que `archive()` propaga, así que
                // la ejecución acaba en `failed` y no se guarda nada.
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
    /// La segunda llamada **no** puede devolver un iterador vacío: un origen de cero entradas
    /// archiva un snapshot vacío y lo da por bueno. La salida es un iterador de un elemento que
    /// es un error.
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
/// El motivo dice **qué** se llamó de más y **por qué** importa, porque quien lea esto en un
/// registro tiene que poder saber que el volcado que no se archivó no es un fallo del volcado.
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
    use std::sync::mpsc::TrySendError;

    use rustic_core::ReadSource as _;
    use rustic_core::ReadSourceOpen;
    use rustic_core::RusticResult;
    use rustic_core::repofile::NodeType;

    use super::{
        LectorDeVolcado, MensajeDelVolcado, OrigenDelVolcado, TunelDelVolcado, tunel_del_volcado,
    };

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
        let tunel = tunel_del_volcado("volcado-de-prueba.sql", 4);

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
        let TunelDelVolcado { origen, productor } = tunel_del_volcado("volcado-de-tienda.sql", 2);

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
        let TunelDelVolcado { origen, productor } = tunel_del_volcado("volcado-desigual.sql", 2);

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
        let TunelDelVolcado { origen, productor } =
            tunel_del_volcado("volcado-enorme.sql", capacidad);

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
    /// Es la prueba que impide archivar un volcado truncado: si el lector devolviera `Ok(0)`
    /// aquí, la biblioteca daría el volcado por completo y guardaría una copia incompleta que
    /// existe, ocupa sitio y no se puede restaurar.
    #[test]
    fn un_canal_cerrado_sin_fin_es_un_error_y_no_un_fin_de_fichero() {
        let TunelDelVolcado { origen, productor } = tunel_del_volcado("volcado-truncado.sql", 4);

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
    }

    /// **6.** `entries()` dos veces da un **error** y no un iterador vacío.
    ///
    /// El receptor se entrega con `.take()`, así que la segunda llamada no tiene de dónde
    /// leer. Devolver un iterador vacío ahí haría que la biblioteca archivara un snapshot de
    /// cero entradas y lo diera por bueno; lo que tiene que llegarle es un error.
    #[test]
    fn los_entries_no_se_pueden_pedir_dos_veces() {
        let TunelDelVolcado { origen, productor } = tunel_del_volcado("volcado-de-prueba.sql", 4);

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
            "la segunda llamada dio un iterador vacío: eso archiva un snapshot de cero \
             entradas dado por bueno"
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
        let TunelDelVolcado { origen, productor } = tunel_del_volcado("volcado-de-tienda.sql", 4);

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
}
