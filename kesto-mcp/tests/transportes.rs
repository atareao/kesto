//! Los dos transportes, arrancando el binario de verdad.
//!
//! Aquí no se monta nada por dentro: se lanza `kesto-mcp` como proceso hijo y se le habla
//! el protocolo. Es la única forma de probar lo que de verdad se despliega —`stdio` sin
//! argumentos, `Streamable HTTP` con `--transport http`— y lo que la persona que lo lanza
//! verá: el código de salida cuando falta una variable, la dirección que anuncia y lo que
//! escribe en su registro.

mod common;

use common::{
    Proceso, Sesion, TOKEN, api_con_todo, es_error, mensaje_de, montar_error_lectura, recibidas,
    resultado,
};
use serde_json::{Value, json};
use wiremock::MockServer;

/// Las nueve herramientas, que son las mismas por los dos transportes.
fn catalogo_esperado() -> Vec<String> {
    let mut nombres = [
        "check_repository_health",
        "get_execution",
        "get_health",
        "list_executions",
        "list_jobs",
        "list_repositories",
        "list_snapshots",
        "list_targets",
        "run_backup",
    ]
    .iter()
    .map(|nombre| (*nombre).to_owned())
    .collect::<Vec<String>>();

    nombres.sort();

    nombres
}

/// El saludo del protocolo, tal y como lo manda un cliente MCP.
fn saludo() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "pruebas", "version": "1"}
        }
    })
}

/// La notificación de cortesía que sigue al saludo.
fn saludo_terminado() -> Value {
    json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
}

#[tokio::test]
async fn por_stdio_sin_argumentos_habla_el_protocolo_y_anuncia_las_nueve_herramientas() {
    let mock = MockServer::start().await;
    api_con_todo(&mock).await;

    let mut proceso = Proceso::arrancar(
        &[("KESTO_URL", &mock.uri()), ("KESTO_API_TOKEN", TOKEN)],
        &[],
    )
    .await;

    let respuesta = proceso.preguntar(saludo()).await;

    assert_eq!(
        respuesta["result"]["serverInfo"]["name"], "kesto-mcp",
        "el servidor se presenta con su nombre: {respuesta}"
    );
    assert!(
        !respuesta["result"]["capabilities"]["tools"].is_null(),
        "tiene que anunciar que sirve herramientas: {respuesta}"
    );

    proceso.avisar(saludo_terminado()).await;

    let catalogo = proceso
        .preguntar(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}))
        .await;
    let mut nombres: Vec<String> = catalogo["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("el catálogo tiene que ser una lista: {catalogo}"))
        .iter()
        .map(|herramienta| {
            herramienta["name"]
                .as_str()
                .expect("toda herramienta tiene nombre")
                .to_owned()
        })
        .collect();
    nombres.sort();

    assert_eq!(nombres, catalogo_esperado(), "el catálogo de `stdio`");

    let llamada = proceso
        .preguntar(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "list_repositories", "arguments": {}}
        }))
        .await;
    let filas = llamada["result"]["structuredContent"]["data"]
        .as_array()
        .unwrap_or_else(|| panic!("la herramienta tiene que devolver datos: {llamada}"));

    assert_eq!(
        filas.len(),
        2,
        "las dos respuestas de la API falsa: {llamada}"
    );
}

#[tokio::test]
async fn por_stdio_sin_kesto_url_el_arranque_falla_y_lo_dice() {
    let mut proceso = Proceso::arrancar(&[], &[]).await;

    let estado = proceso.esperar_final().await;
    let registro = proceso.esperar_registro("KESTO_URL").await;

    assert!(
        !estado.success(),
        "sin `KESTO_URL` el arranque tiene que fallar, y salió {estado:?}"
    );
    assert!(
        registro.contains("KESTO_URL"),
        "el error tiene que nombrar la variable que falta: {registro}"
    );
}

#[tokio::test]
async fn por_http_escucha_donde_se_le_dice_y_sirve_el_mismo_catalogo() {
    let mock = MockServer::start().await;
    api_con_todo(&mock).await;

    let proceso = Proceso::arrancar(
        &[("KESTO_URL", &mock.uri()), ("KESTO_API_TOKEN", TOKEN)],
        &["--transport", "http", "--bind", "127.0.0.1:0"],
    )
    .await;

    let direccion = proceso.esperar_direccion().await;
    let sesion = Sesion::conectar(format!("http://{direccion}/mcp")).await;

    let mut nombres = sesion.nombres().await;
    nombres.sort();

    assert_eq!(
        nombres,
        catalogo_esperado(),
        "el catálogo de `Streamable HTTP` tiene que ser el mismo que el de `stdio`"
    );

    let mensaje = sesion.llamar("list_jobs", json!({}), None).await;

    assert!(
        !es_error(&mensaje),
        "la herramienta tiene que contestar por HTTP: {mensaje}"
    );
    assert_eq!(
        mensaje["result"]["structuredContent"]["data"]
            .as_array()
            .expect("data es una lista")
            .len(),
        1
    );
}

#[tokio::test]
async fn el_token_no_sale_en_el_error_ni_en_lo_que_el_servidor_registra() {
    let mock = MockServer::start().await;

    // Un token que, si aparece en cualquier salida, tiene que hacer fallar la prueba.
    const SECRETO: &str = "token-que-no-puede-salir-en-claro-0123456789";

    montar_error_lectura(&mock, "/api/v1/jobs", 401).await;

    let mut proceso = Proceso::arrancar(
        &[
            ("KESTO_URL", &mock.uri()),
            ("KESTO_API_TOKEN", SECRETO),
            // El registro en `debug` incluye la línea que el servidor escribe al llamar a
            // la API: sin ella, el registro no diría nada de esta llamada y comprobar que
            // no lleva el token no probaría nada.
            ("RUST_LOG", "debug"),
        ],
        &[],
    )
    .await;

    proceso.preguntar(saludo()).await;
    proceso.avisar(saludo_terminado()).await;

    let llamada = proceso
        .preguntar(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "list_jobs", "arguments": {}}
        }))
        .await;
    let registro = proceso.esperar_registro("/api/v1/jobs").await;

    assert!(
        es_error(&llamada),
        "el token revocado tiene que fallar: {llamada}"
    );
    assert!(
        !resultado(&llamada).to_string().contains(SECRETO),
        "el token no puede salir en la respuesta: {llamada}"
    );
    assert!(
        mensaje_de(&llamada).contains("revocado") || mensaje_de(&llamada).contains("no es válido"),
        "el error tiene que explicar que el token no vale: {llamada}"
    );
    assert!(
        !registro.contains(SECRETO),
        "el token no puede salir en el registro del servidor: {registro}"
    );
    assert!(
        recibidas(&mock)
            .await
            .iter()
            .any(|(_, _, autorizacion)| autorizacion.as_deref()
                == Some(&format!("Bearer {SECRETO}"))),
        "el token sí tiene que llegar a la API, aunque no se publique"
    );
}
