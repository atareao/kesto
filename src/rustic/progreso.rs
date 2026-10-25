//! El gancho de progreso: lo que la biblioteca informa, traducido a lo que el motor entiende.
//!
//! La biblioteca pide un [`ProgressBars`] al archivador y le devuelve a cambio
//! [`RusticProgress`] por cada operación que quiere informar. El motor no habla ninguno de los
//! dos: habla [`AvisoDeProgreso`], que es un enumerado propio con tres desenlaces —el total de
//! bytes, los bytes y el fin de los bytes— y ni un dato del volcado dentro.
//!
//! La traducción está sesgada a propósito hacia lo que el panel puede pintar, y el sesgo va en
//! una sola dirección: **por este canal solo viajan bytes**.
//!
//! - [`ProgressType::Bytes`] es el único tipo que se traduce, y el total del panel es un total
//!   **de bytes**; usar el total de otro tipo de progreso pondría un denominador que no
//!   corresponde al numerador, que es peor que no poner ninguno.
//! - [`ProgressType::Counter`] se descarta **entero**, y eso no es comodidad: la biblioteca usa
//!   un contador en el camino normal del archivado —lo pide para buscar el snapshot padre, entre
//!   otros sitios— y avanza **1 por snapshot**. Si sus incrementos entraran en este canal, el
//!   motor recibiría `Bytes(1)` mezclados con los bytes de verdad, publicaría un
//!   `bytes_uploaded` que no corresponde a ninguna magnitud y no tendría forma de detectarlo.
//!   El daño no sería un aviso perdido sino una cifra falsa con la que el panel miente.
//! - [`ProgressType::Spinner`] tampoco: la biblioteca no le fija longitud ni lo avanza —está en
//!   su documentación: «doesn't get a length and is not advanced»—, así que no hay nada que
//!   traducir.
//! - El título que la biblioteca pone a una barra **se ignora a propósito**, y no por descuido:
//!   el título de una entrada es su ruta, y convertirlo en un aviso sería abrir un canal por el
//!   que las rutas del volcado salen hacia el motor, que a su vez las pone en el panel. Un
//!   `current_action` que lleva dentro la ruta de un fichero de la base es un dato del volcado
//!   fuera de su sitio, y este módulo no lo abre. El mismo criterio vale para el `prefix` de
//!   [`ProgressBars::progress`].

use std::sync::Arc;

use rustic_core::{Progress, ProgressBars, ProgressType, RusticProgress};

/// Un aviso de progreso del archivador, ya traducido.
///
/// Solo viajan **bytes**: es lo que el motor publica como avance de la copia, y mezclar ahí una
/// cuenta de ficheros daría una cifra que no corresponde a ninguna magnitud. Lleva cantidades y
/// nada más: ni una ruta, ni un título, ni un byte del volcado. Es la superficie por la que el
/// progreso puede salir hacia el motor sin arrastrar datos de la base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvisoDeProgreso {
    /// El archivador fija el total de bytes que va a procesar.
    TotalDeBytes(u64),
    /// El archivador ha procesado estos bytes más.
    Bytes(u64),
    /// La operación de bytes terminó.
    FinDeBytes,
}

/// Las barras que el motor le pasa a la biblioteca.
///
/// Se construyen una vez por operación y se comparten con el archivador, que las clona por
/// dentro. Lo único que guardan es el destino del aviso.
pub struct BarrasDelMotor {
    /// El destino del aviso. Es un `Arc<dyn Fn…>` y no un trait propio porque el motor solo
    /// necesita un destino —el hub de eventos, en la práctica—, y un trait de un solo método con
    /// una sola implementación es una indirección que no compra nada.
    al_avisar: Arc<dyn Fn(AvisoDeProgreso) + Send + Sync>,
}

impl BarrasDelMotor {
    /// Construye las barras sobre el aviso que el motor quiera recibir.
    pub fn nuevas(al_avisar: Arc<dyn Fn(AvisoDeProgreso) + Send + Sync>) -> Self {
        Self { al_avisar }
    }
}

impl std::fmt::Debug for BarrasDelMotor {
    /// Un `Debug` a mano porque [`ProgressBars`] lo exige y un `Arc<dyn Fn…>` no lo implementa,
    /// pero que **no** publique nada del cierre: en producción el cierre captura el hub de
    /// eventos, y un `Debug` derivado no sabría que no debe imprimirlo. La parte útil para
    /// diagnosticar —de qué tipo es— sí sale.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BarrasDelMotor").finish_non_exhaustive()
    }
}

impl ProgressBars for BarrasDelMotor {
    /// Crea la barra de una operación.
    ///
    /// El `prefix` que pasa la biblioteca se ignora: es un título para humanos —«backing up...»,
    /// «waiting 10s...»— y por aquí no se traduce texto libre. Ver el encabezado del módulo.
    fn progress(&self, tipo: ProgressType, _prefijo: &str) -> Progress {
        Progress::new(BarraDelMotor {
            tipo,
            al_avisar: Arc::clone(&self.al_avisar),
        })
    }
}

