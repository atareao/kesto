//! Estado compartido de la aplicación.

use std::sync::Arc;

use crate::config::Config;

/// Estado que comparten los manejadores de la API.
#[derive(Clone)]
pub struct AppState {
    /// Configuración efectiva, cargada una sola vez al arrancar.
    pub config: Arc<Config>,
}
