//! Dominio de autenticación: la sesión del operador y, más adelante, el flujo OIDC.
//!
//! `src/auth/` no es un módulo del §7 del SRS: lo declara `main.rs` como los
//! demás. Su frontera con `src/api/` es la de siempre —aquí el flujo y la
//! sesión, allí el HTTP—: `src/api/auth.rs` traducirá peticiones a llamadas de
//! este módulo, y no al revés.

pub mod middleware;
pub mod session;
