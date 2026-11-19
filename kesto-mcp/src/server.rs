//! Las nueve herramientas y el servidor que las sirve.
//!
//! El catálogo es **cerrado y corto a propósito**: siete lecturas y dos acciones. No hay
//! ninguna herramienta que restaure, borre, edite o cancele, y su ausencia es una defensa
//! —un agente no puede deshacer una copia por mucho que se lo pidan—, no una tarea
//! pendiente. Ampliarlo exige un change nuevo.
//!
//! Cada herramienta es un reenvío: compone la petición, elige el token —el de la cabecera
//! `Authorization` de la petición manda sobre el del entorno— y devuelve lo que la API
//! contestó. Ni abre la base, ni habla con Podman, ni con rustic, ni decide nada por su
//! cuenta: los scopes los aplica la API.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::client::ApiClient;
use crate::config::SecretToken;
use crate::error::ApiError;

/// Lo que se le cuenta al modelo al inicializar: qué es esto y qué no puede hacer.
const INSTRUCCIONES: &str = "Servidor MCP de Kesto, el orquestador de copias. Consulta el \
estado de los repositorios, los targets, las tareas y el histórico de ejecuciones, lanza una \
copia cuando toca y comprueba la salud de un repositorio. No restaura instantáneas, no borra \
recursos y no edita nada: este servidor no expone esas operaciones.";

/// El servidor MCP de Kesto.
#[derive(Debug, Clone)]
pub struct KestoMcp {
    client: ApiClient,
    tool_router: ToolRouter<Self>,
}

impl KestoMcp {
    /// Construye el servidor sobre un cliente de la API ya configurado.
    pub fn new(client: ApiClient) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }

    /// Los nombres del catálogo, tal y como se anuncian.
    ///
    /// Existe para que la puerta de calidad pueda fijar el catálogo —exactamente nueve, y
    /// ninguna destructiva— sin hablar el protocolo.
    pub fn nombres_del_catalogo() -> Vec<String> {
        Self::tool_router()
            .list_all()
            .iter()
            .map(|herramienta| herramienta.name.to_string())
            .collect()
    }

    /// El token de la petición: la cabecera `Authorization: Bearer` gana al entorno.
    ///
    /// En `stdio` no hay cabecera —no hay petición HTTP—, así que esto devuelve `None` y
    /// decide el token del entorno. Una cabecera que no sea un `Bearer` con valor se
    /// ignora en vez de rechazar la llamada: el rechazo por credencial lo da el token
    /// elegido cuando la API lo mira.
    fn token_de_la_peticion(ctx: &RequestContext<RoleServer>) -> Option<SecretToken> {
        let partes = ctx.extensions.get::<http::request::Parts>()?;
        let cabecera = partes
            .headers
            .get(http::header::AUTHORIZATION)?
            .to_str()
            .ok()?;
        let (esquema, valor) = cabecera.split_once(' ')?;

        if !esquema.eq_ignore_ascii_case("bearer") || valor.trim().is_empty() {
            return None;
        }

        Some(SecretToken::new(valor.trim().to_owned()))
    }

    /// `GET` a la API y traducción a resultado de herramienta.
    async fn leer(
        &self,
        ctx: &RequestContext<RoleServer>,
        ruta: &str,
        consulta: &[(&str, String)],
    ) -> CallToolResult {
        let token = Self::token_de_la_peticion(ctx);

        // Lo que se registra es la ruta, nunca el token ni los argumentos: el registro
        // acaba en un fichero de `journald` o en el visor de un cliente MCP, y ninguno de
        // los dos sitios es lugar para una credencial.
        tracing::debug!(ruta, "consultando la API de Kesto");

        let resultado = self.client.get(ruta, consulta, token.as_ref()).await;

        traducir(resultado)
    }

    /// El `GET` de una ruta pública, que no exige credencial.
    ///
    /// Solo lo usa `get_health`: `/api/v1/health` contesta aunque no haya token —es lo que
    /// hace en modo `disabled`—, y sin esto la herramienta no podría ni decir en qué modo
    /// está la API.
    async fn leer_sin_credencial(&self, ctx: &RequestContext<RoleServer>) -> CallToolResult {
        let ruta = "/api/v1/health";
        let token = Self::token_de_la_peticion(ctx);

        tracing::debug!(ruta, "consultando la API de Kesto");

        let resultado = self
            .client
            .get_sin_exigir_token(ruta, &[], token.as_ref())
            .await;

        traducir(resultado)
    }

    /// `POST` a la API y traducción a resultado de herramienta.
    async fn actuar(&self, ctx: &RequestContext<RoleServer>, ruta: &str) -> CallToolResult {
        let token = Self::token_de_la_peticion(ctx);

        tracing::debug!(ruta, "actuando sobre la API de Kesto");

        let resultado = self.client.post(ruta, token.as_ref()).await;

        traducir(resultado)
    }
}

