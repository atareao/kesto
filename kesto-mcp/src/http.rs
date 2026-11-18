//! El montaje del transporte `Streamable HTTP`.
//!
//! El SDK sirve el protocolo por HTTP como un `Service` de tower, así que lo que falta es
//! un servidor HTTP: eso es `axum`, el mismo que ya usa el panel de Kesto. El servicio se
//! monta **bajo un camino** —`/mcp`— y no como comodín de la raíz: es la superficie entera
//! del transporte, y decirla en la ruta deja claro que todo lo demás no existe.
//!
//! Vive en el crate y no en el binario porque las pruebas tienen que arrancar exactamente
//! el mismo montaje que la producción: un arnés que montara su propio router probaría otro
//! servidor.

use std::sync::Arc;

use axum::Router;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use tokio_util::sync::CancellationToken;

use crate::server::KestoMcp;

/// El camino por el que se atiende el protocolo.
pub const CAMINO: &str = "/mcp";

/// La configuración del transporte HTTP.
///
/// `allowed_hosts` vacío deja los del SDK —bucle local—, que es el defecto seguro; con
/// valores se sustituyen, que es lo que necesita un despliegue publicado.
pub fn configuracion(
    cierre: CancellationToken,
    allowed_hosts: Vec<String>,
) -> StreamableHttpServerConfig {
    let config = StreamableHttpServerConfig::default().with_cancellation_token(cierre);

    if allowed_hosts.is_empty() {
        config
    } else {
        config.with_allowed_hosts(allowed_hosts)
    }
}

/// El router de `axum` con el servicio MCP montado en [`CAMINO`].
///
/// Cada sesión HTTP recibe su propia instancia del servidor —el SDK llama a la fábrica una
/// vez por sesión—, y todas comparten el mismo cliente de la API: el token no vive en el
/// servidor, viaja en cada petición.
pub fn router(servidor: KestoMcp, config: StreamableHttpServerConfig) -> Router {
    let servicio: StreamableHttpService<KestoMcp, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(servidor.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );

    Router::new().nest_service(CAMINO, servicio)
}
