//! La batería de punta a punta de copia y restauración (issue #86).
//!
//! Ejecútala con:
//!
//! ```text
//! DOCKER_HOST=unix:///run/user/1000/podman/podman.sock \
//!   cargo test --features e2e --test e2e -- --test-threads=1
//! ```
//!
//! o, en local, con la receta `just e2e`, que es la que exporta `DOCKER_HOST` por ti.
//!
//! # Por qué `--test-threads=1`
//!
//! Cada escenario levanta su propio servidor S3, su propio binario y sus propios
//! contenedores: son mundos independientes y podrían correr a la vez, pero un motor que
//! arranca cuatro bases de datos en paralelo mide la máquina, no el motor de copias. En
//! serie, un fallo se lee entero.
//!
//! # Por qué está detrás de una feature
//!
//! Porque no puede correr donde no hay motor de contenedores. `required-features = ["e2e"]`
//! en `Cargo.toml` deja `cargo test --locked` —lo que corre el job `backend` del CI— sin
//! compilar ni ejecutar nada de aquí.
//!
//! # Dónde está cada cosa
//!
//! - [`arnes`]: el motor de contenedores, el servidor S3, el binario y las comparaciones.
//! - [`escenarios`]: los treinta y tres escenarios de `verificacion-e2e`, uno por prueba.

mod arnes;
mod escenarios;
