//! Las nueve herramientas, contra la API falsa y por el protocolo.
//!
//! Cada prueba habla `tools/call` con el transporte HTTP de verdad y mira dos cosas: lo que
//! la herramienta devuelve y lo que la API falsa **recibió**. La segunda es la que impide
//! que una herramienta pase por casualidad: una que no llamara a la API también devolvería
//! algo.

mod common;

use common::{
    EJECUCION, REPOSITORIO, Sesion, TAREA, TOKEN, TOKEN_RUN, api_con_todo, consultas, datos,
    envoltura, error_de_la_api, es_error, mensaje_de, montar_accion, montar_error_lectura,
    montar_lectura, montar_lectura_con_parametro, recibidas, resultado,
};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn list_repositories_devuelve_los_repositorios() {
    let mock = MockServer::start().await;
    api_con_todo(&mock).await;
    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN)).await;

    let mensaje = sesion.llamar("list_repositories", json!({}), None).await;

    let filas = datos(&mensaje)
        .as_array()
        .expect("data es una lista")
        .clone();

    assert_eq!(filas.len(), 2, "esperaba los dos repositorios: {filas:?}");
    assert_eq!(filas[0]["name"], "repos-uno");
    assert_eq!(filas[0]["health"], "healthy");

    let peticiones = recibidas(&mock).await;

    assert!(
        peticiones.iter().any(|(metodo, camino, autorizacion)| {
            metodo == "GET"
                && camino == "/api/v1/repositories"
                && autorizacion.as_deref() == Some(&format!("Bearer {TOKEN}"))
        }),
        "la API tiene que haber recibido el GET con el token del entorno: {peticiones:?}"
    );
}

#[tokio::test]
async fn list_targets_devuelve_los_targets() {
    let mock = MockServer::start().await;
    api_con_todo(&mock).await;
    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN)).await;

    let mensaje = sesion.llamar("list_targets", json!({}), None).await;

    let filas = datos(&mensaje)
        .as_array()
        .expect("data es una lista")
        .clone();

    assert_eq!(filas.len(), 1);
    assert_eq!(filas[0]["kind"], "postgresql");
    assert!(
        !filas[0].to_string().contains("password"),
        "el listado de targets no puede traer credenciales: {filas:?}"
    );

    assert!(
        recibidas(&mock)
            .await
            .iter()
            .any(|(_, camino, _)| camino == "/api/v1/targets")
    );
}

#[tokio::test]
async fn list_jobs_devuelve_las_tareas() {
    let mock = MockServer::start().await;
    api_con_todo(&mock).await;
    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN)).await;

    let mensaje = sesion.llamar("list_jobs", json!({}), None).await;

    let filas = datos(&mensaje)
        .as_array()
        .expect("data es una lista")
        .clone();

    assert_eq!(filas[0]["cron_expression"], "0 3 * * *");
    assert_eq!(filas[0]["enabled"], true);
}

#[tokio::test]
async fn list_snapshots_devuelve_las_instantaneas_del_repositorio() {
    let mock = MockServer::start().await;
    montar_lectura_con_parametro(
        &mock,
        "/api/v1/snapshots",
        "repository_id",
        REPOSITORIO,
        json!({
            "snapshots": [{"id": "snap-1", "time": "2026-01-01T03:00:00Z"}],
            "total": 1,
            "truncated": false,
        }),
    )
    .await;

    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN)).await;
    let mensaje = sesion
        .llamar(
            "list_snapshots",
            json!({"repository_id": REPOSITORIO}),
            None,
        )
        .await;

    let filas = datos(&mensaje)["snapshots"]
        .as_array()
        .expect("el listado trae `snapshots` como lista")
        .clone();

    assert_eq!(filas[0]["id"], "snap-1");

    let consultas = consultas(&mock).await;
    let consulta = consultas
        .iter()
        .find(|consulta| consulta.contains(&format!("repository_id={REPOSITORIO}")))
        .unwrap_or_else(|| panic!("no llegó la consulta del listado: {consultas:?}"));

    // La entrada de `list_snapshots` no expone `limit`, así que la herramienta tiene que
    // mandar el tope válido por su cuenta: sin él la API aplicaría su defecto (50) y el MCP
    // dejaría de ver las instantáneas que veía antes de que el listado paginara.
    assert!(consulta.contains("limit=200"), "{consulta}");
}

#[tokio::test]
async fn list_executions_respeta_los_filtros() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/executions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(envoltura(json!({
            "executions": [{
                "id": EJECUCION,
                "job_id": TAREA,
                "status": "success",
                "bytes_sent": 1024,
            }],
            "total": 1,
        }))))
        .mount(&mock)
        .await;

    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN)).await;
    let mensaje = sesion
        .llamar(
            "list_executions",
            json!({"job_id": TAREA, "limit": 5}),
            None,
        )
        .await;

    let filas = datos(&mensaje)["executions"]
        .as_array()
        .expect("el histórico trae `executions` como lista")
        .clone();

    assert_eq!(filas.len(), 1);
    assert_eq!(filas[0]["status"], "success");

    let consultas = consultas(&mock).await;
    let consulta = consultas
        .iter()
        .find(|consulta| consulta.contains("job_id"))
        .unwrap_or_else(|| panic!("no llegó ningún filtro: {consultas:?}"));

    assert!(consulta.contains(&format!("job_id={TAREA}")), "{consulta}");
    assert!(consulta.contains("limit=5"), "{consulta}");
}

