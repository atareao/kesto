//! Arnés común de las pruebas de `kesto-mcp`.
//!
//! Levanta dos servidores de verdad: la **API falsa de Kesto** —`wiremock`, que es quien
//! dice qué llegó por el cable— y el **transporte HTTP** del servidor MCP, montado con el
//! mismo [`http::router`](kesto_mcp::http::router) que usa el binario. Hablar el protocolo
//! a pelo, en vez de llamar a las herramientas por dentro, es lo que hace que estas pruebas
//! prueben el servidor que se despliega: catálogo, cabeceras de sesión y errores incluidos.

#![allow(dead_code)]

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use kesto_mcp::client::ApiClient;
use kesto_mcp::config::Config;
use kesto_mcp::http;
use kesto_mcp::server::KestoMcp;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// El token del entorno que usan casi todas las pruebas.
pub const TOKEN: &str = "token-de-entorno-aaaaaaaaaaaaaaaaaaaaaaaaaa";

/// Un segundo token, con scope `run`, que solo aparece en las cabeceras.
pub const TOKEN_RUN: &str = "token-de-cabecera-con-scope-run-aaaaaaaaaa";

/// Un identificador de repositorio con forma de UUID.
pub const REPOSITORIO: &str = "11111111-1111-1111-1111-111111111111";

/// Un identificador de tarea con forma de UUID.
pub const TAREA: &str = "22222222-2222-2222-2222-222222222222";

/// Un identificador de ejecución con forma de UUID.
pub const EJECUCION: &str = "33333333-3333-3333-3333-333333333333";

/// La lectura del entorno que [`Config::from_lookup`] espera.
pub fn lectura(url: &str, token: Option<&str>) -> impl Fn(&str) -> Option<String> + use<> {
    let url = url.to_owned();
    let token = token.map(str::to_owned);

    move |nombre: &str| match nombre {
        "KESTO_URL" => Some(url.clone()),
        "KESTO_API_TOKEN" => token.clone(),
        _ => None,
    }
}

/// La envoltura de la API: es la forma que publica el §5 y la que las herramientas
/// reenvían tal cual.
pub fn envoltura(data: impl Into<Value>) -> Value {
    json!({"status": 200, "message": "atendida", "data": data.into()})
}

/// Un error de la API, con la envoltura de error que publica el §5.
pub fn error_de_la_api(status: u16, mensaje: &str) -> Value {
    json!({"status": status, "message": mensaje})
}

/// Monta el catálogo completo de la API falsa, con respuestas felices.
pub async fn api_con_todo(mock: &MockServer) {
    montar_lectura(
        mock,
        "/api/v1/repositories",
        vec![
            json!({"id": REPOSITORIO, "name": "repos-uno", "health": "healthy"}),
            json!({"id": "44444444-4444-4444-4444-444444444444", "name": "repos-dos", "health": "unknown"}),
        ],
    )
    .await;

    montar_lectura(
        mock,
        "/api/v1/targets",
        vec![json!({"id": TAREA, "name": "target-uno", "kind": "postgresql"})],
    )
    .await;

    montar_lectura(
        mock,
        "/api/v1/jobs",
        vec![json!({"id": TAREA, "name": "tarea-uno", "cron_expression": "0 3 * * *", "enabled": true})],
    )
    .await;

    montar_lectura(
        mock,
        "/api/v1/health",
        json!({"version": "0.12.0", "auth_mode": "oidc"}),
    )
    .await;
}

/// Monta un `GET` que devuelve `data`.
pub async fn montar_lectura(mock: &MockServer, ruta: &str, data: impl Into<Value>) {
    Mock::given(method("GET"))
        .and(path(ruta))
        .respond_with(ResponseTemplate::new(200).set_body_json(envoltura(data.into())))
        .mount(mock)
        .await;
}

/// Monta un `POST` que devuelve `data` con el código pedido.
pub async fn montar_accion(mock: &MockServer, ruta: &str, status: u16, data: Value) {
    let plantilla = if status < 400 {
        ResponseTemplate::new(status).set_body_json(envoltura(data))
    } else {
        ResponseTemplate::new(status).set_body_json(json!({
            "status": status,
            "message": "la API rechaza la operación",
        }))
    };

    Mock::given(method("POST"))
        .and(path(ruta))
        .respond_with(plantilla)
        .mount(mock)
        .await;
}

/// Monta una respuesta de error para un `GET`.
pub async fn montar_error_lectura(mock: &MockServer, ruta: &str, status: u16) {
    Mock::given(method("GET"))
        .and(path(ruta))
        .respond_with(
            ResponseTemplate::new(status)
                .set_body_json(error_de_la_api(status, "eso no está autorizado")),
        )
        .mount(mock)
        .await;
}