/// Una barra concreta: el tipo del progreso y el aviso de vuelta.
///
/// Es privado a propósito: lo que el motor ve son las [`BarrasDelMotor`] y los
/// [`AvisoDeProgreso`], no un tipo de la biblioteca.
struct BarraDelMotor {
    /// El tipo que la biblioteca pidió para esta barra: es lo que decide qué se traduce.
    tipo: ProgressType,
    /// El mismo destino que el de las barras.
    al_avisar: Arc<dyn Fn(AvisoDeProgreso) + Send + Sync>,
}

impl std::fmt::Debug for BarraDelMotor {
    /// Igual que el de las barras: el tipo del progreso se publica —es lo que hace legible un
    /// fallo— y el cierre **no**. Hace falta porque la biblioteca imprime este `Debug` a través
    /// de su propio [`Progress`].
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BarraDelMotor")
            .field("tipo", &self.tipo)
            .finish_non_exhaustive()
    }
}

impl RusticProgress for BarraDelMotor {
    /// Las barras del motor nunca están ocultas: el motor quiere el aviso.
    fn is_hidden(&self) -> bool {
        false
    }

    /// El total de bytes, y **solo** del progreso de bytes.
    ///
    /// El total del panel es un total de bytes, así que un `set_length` de otro tipo —el del
    /// contador de snapshots con el que la biblioteca busca el padre, por ejemplo— no se traduce:
    /// daría un denominador que no corresponde al numerador, que es peor que no tener ninguno. Y
    /// del `Spinner` no se traduce nada: la biblioteca no le fija longitud —está medido en su
    /// documentación: «doesn't get a length and is not advanced»— y lo usa para esperas, no para
    /// trabajo.
    fn set_length(&self, longitud: u64) {
        if matches!(self.tipo, ProgressType::Bytes) {
            (self.al_avisar)(AvisoDeProgreso::TotalDeBytes(longitud));
        }
    }

    /// El título se **ignora**, y es una decisión y no un olvido.
    ///
    /// El título de una entrada es su ruta —la biblioteca pone ahí el camino del fichero que
    /// está procesando—, así que traducirlo a un aviso abriría un canal por el que las rutas del
    /// volcado salen hacia el motor y de ahí al panel. Un `current_action` con la ruta de un
    /// fichero de la base dentro es un dato del volcado fuera de su sitio. Por la misma razón no
    /// se registra: no hay ni un `tracing` que lleve esto.
    fn set_title(&self, _titulo: &str) {}

    /// El avance en bytes, y **solo** del progreso de bytes.
    ///
    /// Ni el `Counter` ni el `Spinner` producen aviso. El `Spinner` porque no se avanza. Y el
    /// contador porque **cuenta otra cosa**: la biblioteca lo usa para recorrer snapshots o
    /// entradas, y su `1` no es un byte. Traducirlo daría un `bytes_uploaded` sistemáticamente
    /// falso —y hacia arriba, que es la dirección peor— sin nada que lo delatara: los dos avisos
    /// llegarían al motor con la misma forma, y el motor no tendría forma de separarlos.
    fn inc(&self, incremento: u64) {
        if matches!(self.tipo, ProgressType::Bytes) {
            (self.al_avisar)(AvisoDeProgreso::Bytes(incremento));
        }
    }

