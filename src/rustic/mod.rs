//! El trato con Rustic: poner el repositorio en marcha, meter el volcado dentro y retener.
//!
//! Este módulo es la frontera entre el motor de copias y la biblioteca que las escribe, y
//! existe porque las dos partes no hablan el mismo idioma. La extracción produce bytes
//! **asíncronos y a trozos** —el `stream` de un `exec` de la API de Docker—, mientras que
//! la biblioteca pide un origen de lectura **bloqueante**: [`rustic_core::ReadSource`]. Las
//! cuatro piezas que cruzan esa frontera son el puente ([`origen`]), el gancho de progreso
//! ([`progreso`]), el repositorio ([`repositorio`]) y la retención ([`retencion`]).
//!
//! Nada de lo que entra por aquí pasa por el disco, y nada que salga de aquí lleva datos del
//! volcado: ni un [`std::fmt::Debug`], ni un registro.
//!
//! Lo que este módulo rechaza por su cuenta es lo que la biblioteca no puede validar —un
//! nombre de volcado que no sirve para nombrar nada, una copia sin rutas, una fila que no
//! describe un destino utilizable— y los desenlaces que la biblioteca no distingue, como
//! cuál de los tres le ocurrió a la puesta en marcha de un repositorio. Lo que ya está en
//! manos de la biblioteca no se puede validar desde aquí, y sus pánicos son suyos.
//!
//! Y hay un desenlace que este módulo **no** fabrica: la condición de éxito de una copia. El
//! `Ok` del archivado no la da —está medido: silencia el fallo del lector y guarda la
//! instantánea sin el volcado dentro—, así que lo que sale de aquí son hechos medidos
//! ([`CopiaHecha`]) y la señal de la entrega del volcado
//! ([`EntregaDelVolcado::completa`]), y quien los compone en un veredicto es el motor.

mod error;
mod origen;
mod progreso;
mod repositorio;
mod retencion;

pub use error::Error;
pub use origen::{
    EntregaDelVolcado, LectorDeVolcado, OrigenDelVolcado, ProductorDelVolcado, TunelDelVolcado,
    tunel_del_volcado,
};
pub use progreso::{AvisoDeProgreso, BarrasDelMotor};
pub use repositorio::{Acceso, Copia, CopiaHecha, PuestaEnMarcha, Repositorio, ResumenDeSnapshot};
pub use retencion::{InformeDeRetencion, Retencion};
