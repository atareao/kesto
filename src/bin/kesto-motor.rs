//! Binario del motor de contenedores: escucha en el socket IPC y atiende a la API.
//!
//! El crate raíz es un binario y no expone una biblioteca, así que este segundo
//! binario incluye directamente los tres módulos que necesita: el transporte
//! del daemon, el contrato IPC y su propia configuración. La API se compila sin
//! la feature `motor` y, por tanto, sin el cliente `bollard`.
//!
//! La superficie compartida se incluye entera: este binario solo ejercita una
//! parte de ella, y el resto no es código muerto en la API.
#![allow(dead_code)]

#[path = "../docker/mod.rs"]
mod docker;
#[path = "../engine/frontera.rs"]
mod frontera;
#[path = "../ipc/mod.rs"]
mod ipc;
#[path = "../motor/mod.rs"]
mod motor;

use motor::config::AjustesMotor;
use motor::servidor::servir;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let ajustes = match AjustesMotor::desde_env() {
        Ok(ajustes) => ajustes,
        Err(error) => {
            eprintln!("kesto-motor no puede arrancar: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    match servir(ajustes).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kesto-motor se ha detenido: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