    /// El final de la operación de bytes.
    ///
    /// Los otros dos tipos no producen aviso: un `Spinner` que termina no mandó ni un total ni un
    /// avance, así que no hay ninguna barra que cerrar, y de un `Counter` no ha salido nada por
    /// este canal, así que un `FinDeBytes` suyo cerraría una barra que no existe. En los dos
    /// casos un aviso suelto descuadraría la cuenta del motor.
    fn finish(&self) {
        if matches!(self.tipo, ProgressType::Bytes) {
            (self.al_avisar)(AvisoDeProgreso::FinDeBytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use rustic_core::{ProgressBars, ProgressType};

    use super::{AvisoDeProgreso, BarrasDelMotor};

    /// **8.** El progreso traduce los avisos de **bytes** con los tres desenlaces.
    ///
    /// Con [`ProgressType::Bytes`] —el único que fija un total— el motor tiene que recibir la
    /// longitud como [`AvisoDeProgreso::TotalDeBytes`], el incremento como
    /// [`AvisoDeProgreso::Bytes`] y el final como [`AvisoDeProgreso::FinDeBytes`]. Y de paso
    /// queda fijado que el título **no** produce ningún aviso: si lo produjera, la ruta de una
    /// entrada viajaría al panel.
    #[test]
    fn las_barras_traducen_los_avisos_de_bytes() {
        let recogidos = Arc::new(Mutex::new(Vec::new()));
        let destino = Arc::clone(&recogidos);

        let barras = BarrasDelMotor::nuevas(Arc::new(move |aviso: AvisoDeProgreso| {
            destino
                .lock()
                .expect("el candado de la prueba no puede estar envenenado")
                .push(aviso);
        }));

        let progreso = barras.progress(ProgressType::Bytes, "volcado");
        assert!(
            !progreso.is_hidden(),
            "las barras del motor no están ocultas: el motor quiere el aviso"
        );

        progreso.set_length(2048);
        // El título lleva la ruta de una entrada, y **no** puede salir como aviso.
        progreso.set_title("volcado/tienda.sql");
        progreso.inc(512);
        progreso.finish();

        let avisos = recogidos
            .lock()
            .expect("el candado de la prueba no puede estar envenenado")
            .clone();

        assert_eq!(
            avisos,
            vec![
                AvisoDeProgreso::TotalDeBytes(2048),
                AvisoDeProgreso::Bytes(512),
                AvisoDeProgreso::FinDeBytes,
            ],
            "los avisos no son los tres que el motor espera, o el título se convirtió en uno"
        );
    }

    /// **9.** Un `Counter` **no** produce ningún aviso.
    ///
    /// Es la prueba que impide que vuelva el error: la biblioteca usa un contador en el camino
    /// normal del archivado —para buscar el snapshot padre, entre otros— y avanza 1 por
    /// snapshot, así que sus avisos llegarían al motor con **la misma forma** que los bytes y
    /// `bytes_uploaded` saldría falso de manera sistemática, sin nada que lo delatara. Al
    /// contador se le dan las cuatro llamadas que puede recibir —longitud, título, incremento y
    /// final— y de aquí no puede salir ninguna.
    ///
    /// Y después se traduce una barra de bytes contra el **mismo** destino: sin eso, un destino
    /// roto haría pasar esta prueba sin haber medido nada.
    #[test]
    fn un_contador_no_produce_ningun_aviso() {
        let recogidos = Arc::new(Mutex::new(Vec::new()));
        let destino = Arc::clone(&recogidos);

        let barras = BarrasDelMotor::nuevas(Arc::new(move |aviso: AvisoDeProgreso| {
            destino
                .lock()
                .expect("el candado de la prueba no puede estar envenenado")
                .push(aviso);
        }));

        let contador = barras.progress(ProgressType::Counter, "getting snapshots...");
        contador.set_length(7);
        // El título de un contador también lleva una ruta, y tampoco puede salir.
        contador.set_title("volcado/tienda.sql");
        contador.inc(1);
        contador.finish();

        assert!(
            recogidos
                .lock()
                .expect("el candado de la prueba no puede estar envenenado")
                .is_empty(),
            "un `Counter` mandó avisos: sus incrementos de snapshot llegarían al motor \
             mezclados con los bytes, y `bytes_uploaded` saldría falso sin que nada lo delatara"
        );

        let de_bytes = barras.progress(ProgressType::Bytes, "backing up...");
        de_bytes.set_length(1024);
        de_bytes.inc(256);
        de_bytes.finish();

        assert_eq!(
            *recogidos
                .lock()
                .expect("el candado de la prueba no puede estar envenenado"),
            vec![
                AvisoDeProgreso::TotalDeBytes(1024),
                AvisoDeProgreso::Bytes(256),
                AvisoDeProgreso::FinDeBytes,
            ],
            "el mismo destino tampoco dejó pasar los bytes: la comprobación del contador se hizo \
             contra un canal roto y no mide nada"
        );
    }

    /// **10.** El `Debug` de las barras **no publica el callback**.
    ///
    /// Un `Arc<dyn Fn…>` no implementa `Debug`, y el trait lo exige, así que hay uno a mano; lo
    /// que no puede hacer ese `Debug` es imprimir el contenido del cierre, porque en producción
    /// el cierre captura el hub de eventos y sus datos. Aquí se comprueba con una cadena
    /// distinguible que tiene que **no** aparecer —ni en el `Debug` de las barras ni en el de la
    /// barra concreta, que es la que la biblioteca imprime a través de `Progress`—.
    #[test]
    fn el_debug_de_las_barras_no_publica_el_callback() {
        let distinguible = String::from("cadena-distinguible-del-callback-0001");
        let capturada = distinguible.clone();

        let barras = BarrasDelMotor::nuevas(Arc::new(move |_aviso: AvisoDeProgreso| {
            // El cierre **nombra** la cadena para que la captura exista de verdad: un cierre
            // que no usara nada no capturaría nada y esta prueba no probaría nada.
            let _ = capturada.len();
        }));

        let debug = format!("{barras:?}");
        assert!(
            debug.contains("BarrasDelMotor"),
            "el Debug no dice ni de qué tipo es: {debug}"
        );
        assert!(
            !debug.contains(&distinguible),
            "el Debug de las barras publicó el contenido del callback: {debug}"
        );

        let progreso = barras.progress(ProgressType::Bytes, "volcado");
        let debug_del_progreso = format!("{progreso:?}");
        assert!(
            !debug_del_progreso.contains(&distinguible),
            "el Debug de la barra publicó el contenido del callback: {debug_del_progreso}"
        );
    }
}
