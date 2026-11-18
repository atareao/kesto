//! `kesto-mcp`: el servidor MCP de Kesto.
//!
//! Es el puente entre un agente de IA y la API de Kesto, y **solo** eso: no abre la base
//! de datos, no habla con Podman ni con rustic, no decide nada. Reenvía a la API el token
//! que le llega —de la cabecera `Authorization` de la petición HTTP o del entorno— y deja
//! que la API aplique los scopes. Por eso el catálogo es corto y de solo lectura salvo las
//! dos acciones que el scope `run` autoriza: leer sí, restaurar y borrar nunca.
//!
//! El módulo [`server`] tiene las nueve herramientas, [`client`] el cliente HTTP de la
//! API, [`config`] la lectura del entorno y [`http`] el montaje del transporte HTTP. El
//! binario elige el transporte: `stdio` sin argumentos, `Streamable HTTP` cuando se le
//! pide.

pub mod client;
pub mod config;
pub mod error;
pub mod http;
pub mod server;