/// Traduce el desenlace de una llamada a la API a un resultado de herramienta.
fn traducir(resultado: Result<Value, ApiError>) -> CallToolResult {
    match resultado {
        Ok(valor) => CallToolResult::structured(valor),
        Err(error) => error_de_herramienta(&error),
    }
}

#[tool_router]
impl KestoMcp {
    /// Lista los repositorios de copia con su destino y su salud.
    #[tool(
        name = "list_repositories",
        description = "Lista los repositorios de copia dados de alta, con su destino y su \
                       salud. No modifica nada."
    )]
    async fn list_repositories(&self, ctx: RequestContext<RoleServer>) -> CallToolResult {
        self.leer(&ctx, "/api/v1/repositories", &[]).await
    }

    /// Lista los targets: de dónde se extrae.
    #[tool(
        name = "list_targets",
        description = "Lista los targets de los que se extrae un volcado —una base de datos o \
                       un directorio—, con su tipo y su configuración pública."
    )]
    async fn list_targets(&self, ctx: RequestContext<RoleServer>) -> CallToolResult {
        self.leer(&ctx, "/api/v1/targets", &[]).await
    }

    /// Lista las tareas programadas.
    #[tool(
        name = "list_jobs",
        description = "Lista las tareas de copia programadas, con su expresión cron, su \
                       retención y si están activas."
    )]
    async fn list_jobs(&self, ctx: RequestContext<RoleServer>) -> CallToolResult {
        self.leer(&ctx, "/api/v1/jobs", &[]).await
    }

    /// Lista las instantáneas de un repositorio.
    #[tool(
        name = "list_snapshots",
        description = "Lista las instantáneas de un repositorio, con su identificador y su \
                       fecha. No restaura ninguna."
    )]
    async fn list_snapshots(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(peticion): Parameters<PorRepositorio>,
    ) -> CallToolResult {
        let consulta = [("repository_id", peticion.repository_id)];

        self.leer(&ctx, "/api/v1/snapshots", &consulta).await
    }

    /// El histórico de ejecuciones, con sus filtros.
    #[tool(
        name = "list_executions",
        description = "Devuelve el histórico de ejecuciones con sus métricas, filtrado por \
                       tarea, por estado y por límite."
    )]
    async fn list_executions(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(filtro): Parameters<FiltroDeEjecuciones>,
    ) -> CallToolResult {
        let mut consulta: Vec<(&str, String)> = Vec::new();

        if let Some(job_id) = filtro.job_id {
            consulta.push(("job_id", job_id));
        }
        if let Some(status) = filtro.status {
            consulta.push(("status", status));
        }
        if let Some(limit) = filtro.limit {
            consulta.push(("limit", limit.to_string()));
        }
        if let Some(offset) = filtro.offset {
            consulta.push(("offset", offset.to_string()));
        }

        self.leer(&ctx, "/api/v1/executions", &consulta).await
    }

    /// Una ejecución concreta por su identificador.
    #[tool(
        name = "get_execution",
        description = "Devuelve una ejecución concreta, con sus métricas y su estado, a \
                       partir de su identificador."
    )]
    async fn get_execution(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(peticion): Parameters<PorIdentificador>,
    ) -> CallToolResult {
        let consulta = [("id", peticion.id)];

        let resultado = self.leer(&ctx, "/api/v1/executions", &consulta).await;

        // La API no tiene `GET /executions/{id}`: el histórico se filtra por `id`, y un
        // identificador que no está devuelve una envoltura con `executions` **vacío**, no un
        // `404`. Devolverla tal cual haría creer al agente que la ejecución existe y no tiene
        // filas, así que aquí se convierte en un error de herramienta con el identificador
        // nombrado.
        if resultado.is_error != Some(true) && filas_vacias(&resultado) {
            let error = ApiError::RespuestaDeLaApi {
                status: 404,
                mensaje: "no hay ninguna ejecución con ese identificador".to_owned(),
            };

            return error_de_herramienta(&error);
        }

        resultado
    }

    /// La salud del servicio y su modo de autenticación.
    #[tool(
        name = "get_health",
        description = "Devuelve la salud del servicio de Kesto y su modo de autenticación."
    )]
    async fn get_health(&self, ctx: RequestContext<RoleServer>) -> CallToolResult {
        self.leer_sin_credencial(&ctx).await
    }

    /// Lanza una copia de una tarea ahora mismo.
    #[tool(
        name = "run_backup",
        description = "Lanza una copia de una tarea programada ahora mismo. Exige un token \
                       con scope `run`. Devuelve la ejecución creada."
    )]
    async fn run_backup(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(peticion): Parameters<PorIdentificador>,
    ) -> CallToolResult {
        let ruta = format!("/api/v1/jobs/{}/run", segmento_de_camino(&peticion.id));

        self.actuar(&ctx, &ruta).await
    }

    /// Comprueba la salud de un repositorio y deja el resultado persistido.
    #[tool(
        name = "check_repository_health",
        description = "Comprueba que un repositorio responde y deja el resultado persistido. \
                       Exige un token con scope `run`."
    )]
    async fn check_repository_health(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(peticion): Parameters<PorRepositorio>,
    ) -> CallToolResult {
        let ruta = format!(
            "/api/v1/repositories/{}/health",
            segmento_de_camino(&peticion.repository_id)
        );

        self.actuar(&ctx, &ruta).await
    }
}

