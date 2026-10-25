//! El gancho de progreso: lo que la biblioteca informa, traducido a lo que el motor entiende.
//!
//! La biblioteca pide un [`ProgressBars`] al archivador y le devuelve a cambio
//! [`RusticProgress`] por cada operación que quiere informar. El motor no habla ninguno de los
//! dos: habla [`AvisoDeProgreso`], que es un enumerado propio con tres desenlaces —el total, el
//! avance y el final— y ni un dato del volcado dentro.
//!
//! La traducción está sesgada a propósito hacia lo que el panel puede pintar:
//!
//! - [`ProgressType::Bytes`] es el único que produce [`AvisoDeProgreso::Total`]. Es el progreso
//!   de bytes, y el total del panel es un total **de bytes**; convertirlo en el total de otro
//!   tipo de progreso —un contador de ficheros, por ejemplo— pondría un denominador que no
//!   corresponde al numerador, que es peor que no poner ninguno.
//! - El título que la biblioteca pone a una barra **se ignora a propósito**, y no por descuido:
//!   el título de una entrada es su ruta, y convertirlo en un aviso sería abrir un canal por el
//!   que las rutas del volcado salen hacia el motor, que a su vez las pone en el panel. Un
//!   `current_action` que lleva dentro la ruta de un fichero de la base es un dato del volcado
//!   fuera de su sitio, y este módulo no lo abre. El mismo criterio vale para el `prefix` de
//!   [`ProgressBars::progress`].

use std::sync::Arc;

use rustic_core::{Progress, ProgressBars, ProgressType, RusticProgress};

/// Un aviso de progreso del archivador, ya traducido a lo que el motor entiende.
///
/// Lleva cantidades y nada más: ni una ruta, ni un título, ni un byte del volcado. Es la
/// superficie por la que el progreso puede salir hacia el motor sin arrastrar datos de la base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvisoDeProgreso {
    /// El archivador fija el total que va a procesar.
    Total(u64),
    /// El archivador ha procesado estos bytes más.
    Avance(u64),
    /// Una operación terminó.
    Fin,
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

    /// El total, y **solo** del progreso de bytes.
    ///
    /// El total del panel es un total de bytes, así que un `set_length` de otro tipo —el de un
    /// contador de ficheros, por ejemplo— no se traduce: daría un denominador que no
    /// corresponde al numerador, que es peor que no tener ninguno. Y del `Spinner` no se traduce
    /// nada: la biblioteca no le fija longitud —está medido en su documentación: «doesn't get a
    /// length and is not advanced»— y lo usa para esperas, no para trabajo.
    fn set_length(&self, longitud: u64) {
        if matches!(self.tipo, ProgressType::Bytes) {
            (self.al_avisar)(AvisoDeProgreso::Total(longitud));
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

    /// El avance, de los dos tipos que cuentan algo.
    ///
    /// El `Spinner` no: no se avanza. Sí se avanza un `Counter` —los ficheros procesados—, y
    /// aunque no vaya a ninguna parte del panel, descartarlo aquí sería decidir por el motor.
    fn inc(&self, incremento: u64) {
        if !matches!(self.tipo, ProgressType::Spinner) {
            (self.al_avisar)(AvisoDeProgreso::Avance(incremento));
        }
    }

    /// El final de una operación que **estaba contando**.
    ///
    /// Un `Spinner` que termina no produce aviso: nunca mandó un `Total` ni un `Avance`, así que
    /// no hay ninguna barra que cerrar y un `Fin` suelto descuadraría la cuenta del motor. Los
    /// otros dos sí: la barra existe y hay que decir que se acabó.
    fn finish(&self) {
        if !matches!(self.tipo, ProgressType::Spinner) {
            (self.al_avisar)(AvisoDeProgreso::Fin);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use rustic_core::{ProgressBars, ProgressType};

    use super::{AvisoDeProgreso, BarrasDelMotor};

    /// **8.** El progreso traduce los avisos con los tres desenlaces.
    ///
    /// Con [`ProgressType::Bytes`] —el único que fija un total— el motor tiene que recibir la
    /// longitud como [`AvisoDeProgreso::Total`], el incremento como [`AvisoDeProgreso::Avance`]
    /// y el final como [`AvisoDeProgreso::Fin`]. Y de paso queda fijado que el título **no**
    /// produce ningún aviso: si lo produjera, la ruta de una entrada viajaría al panel.
    #[test]
    fn las_barras_traducen_los_avisos() {
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
                AvisoDeProgreso::Total(2048),
                AvisoDeProgreso::Avance(512),
                AvisoDeProgreso::Fin,
            ],
            "los avisos no son los tres que el motor espera, o el título se convirtió en uno"
        );
    }

    /// **9.** El `Debug` de las barras **no publica el callback**.
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