/// Una sesión MCP abierta contra el transporte HTTP del servidor.
pub struct Sesion {
    url: String,
    http: Client,
    sesion: Option<String>,
}

impl Sesion {
    /// Arranca el transporte HTTP con la configuración pedida y hace el saludo.
    pub async fn abrir(url_kesto: &str, token: Option<&str>) -> Self {
        let config =
            Config::from_lookup(&lectura(url_kesto, token)).expect("la configuración es válida");
        let servidor = KestoMcp::new(ApiClient::new(&config).expect("el cliente se construye"));

        Self::con_servidor(servidor).await
    }

    /// Arranca el transporte HTTP con un servidor ya montado.
    pub async fn con_servidor(servidor: KestoMcp) -> Self {
        let router = http::router(
            servidor,
            http::configuracion(CancellationToken::new(), Vec::new()),
        );
        let escucha = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("el puerto efímero se abre");
        let direccion = escucha.local_addr().expect("la dirección se lee");

        tokio::spawn(async move {
            let _ = axum::serve(escucha, router).await;
        });

        Self::conectar(format!("http://{direccion}{}", http::CAMINO)).await
    }

    /// Se conecta a un servidor MCP que ya está en marcha en esa URL.
    ///
    /// Es lo que usan las pruebas del binario: ahí el servidor lo arranca el sistema
    /// operativo —un proceso de verdad—, no este arnés.
    pub async fn conectar(url: String) -> Self {
        let mut sesion = Self {
            url,
            http: Client::new(),
            sesion: None,
        };

        sesion.saludar().await;

        sesion
    }

    /// El saludo del protocolo: `initialize` y su notificación de cortesía.
    async fn saludar(&mut self) {
        let respuesta = self
            .enviar(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-03-26",
                        "capabilities": {},
                        "clientInfo": {"name": "pruebas", "version": "1"}
                    }
                }),
                None,
            )
            .await;

        assert_eq!(
            respuesta.0,
            StatusCode::OK,
            "el saludo tiene que atenderse: {:?}",
            respuesta.2
        );
        assert_eq!(
            respuesta.2["result"]["serverInfo"]["name"], "kesto-mcp",
            "el servidor se presenta con su nombre: {:?}",
            respuesta.2
        );

        self.sesion = respuesta.1;

        let _ = self
            .enviar(
                json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
                None,
            )
            .await;
    }

    /// El catálogo anunciado.
    pub async fn herramientas(&self) -> Vec<Value> {
        let (_, _, cuerpo) = self
            .enviar(
                json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
                None,
            )
            .await;

        cuerpo["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("el catálogo tiene que ser una lista: {cuerpo}"))
            .clone()
    }

    /// Los nombres del catálogo anunciado.
    pub async fn nombres(&self) -> Vec<String> {
        self.herramientas()
            .await
            .iter()
            .map(|herramienta| {
                herramienta["name"]
                    .as_str()
                    .expect("toda herramienta tiene nombre")
                    .to_owned()
            })
            .collect()
    }

    /// Invoca una herramienta y devuelve el mensaje JSON-RPC entero.
    pub async fn llamar(&self, nombre: &str, argumentos: Value, token: Option<&str>) -> Value {
        let (_, _, cuerpo) = self
            .enviar(
                json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {"name": nombre, "arguments": argumentos}
                }),
                token,
            )
            .await;

        cuerpo
    }

    /// Envía un mensaje y devuelve el código, la sesión que asignó el servidor y el cuerpo.
    async fn enviar(
        &self,
        mensaje: Value,
        token: Option<&str>,
    ) -> (StatusCode, Option<String>, Value) {
        let mut peticion = self
            .http
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(mensaje.to_string());

        if let Some(token) = token {
            peticion = peticion.header("Authorization", format!("Bearer {token}"));
        }
        if let Some(sesion) = &self.sesion {
            peticion = peticion.header("mcp-session-id", sesion);
        }

        let respuesta = peticion.send().await.expect("el servidor MCP responde");
        let status = respuesta.status();
        let sesion = respuesta
            .headers()
            .get("mcp-session-id")
            .and_then(|valor| valor.to_str().ok())
            .map(str::to_owned);
        let texto = respuesta.text().await.expect("el cuerpo se lee");

        (status, sesion, parsear(&texto))
    }
}

/// Saca el mensaje JSON-RPC de un cuerpo, que puede venir en JSON o en un flujo SSE.
///
/// En `Streamable HTTP` la respuesta va por `text/event-stream` cuando la sesión es la del
/// protocolo antiguo, y el último `data:` del flujo es el mensaje final: los anteriores son
/// notificaciones.
fn parsear(texto: &str) -> Value {
    if let Ok(valor) = serde_json::from_str::<Value>(texto) {
        return valor;
    }

    let mut ultimo = Value::Null;

    for linea in texto.lines() {
        if let Some(datos) = linea.strip_prefix("data:")
            && let Ok(valor) = serde_json::from_str::<Value>(datos.trim())
        {
            ultimo = valor;
        }
    }

    ultimo
}

