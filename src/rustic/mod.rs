//! El trato con Rustic: poner el repositorio en marcha, meter el volcado dentro y retener.
//!
//! Este módulo es la frontera entre el motor de copias y la biblioteca que las escribe, y
//! existe porque las dos partes no hablan el mismo idioma. La extracción produce bytes
//! **asíncronos y a trozos** —el `stream` de un `exec` de la API de Docker—, mientras que
//! la biblioteca pide un origen de lectura **bloqueante**: [`rustic_core::ReadSource`]. Las
//! cuatro piezas que cruzan esa frontera son el puente ([`origen`]), el gancho de progreso
//! ([`progreso`]), el repositorio ([`repositorio`]) y la retención ([`retencion`]).
//!
//! Nada que venga de un **volcado** pasa por el disco: los bytes van del `stream` del
//! `exec` al árbol de la instantánea sin fichero temporal, y nada que salga de aquí lleva
//! datos del volcado: ni un [`std::fmt::Debug`], ni un registro. Lo que sí lee del sistema
//! de ficheros es [`Repositorio::archivar_directorio`], y es su cometido —el target de tipo
//! `directory` va contra un directorio del host, no contra un contenedor—: la promesa es de
//! los volcados, no del módulo entero.
//!
//! Lo que este módulo rechaza por su cuenta es lo que la biblioteca no puede validar —un
//! nombre de volcado que no sirve para nombrar nada, un nombre de volcado que no es el que
//! la copia declara, una copia sin rutas, una fila que no describe un destino utilizable— y
//! los desenlaces que la biblioteca no distingue, como cuál de los tres le ocurrió a la
//! puesta en marcha de un repositorio. Lo que ya está en manos de la biblioteca no se puede
//! validar desde aquí, y sus pánicos son suyos.
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
    AvanceDelVolcado, EntregaDelVolcado, LectorDeVolcado, OrigenDelVolcado, ProductorDelVolcado,
    TunelDelVolcado, tunel_del_volcado,
};
pub use progreso::{AvisoDeProgreso, BarrasDelMotor};
pub use repositorio::{Acceso, Copia, CopiaHecha, PuestaEnMarcha, Repositorio, ResumenDeSnapshot};
pub use retencion::{InformeDeRetencion, Retencion};

// El gancho por el que las pruebas del motor abren un repositorio **local** en vez de uno de S3.
// Se reexporta y no se usa desde aquí para nada: el módulo `repositorio` es privado, y el arnés de
// pruebas —y con él el motor— viven fuera. Va detrás de `cfg(test)` a propósito: en un binario de
// producción nadie abre un repositorio local, y un reexport que nadie usa sería un aviso de import
// sin uso.
#[cfg(test)]
pub(crate) use repositorio::poner_en_marcha_con;