#[tokio::test]
async fn get_execution_devuelve_una_ejecucion_concreta() {
    let mock = MockServer::start().await;
    montar_lectura_con_parametro(
        &mock,
        "/api/v1/executions",
        "id",
        EJECUCION,
        json!({
            "executions": [{"id": EJECUCION, "status": "failed", "error": "timeout"}],
            "total": 1,
        }),
    )
    .await;

    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN)).await;
    let mensaje = sesion
        .llamar("get_execution", json!({"id": EJECUCION}), None)
        .await;

    let filas = datos(&mensaje)["executions"]
        .as_array()
        .expect("el histórico trae `executions` como lista")
        .clone();

    assert_eq!(filas[0]["status"], "failed");
}

#[tokio::test]
async fn get_execution_con_un_identificador_inexistente_no_devuelve_una_lista_vacia() {
    let mock = MockServer::start().await;
    montar_lectura(
        &mock,
        "/api/v1/executions",
        json!({"executions": [], "total": 0}),
    )
    .await;

    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN)).await;
    let mensaje = sesion
        .llamar(
            "get_execution",
            json!({"id": "99999999-9999-9999-9999-999999999999"}),
            None,
        )
        .await;

    assert!(
        es_error(&mensaje),
        "una ejecución que no está no puede salir como lista vacía: {mensaje}"
    );
    assert!(
        mensaje_de(&mensaje).contains("ejecución"),
        "el error tiene que explicar qué falta: {mensaje}"
    );
}

#[tokio::test]
async fn get_health_devuelve_la_salud_y_el_modo_de_autenticacion() {
    let mock = MockServer::start().await;
    api_con_todo(&mock).await;
    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN)).await;

    let mensaje = sesion.llamar("get_health", json!({}), None).await;

    assert_eq!(datos(&mensaje)["auth_mode"], "oidc");
    assert_eq!(datos(&mensaje)["version"], "0.12.0");
}

#[tokio::test]
async fn get_health_sin_token_dice_en_que_modo_esta_el_servicio() {
    let mock = MockServer::start().await;
    montar_lectura(
        &mock,
        "/api/v1/health",
        json!({"version": "0.12.0", "auth_mode": "disabled"}),
    )
    .await;

    let sesion = Sesion::abrir(&mock.uri(), None).await;
    let mensaje = sesion.llamar("get_health", json!({}), None).await;

    assert!(
        !es_error(&mensaje),
        "sin token `get_health` tiene que contestar: {mensaje}"
    );
    assert_eq!(datos(&mensaje)["auth_mode"], "disabled");

    let peticiones = recibidas(&mock).await;

    assert!(
        peticiones.iter().any(|(metodo, camino, autorizacion)| {
            metodo == "GET" && camino == "/api/v1/health" && autorizacion.is_none()
        }),
        "sin token no se manda cabecera `Authorization`: {peticiones:?}"
    );

    // El modo `disabled` de la API no vuelve público el resto del catálogo: las otras
    // ocho herramientas siguen exigiendo credencial.
    let sin_token = sesion.llamar("list_jobs", json!({}), None).await;

    assert!(es_error(&sin_token), "{sin_token}");
    assert!(
        mensaje_de(&sin_token).contains("KESTO_API_TOKEN"),
        "{sin_token}"
    );
}

#[tokio::test]
async fn run_backup_lanza_la_copia_de_una_tarea() {
    let mock = MockServer::start().await;
    let ruta = format!("/api/v1/jobs/{TAREA}/run");
    montar_accion(
        &mock,
        &ruta,
        201,
        json!({"id": EJECUCION, "job_id": TAREA, "status": "pending"}),
    )
    .await;

    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN_RUN)).await;
    let mensaje = sesion
        .llamar("run_backup", json!({"id": TAREA}), None)
        .await;

    assert_eq!(datos(&mensaje)["status"], "pending");

    let peticiones = recibidas(&mock).await;

    assert!(
        peticiones
            .iter()
            .any(|(metodo, camino, _)| metodo == "POST" && camino == &ruta),
        "la API tiene que haber recibido el POST del disparo: {peticiones:?}"
    );
}

#[tokio::test]
async fn check_repository_health_comprueba_y_persiste() {
    let mock = MockServer::start().await;
    let ruta = format!("/api/v1/repositories/{REPOSITORIO}/health");
    montar_accion(&mock, &ruta, 200, json!({"health": "healthy"})).await;

    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN_RUN)).await;
    let mensaje = sesion
        .llamar(
            "check_repository_health",
            json!({"repository_id": REPOSITORIO}),
            None,
        )
        .await;

    assert_eq!(datos(&mensaje)["health"], "healthy");

    assert!(
        recibidas(&mock)
            .await
            .iter()
            .any(|(metodo, camino, _)| metodo == "POST" && camino == &ruta)
    );
}

