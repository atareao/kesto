//! El reloj del §2: que una tarea se dispare sola cuando le toca.
//!
//! Hoy `cron_expression` es una columna que se guarda, se sirve y se edita, y que **nadie
//! interpreta**: la única puerta que crea una ejecución es `POST /api/v1/jobs/:id/run`, la mano
//! del operador. Este módulo cierra esa mitad del SRS, y su reparto interno es:
//!
//! - [`validacion`], que convierte una `cron_expression` en una [`Agenda`] o en un rechazo. Es la
//!   única pieza de la **gramática** que se publica hacia fuera, porque el alta y la edición de una
//!   tarea la necesitan **antes** de escribir;
//! - [`planificacion`], que tiene el reloj de la biblioteca, la tabla `job_id → mango` —la única
//!   autoridad sobre lo planificado, porque la biblioteca no ofrece ninguna señal de estado— y la
//!   **puerta de parada**. La puerta vive aquí y no en la biblioteca por una medición: su
//!   `shutdown()` vuelve en microsegundos y deja un tick disparando después, así que la garantía de
//!   «no se encola tras la parada» es del módulo, cerrada **en la misma sección crítica que el
//!   encolado**;
//! - y [`disparo`], que es el cuerpo que el cierre de la biblioteca llama en una sola línea, y su
//!   tabla de desenlaces: seis variantes de [`crate::engine::ErrorDelMotor`] con la reacción que le
//!   toca a cada una.
//!
//! Lo que sale de este módulo es [`Planificador`], sus [`Ajustes`] y su [`ErrorDelPlanificador`],
//! además de la validación. Ninguno de los tres nombres es de `tokio_cron_scheduler`: el reloj de la
//! biblioteca se queda dentro, como la tabla de mangos.
//!
//! # El borde: la validación se alcanza sin nombrar la biblioteca del reloj
//!
//! `src/api` tiene que poder validar una expresión **sin importar `tokio_cron_scheduler`**. Un borde
//! HTTP que nombrara la biblioteca daría a la gramática dos dueños —el que valida y el que
//! evalúa—, y el día que uno de los dos cambiara, la validación aceptaría lo que el reloj no sabe
//! leer. Por eso **ningún tipo de `tokio_cron_scheduler` aparece en la superficie pública** de este
//! módulo: lo que se publica es [`Agenda`], [`ErrorDeExpresion`], [`normalizar`] y [`validar`], y
//! con eso la capa de recursos valida sin saber que existe un reloj.
//!
//! # La familia de fechas de la superficie
//!
//! `croner` y la biblioteca del reloj miden el calendario con `chrono`; el §3 y el resto de Kesto lo
//! miden con `time`. La frontera de esta pieza habla **la familia del repositorio** —`time`, que es
//! una dependencia directa y la que el resto del programa ya usa—, así que quien valida desde la API
//! no tiene que aprender una segunda familia para llamar a [`validar`]: [`Agenda::proxima_tras`]
//! recibe y devuelve `time::OffsetDateTime`, y ninguna firma pública de este módulo nombra una fecha
//! de `chrono`.

mod disparo;
mod planificacion;
#[cfg(test)]
mod pruebas;
mod validacion;

pub use planificacion::{
    Ajustes, ErrorDelPlanificador, PERIODO_DE_RECONCILIACION, PLAZO_DE_PARADA, Planificador,
};
pub use validacion::{Agenda, ErrorDeExpresion, normalizar, validar};
