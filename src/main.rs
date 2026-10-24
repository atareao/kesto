//! Kesto: orquestador ligero de copias de seguridad dentro de un contenedor.

// Contrato público del crate: el módulo raíz lo declara como su superficie.
//
// Son módulos públicos —y no privados— porque `AppError`, `ApiResponse`,
// `Config` y `SecretString` fijan el contrato de configuración y la forma del
// JSON, que es la parte cara de cambiar después. Un binario no tiene consumidor
// externo, así que lo que no sea alcanzable desde esta superficie se considera
// código muerto; declararla es lo que hace que el contrato no se lea como
// código muerto y siga delatando, en cambio, los ayudantes privados sin uso.
pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod secret;
pub mod state;

#[cfg(test)]
mod test_support;

use std::future::Future;

use crate::state::AppState;

/// Arranca el servidor sobre `state.config.bind_address` y devuelve cuando
/// `shutdown` se resuelve, dando salida antes a las peticiones en curso.
pub async fn serve(
    state: AppState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(state.config.bind_address).await?;

    axum::serve(listener, api::api_router(state))
        .with_graceful_shutdown(shutdown)
        .await
}

/// Señal de cierre del proceso: `SIGTERM` o `SIGINT`.
///
/// Si una de las dos no se puede registrar, se deja de esperar por ella en vez
/// de apagar el servidor: un fallo al escuchar una señal no es una señal.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {}
            Err(error) => {
                tracing::error!(%error, "no se pudo atender SIGINT");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "no se pudo atender SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }

    tracing::info!("señal de cierre recibida: se deja de aceptar conexiones");
}

/// Inicializa el registro con el filtro ya resuelto por la configuración.
///
/// El filtro entra por parámetro en vez de volver a leer el entorno: el
/// `EnvFilter` que se aplica y el que se vuelca en el arranque tienen que ser
/// el mismo, o el volcado miente.
///
/// Si la cadena no es un filtro válido, se cae a
/// [`crate::config::DEFAULT_LOG_FILTER`] avisando por `stderr`: un `RUST_LOG`
/// mal escrito no puede dejar el proceso sin registro.
fn init_tracing(filtro: &str) {
    let filter = match tracing_subscriber::EnvFilter::try_new(filtro) {
        Ok(filter) => filter,
        Err(error) => {
            eprintln!(
                "el filtro de registro {filtro:?} no es válido: {error}; se usa {}",
                crate::config::DEFAULT_LOG_FILTER
            );

            tracing_subscriber::EnvFilter::new(crate::config::DEFAULT_LOG_FILTER)
        }
    };

    if let Err(error) = tracing_subscriber::fmt().with_env_filter(filter).try_init() {
        eprintln!("no se pudo inicializar el registro: {error}");
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // 1. Configuración. Un fallo no arranca el proceso, y el mensaje nombra la
    //    variable ausente y nunca su valor. Sale por `stderr` porque el registro
    //    todavía no está en pie.
    let config = match config::Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("kesto no puede arrancar: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // 2. Registro, con el filtro de la configuración efectiva.
    init_tracing(&config.log_filter);

    // 3. Volcado de arranque y avisos.
    tracing::info!(config = %config.describe(), "configuración efectiva al arrancar");

    for aviso in config.warnings() {
        tracing::warn!("{aviso}");
    }

    // 4. Router, escucha y apagado. El estado se construye desde la
    //    configuración ya resuelta: en modo `Oidc` lleva el estado de
    //    autenticación, y en desarrollo no lleva ninguno.
    let address = config.bind_address;

    let state = match AppState::new(config) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("kesto no puede arrancar: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // 5. Descubrimiento del proveedor de identidad: se intenta al arrancar y,
    //    si no contesta, se avisa y se sigue. Kesto es el orquestador de las
    //    **copias**, y negarse a arrancar porque el proveedor está caído
    //    convertiría un problema de autenticación en ausencia de copias. El
    //    reintento llega con el primer login, que además cachea el resultado.
    if let Some(auth) = state.auth.as_ref()
        && let Some(aviso) = auth::descubrimiento_al_arrancar(auth).await
    {
        tracing::warn!("{aviso}");
    }

    tracing::info!(%address, "escuchando");

    if let Err(error) = serve(state, shutdown_signal()).await {
        // El registro puede estar apagado (`RUST_LOG=off`), así que el fallo
        // fatal también sale por `stderr`. Devolver el código —en vez de matar
        // el proceso— deja caer `state`, y con él los `SecretString`, que
        // borran su contenido al soltarse.
        eprintln!("el servidor se detuvo con error: {error}");
        return std::process::ExitCode::FAILURE;
    }

    std::process::ExitCode::SUCCESS
}