#[tokio::test]
async fn una_accion_con_un_token_de_solo_lectura_se_rechaza_con_el_scope() {
    let mock = MockServer::start().await;
    let ruta = format!("/api/v1/jobs/{TAREA}/run");
    montar_accion(&mock, &ruta, 403, json!({})).await;

    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN)).await;
    let mensaje = sesion
        .llamar("run_backup", json!({"id": TAREA}), None)
        .await;

    assert!(es_error(&mensaje), "{mensaje}");
    assert!(
        mensaje_de(&mensaje).contains("run"),
        "el error tiene que nombrar el scope que falta: {mensaje}"
    );
}

#[tokio::test]
async fn un_token_revocado_se_explica_y_no_devuelve_una_lista_vacia() {
    let mock = MockServer::start().await;
    montar_error_lectura(&mock, "/api/v1/jobs", 401).await;

    let sesion = Sesion::abrir(&mock.uri(), Some("token-revocado")).await;
    let mensaje = sesion.llamar("list_jobs", json!({}), None).await;

    assert!(es_error(&mensaje), "{mensaje}");

    let texto = mensaje_de(&mensaje);

    assert!(
        texto.contains("revocado") || texto.contains("no es válido"),
        "el error tiene que decir que el token no vale: {texto}"
    );
    assert!(
        !resultado(&mensaje)
            .get("structuredContent")
            .is_some_and(|cuerpo| { cuerpo["data"].as_array().is_some_and(Vec::is_empty) }),
        "un token revocado no puede salir como lista vacía: {mensaje}"
    );
}

#[tokio::test]
async fn sin_token_el_error_nombra_la_variable_del_entorno() {
    let mock = MockServer::start().await;
    api_con_todo(&mock).await;

    let sesion = Sesion::abrir(&mock.uri(), None).await;
    let mensaje = sesion.llamar("list_repositories", json!({}), None).await;

    assert!(es_error(&mensaje), "{mensaje}");
    assert!(
        mensaje_de(&mensaje).contains("KESTO_API_TOKEN"),
        "el error tiene que nombrar la variable que falta: {mensaje}"
    );
    assert!(
        recibidas(&mock).await.is_empty(),
        "sin token no se llama a la API: {:?}",
        recibidas(&mock).await
    );
}

#[tokio::test]
async fn el_servidor_sigue_vivo_tras_un_error_de_credencial() {
    let mock = MockServer::start().await;

    // Dos respuestas para el mismo camino, separadas por la credencial: la revocada no
    // vale y la buena sí. Es la forma de probar que el servidor no se cae ni se queda
    // pegado a la credencial fallida.
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs"))
        .and(header("authorization", "Bearer token-revocado"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(error_de_la_api(401, "el token está revocado")),
        )
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(envoltura(vec![json!({"id": TAREA, "name": "tarea-uno"})])),
        )
        .mount(&mock)
        .await;

    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN)).await;

    let revocado = sesion
        .llamar("list_jobs", json!({}), Some("token-revocado"))
        .await;

    assert!(es_error(&revocado), "{revocado}");

    let despues = sesion.llamar("list_jobs", json!({}), None).await;

    assert!(
        !es_error(&despues),
        "con un token válido la siguiente llamada tiene que funcionar: {despues}"
    );
    assert_eq!(datos(&despues).as_array().expect("lista").len(), 1);
}

#[tokio::test]
async fn el_catalogo_es_de_nueve_herramientas_y_ninguna_destructiva() {
    let mock = MockServer::start().await;
    api_con_todo(&mock).await;
    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN_RUN)).await;

    let mut nombres = sesion.nombres().await;
    nombres.sort();

    let mut esperado = vec![
        "check_repository_health",
        "get_execution",
        "get_health",
        "list_executions",
        "list_jobs",
        "list_repositories",
        "list_snapshots",
        "list_targets",
        "run_backup",
    ];
    esperado.sort();

    assert_eq!(nombres, esperado, "el catálogo es exactamente este");

    for prohibida in [
        "restore_snapshot",
        "delete_repository",
        "delete_job",
        "delete_target",
        "update_repository",
        "cancel_execution",
    ] {
        assert!(
            !nombres.iter().any(|nombre| nombre == prohibida),
            "no puede existir {prohibida}: {nombres:?}"
        );
    }
}

#[tokio::test]
async fn una_herramienta_que_no_existe_es_un_error_del_protocolo_y_no_llega_a_la_api() {
    let mock = MockServer::start().await;
    api_con_todo(&mock).await;
    let sesion = Sesion::abrir(&mock.uri(), Some(TOKEN_RUN)).await;

    let mensaje = sesion
        .llamar(
            "restore_snapshot",
            json!({"id": EJECUCION}),
            Some(TOKEN_RUN),
        )
        .await;

    assert!(
        mensaje.get("error").is_some(),
        "una herramienta que no está tiene que dar el error del protocolo: {mensaje}"
    );
    assert!(
        recibidas(&mock).await.is_empty(),
        "no puede llegar ninguna petición de restauración a la API: {:?}",
        recibidas(&mock).await
    );
}