/// El `result` de un mensaje JSON-RPC, con un mensaje claro si lo que llegó fue un error.
pub fn resultado(mensaje: &Value) -> &Value {
    assert!(
        mensaje.get("error").is_none(),
        "la herramienta devolvió un error de protocolo en vez de un resultado: {mensaje}"
    );

    mensaje
        .get("result")
        .unwrap_or_else(|| panic!("la herramienta no devolvió resultado: {mensaje}"))
}

/// ¿El resultado es un error de herramienta?
pub fn es_error(mensaje: &Value) -> bool {
    mensaje["result"]["isError"] == Value::Bool(true)
}

/// El texto que el agente va a leer: el `message` estructurado del error, o el contenido.
pub fn mensaje_de(mensaje: &Value) -> String {
    if let Some(texto) = mensaje["result"]["structuredContent"]["message"].as_str() {
        return texto.to_owned();
    }

    mensaje["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Los datos estructurados de un resultado atendido.
pub fn datos(mensaje: &Value) -> &Value {
    assert!(
        !es_error(mensaje),
        "se esperaba un resultado atendido: {mensaje}"
    );

    &mensaje["result"]["structuredContent"]["data"]
}

/// Las peticiones que la API falsa recibió, como `(método, camino, cabecera Authorization)`.
pub async fn recibidas(mock: &MockServer) -> Vec<(String, String, Option<String>)> {
    mock.received_requests()
        .await
        .expect("las peticiones se registran")
        .iter()
        .map(|peticion| {
            (
                peticion.method.to_string(),
                peticion.url.path().to_owned(),
                peticion
                    .headers
                    .get("authorization")
                    .and_then(|valor| valor.to_str().ok().map(str::to_owned)),
            )
        })
        .collect()
}

/// Los parámetros de consulta que la API falsa recibió, como cadenas.
pub async fn consultas(mock: &MockServer) -> Vec<String> {
    mock.received_requests()
        .await
        .expect("las peticiones se registran")
        .iter()
        .filter_map(|peticion| peticion.url.query().map(str::to_owned))
        .collect()
}

/// Monta un `GET` que solo contesta cuando el camino y el parámetro de consulta coinciden.
pub async fn montar_lectura_con_parametro(
    mock: &MockServer,
    ruta: &str,
    parametro: &str,
    valor: &str,
    data: impl Into<Value>,
) {
    Mock::given(method("GET"))
        .and(path(ruta))
        .and(query_param(parametro, valor))
        .respond_with(ResponseTemplate::new(200).set_body_json(envoltura(data.into())))
        .mount(mock)
        .await;
}

/// Saca la dirección de escucha del anuncio que el binario escribe en su registro.
pub fn extraer_direccion(texto: &str) -> Option<String> {
    let inicio = texto.find("http://")? + "http://".len();
    let resto = &texto[inicio..];
    let fin = resto.find('/').unwrap_or(resto.len());

    Some(resto[..fin].trim().to_owned())
}

/// El registro que un servidor escribe por `stderr`, con aviso de cada línea nueva.
///
/// Con aviso y no con sondeo: el servidor tarda lo que tarde, y preguntar cada cien
/// milisegundos solo hace la prueba más lenta y más frágil.
struct Registro {
    texto: Mutex<String>,
    aviso: watch::Sender<u64>,
}

impl Registro {
    fn nuevo() -> Arc<Self> {
        Arc::new(Self {
            texto: Mutex::new(String::new()),
            aviso: watch::channel(0).0,
        })
    }

    /// Lo que el servidor lleva escrito hasta ahora.
    async fn texto(&self) -> String {
        self.texto.lock().await.clone()
    }

    /// Espera a que el registro contenga lo que `encontrar` busque, y lo devuelve.
    async fn esperar(&self, encontrar: impl Fn(&str) -> Option<String>) -> String {
        // Se escucha **antes** de mirar: al revés, una línea escrita entre la mirada y la
        // espera se perdería y la prueba se quedaría colgada por un aviso que ya pasó.
        let mut receptor = self.aviso.subscribe();

        let plazo = tokio::time::Instant::now() + Duration::from_secs(30);

        loop {
            if let Some(encontrado) = encontrar(&self.texto().await) {
                return encontrado;
            }

            let restante = plazo.saturating_duration_since(tokio::time::Instant::now());

            if tokio::time::timeout(restante, receptor.changed())
                .await
                .is_err()
            {
                panic!(
                    "el registro nunca dijo lo que se esperaba; esto es lo que dijo: {}",
                    self.texto().await
                );
            }
        }
    }
}

/// Un `kesto-mcp` de verdad, arrancado como proceso hijo.
///
/// Las pruebas de transporte no montan el servidor por dentro: arrancan el binario, que es
/// el que se despliega, y le hablan el protocolo por la entrada y la salida estándar. Todo
/// lo que el servidor escribe por `stderr` queda guardado para poder mirarlo.
pub struct Proceso {
    hijo: Child,
    entrada: ChildStdin,
    salida: Lines<BufReader<ChildStdout>>,
    registro: Arc<Registro>,
}

impl Proceso {
    /// Arranca el binario con ese entorno y esos argumentos.
    ///
    /// El entorno se limpia a propósito: una prueba que heredara `KESTO_URL` de la sesión
    /// que la lanza estaría probando otra cosa de la que dice probar.
    pub async fn arrancar(entorno: &[(&str, &str)], argumentos: &[&str]) -> Self {
        let mut orden = Command::new(env!("CARGO_BIN_EXE_kesto-mcp"));

        orden.env_clear().env("RUST_LOG", "info");

        for (clave, valor) in entorno {
            orden.env(clave, valor);
        }

        let mut hijo = orden
            .args(argumentos)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("el binario de `kesto-mcp` arranca");

        let entrada = hijo.stdin.take().expect("la entrada del hijo se toma");
        let salida =
            BufReader::new(hijo.stdout.take().expect("la salida del hijo se toma")).lines();
        let errores = hijo.stderr.take().expect("el registro del hijo se toma");

        let registro = Registro::nuevo();
        let copia = Arc::clone(&registro);

        tokio::spawn(async move {
            let mut lineas = BufReader::new(errores).lines();

            while let Ok(Some(linea)) = lineas.next_line().await {
                {
                    let mut texto = copia.texto.lock().await;
                    texto.push_str(&linea);
                    texto.push('\n');
                }

                copia.aviso.send_modify(|contador| *contador += 1);
            }
        });

        Self {
            hijo,
            entrada,
            salida,
            registro,
        }
    }

    /// Envía un mensaje y espera su contestación.
    ///
    /// Se empareja por identificador porque el servidor puede intercalar notificaciones
    /// antes de la respuesta, y dar por hecho que la siguiente línea es la respuesta es
    /// exactamente la clase de suposición que rompe una prueba cuando cambia el SDK.
    pub async fn preguntar(&mut self, mensaje: Value) -> Value {
        let identificador = mensaje["id"].clone();

        self.avisar(mensaje).await;

        loop {
            let linea = tokio::time::timeout(Duration::from_secs(30), self.salida.next_line())
                .await
                .unwrap_or_else(|_| panic!("el servidor no contestó a {identificador}"))
                .expect("la salida del hijo se lee")
                .unwrap_or_else(|| panic!("el servidor cerró su salida sin contestar"));

            if linea.trim().is_empty() {
                continue;
            }

            let valor: Value = serde_json::from_str(&linea)
                .unwrap_or_else(|error| panic!("la línea no es JSON-RPC ({error}): {linea}"));

            if valor.get("id") == Some(&identificador) {
                return valor;
            }
        }
    }

    /// Envía un mensaje sin esperar contestación: una notificación.
    pub async fn avisar(&mut self, mensaje: Value) {
        let mut linea = mensaje.to_string();

        linea.push('\n');

        self.entrada
            .write_all(linea.as_bytes())
            .await
            .expect("la entrada del hijo se escribe");
        self.entrada
            .flush()
            .await
            .expect("la entrada del hijo se vacía");
    }

    /// Espera a que el registro contenga esa marca y devuelve lo escrito hasta entonces.
    pub async fn esperar_registro(&self, marca: &str) -> String {
        let marca = marca.to_owned();

        self.registro
            .esperar(move |texto| texto.contains(&marca).then(|| texto.to_owned()))
            .await
    }

    /// Espera a que el registro anuncie la dirección de escucha y la devuelve.
    pub async fn esperar_direccion(&self) -> String {
        self.registro.esperar(extraer_direccion).await
    }

    /// El registro tal y como está ahora.
    pub async fn registro(&self) -> String {
        self.registro.texto().await
    }

    /// Espera a que el proceso termine y devuelve su desenlace.
    pub async fn esperar_final(&mut self) -> std::process::ExitStatus {
        tokio::time::timeout(Duration::from_secs(30), self.hijo.wait())
            .await
            .expect("el proceso termina")
            .expect("el desenlace se lee")
    }
}
