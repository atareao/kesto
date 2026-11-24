//! Proceso privilegiado: el único que posee el socket del daemon de contenedores.

pub mod config;

#[cfg(feature = "motor")]
pub mod servidor;
