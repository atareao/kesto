//! El reloj del §2: que una tarea se dispare sola cuando le toca.
//!
//! Hoy `cron_expression` es una columna que se guarda, se sirve y se edita, y que **nadie
//! interpreta**: la única puerta que crea una ejecución es `POST /api/v1/jobs/:id/run`, la mano
//! del operador. Este módulo cierra esa mitad del SRS, y su reparto interno es:
//!
//! - [`validacion`], que convierte una `cron_expression` en una [`Agenda`] o en un rechazo. Es la
//!   única pieza de la **gramática** que se publica hacia fuera, porque el alta y la edición de una
//!   tarea la necesitan **antes** de escribir;
//! - [`planificacion`], que lleva el reloj nativo —un bucle de Tokio con un mapa de tareas y un
//!   despertador calculado sobre `time::OffsetDateTime`—, la tabla `job_id → mango`, la única
//!   autoridad sobre lo planificado, y la **puerta de parada**. La puerta vive aquí porque el
//!   cierre de la bandera no puede esperar candados: se cierra **en el mismo instante** en que
//!   empieza la parada, antes de cualquier `await`, y se consulta **en la misma sección crítica
//!   que el encolado**;
//! - y [`disparo`], que es el cuerpo que el cierre del reloj llama en una sola línea, y su
//!   tabla de desenlaces: seis variantes de [`crate::engine::ErrorDelMotor`] con la reacción que le
//!   toca a cada una.
//!
//! Lo que sale de este módulo es [`Planificador`], sus [`Ajustes`] y su [`ErrorDelPlanificador`],
//! además de la validación. El reloj y su tabla de mangos se quedan dentro.
//!
//! # El borde: la validación se alcanza sin arrancar el reloj
//!
//! `src/api` tiene que poder validar una expresión **sin depender del bucle de disparos**. Un borde
//! HTTP que arrancara el reloj daría a la gramática dos dueños —el que valida y el que evalúa—, y
//! el día que uno de los dos cambiara, la validación aceptaría lo que el reloj no sabe leer. Por
//! eso **ningún tipo del reloj aparece en la superficie pública** de este módulo: lo que se publica
//! es [`Agenda`], [`ErrorDeExpresion`], [`normalizar`] y [`validar`], y con eso la capa de recursos
//! valida sin saber que existe un bucle.
//!
//! # La familia de fechas de la superficie
//!
//! El reloj nativo mide el calendario con `time`, que es la familia que ya usa el §3 y el resto de
//! Kesto. La frontera de esta pieza habla **la familia del repositorio**, así que quien valida
//! desde la API no tiene que aprender otra: [`Agenda::proxima_tras`] recibe y devuelve
//! `time::OffsetDateTime`.

mod disparo;
mod planificacion;
#[cfg(test)]
mod pruebas;
mod validacion;

pub use planificacion::{
    Ajustes, ErrorDelPlanificador, PERIODO_DE_RECONCILIACION, PLAZO_DE_PARADA, Planificador,
};
pub use validacion::{Agenda, ErrorDeExpresion, normalizar, validar};