// `get_info` se escribe a mano en vez de dejárselo al macro: el macro acepta el nombre y
// las instrucciones solo como literales dentro del atributo, y ahí el `INSTRUCCIONES` de
// arriba no entra —habría que duplicarlo—. El macro respeta un `get_info` ya escrito.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for KestoMcp {
    /// El servidor se presenta con su nombre, sus instrucciones y las herramientas, y con
    /// nada más: no sirve prompts ni recursos, y declararlos sin tenerlos sería mentir al
    /// cliente.
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("kesto-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCCIONES)
    }
}

/// Los parámetros de una herramienta que actúa sobre un recurso por su identificador.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PorIdentificador {
    /// El identificador del recurso, en su forma canónica con guiones.
    pub id: String,
}

/// Los parámetros de una herramienta que actúa sobre un repositorio.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PorRepositorio {
    /// El identificador del repositorio, en su forma canónica con guiones.
    pub repository_id: String,
}

/// Los filtros del histórico de ejecuciones, todos opcionales.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FiltroDeEjecuciones {
    /// La tarea por la que filtrar.
    pub job_id: Option<String>,
    /// El estado por el que filtrar.
    pub status: Option<String>,
    /// Cuántas filas devolver como mucho. La API tiene su propio tope.
    pub limit: Option<i64>,
    /// Cuántas filas saltar.
    pub offset: Option<i64>,
}

