//! Canal IPC entre la API sin privilegios y el daemon que posee el socket Docker.

pub mod mensaje;

// Fachada del contrato: no todos los consumidores usan todos los símbolos.
#[allow(unused_imports)]
pub use mensaje::{
    CanalIpc, ClaseFallo, Error, OrdenIpc, RespuestaMotor, SolicitudMotor, TOPE_CUERPO, conectar,
    enviar, escribir, leer, recibir, recibir_solicitud,
};
