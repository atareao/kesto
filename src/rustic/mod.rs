//! El trato con Rustic: el puente por el que entra el volcado y el progreso que sale.
//!
//! Este módulo es la frontera entre el motor de copias y la biblioteca que las escribe, y
//! existe porque las dos partes no hablan el mismo idioma. La extracción produce bytes
//! **asíncronos y a trozos** —el `stream` de un `exec` de la API de Docker—, mientras que
//! la biblioteca pide un origen de lectura **bloqueante**: [`rustic_core::ReadSource`]. Las
//! dos piezas que cruzan esa frontera son el puente ([`origen`]) y el gancho de progreso
//! ([`progreso`]).
//!
//! Nada de lo que entra por aquí pasa por el disco, y nada que salga de aquí lleva datos del
//! volcado: ni un [`std::fmt::Debug`], ni un registro.
//!
//! El único fallo que este módulo puede rechazar por su cuenta es el que recoge [`Error`] —un
//! nombre de volcado que no sirve para nombrar nada—: lo que ya está en manos de la biblioteca
//! no se puede validar desde aquí, y sus pánicos son suyos.

mod origen;
mod progreso;

pub use origen::{
    Error, LectorDeVolcado, MensajeDelVolcado, OrigenDelVolcado, TunelDelVolcado, tunel_del_volcado,
};
pub use progreso::{AvisoDeProgreso, BarrasDelMotor};