/// Traduce el fallo de la API a un error de herramienta **estructurado**.
///
/// Va por `CallToolResult::structured_error` y no por `Err(ErrorData)` a propósito: el
/// primero es un error de herramienta —la llamada se atendió y falló—, que el cliente
/// enseña al modelo con el mensaje intacto, y el segundo es un error de protocolo, que el
/// cliente suele tapar. Y va con `error` y `status` además del `message` para que un agente
/// pueda decidir sin analizar texto.
fn error_de_herramienta(error: &ApiError) -> CallToolResult {
    CallToolResult::structured_error(serde_json::json!({
        "ok": false,
        "error": error.clase(),
        "status": error.status_http(),
        "message": error.to_string(),
    }))
}

/// ¿La respuesta es la **envoltura del histórico** con las ejecuciones vacías?
///
/// Desde que el listado dejó de ser un array pelado, `data` es `{executions, total}`: «vacío» es que
/// `executions` sea una lista vacía. Un cuerpo sin esa clave —otro recurso, o un error— **no** cuenta
/// como vacío: convertir eso en «esa ejecución no existe» mandaría a buscar el fallo donde no está.
fn filas_vacias(resultado: &CallToolResult) -> bool {
    resultado
        .structured_content
        .as_ref()
        .and_then(|cuerpo| cuerpo.get("data"))
        .and_then(|data| data.get("executions"))
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
}

/// Codifica un segmento de camino.
///
/// Los identificadores son UUIDs y no necesitan nada de esto, pero una herramienta no elige
/// lo que le mandan: un valor con una barra dentro cambiaría la ruta a la que se llama y
/// apuntaría a otro recurso sin que nada lo delate. Se codifica todo lo que no sea
/// inequívocamente seguro, y así el valor viaja entero y el camino no se rompe.
fn segmento_de_camino(valor: &str) -> String {
    let mut salida = String::with_capacity(valor.len());

    for byte in valor.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                salida.push(char::from(byte));
            }
            otro => salida.push_str(&format!("%{otro:02X}")),
        }
    }

    salida
}

#[cfg(test)]
mod tests {
    use super::{KestoMcp, filas_vacias, segmento_de_camino};
    use rmcp::model::CallToolResult;

    #[test]
    fn el_catalogo_tiene_exactamente_nueve_herramientas() {
        let nombres = KestoMcp::nombres_del_catalogo();

        assert_eq!(nombres.len(), 9, "el catálogo es de nueve: {nombres:?}");
    }

    #[test]
    fn el_catalogo_no_tiene_herramientas_destructivas() {
        let nombres = KestoMcp::nombres_del_catalogo();

        for prohibida in [
            "restore_snapshot",
            "restore",
            "delete_repository",
            "delete_job",
            "delete_target",
            "update_repository",
            "cancel_execution",
        ] {
            assert!(
                !nombres.iter().any(|nombre| nombre == prohibida),
                "el catálogo no puede incluir {prohibida}: {nombres:?}"
            );
        }
    }

    #[test]
    fn un_segmento_de_camino_no_deja_escapar_barras() {
        assert_eq!(
            segmento_de_camino("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        );
        assert_eq!(segmento_de_camino("a/b"), "a%2Fb");
        assert_eq!(segmento_de_camino("a b"), "a%20b");
    }

    #[test]
    fn una_lista_de_ejecuciones_vacia_se_reconoce() {
        let vacia =
            CallToolResult::structured(serde_json::json!({"data": {"executions": [], "total": 0}}));
        let con_filas = CallToolResult::structured(
            serde_json::json!({"data": {"executions": [{"id": "x"}], "total": 1}}),
        );
        let sin_datos = CallToolResult::structured(serde_json::json!({"message": "hola"}));
        let array_pelado = CallToolResult::structured(serde_json::json!({"data": []}));

        assert!(filas_vacias(&vacia));
        assert!(!filas_vacias(&con_filas));
        assert!(!filas_vacias(&sin_datos));
        assert!(
            !filas_vacias(&array_pelado),
            "el histórico ya no viaja como array pelado: un `data` que no sea la envoltura no es vacío"
        );
    }
}
