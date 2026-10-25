//! Pruebas transversales de la API sobre los secretos: lo que se registra y lo que
//! se guarda.
//!
//! Van aquí y no en el `mod tests` de cada recurso porque lo que afirman no es de
//! ningún recurso en particular: es una propiedad del **camino** que una credencial
//! recorre al entrar por la API —el extractor, el manejador, la capa de acceso y el
//! fichero de la base—, y de lo que ese camino deja en el registro del proceso.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::http::StatusCode;

use crate::api::api_router;
use crate::api::jobs::AltaDeTareaDelCable;
use crate::api::repositories::AltaDeRepositorioDelCable;
use crate::api::targets::AltaDeTargetDelCable;
use crate::state::{AppState, EstadoDeBase};
use crate::test_support::{
    DirectorioDePrueba, base_con_clave, body_json, body_text, capturar_log, config_con_dsn, delete,
    estado_desarrollo, get, post_json, put_json,
};

/// Credencial de acceso distinguible: larga y con coletilla propia, para que un
/// `contains` sobre el texto crudo no pueda dar un falso positivo por casualidad.
const CREDENCIAL_DE_ACCESO: &str = "AKIAIOSFODNN7EXAMPLE-secreto-de-acceso-1-19";
/// Passphrase distinguible del repositorio.
const PASSPHRASE_DEL_REPOSITORIO: &str = "passphrase-distinguible-1-19-000000000000000001";
/// Contraseña de base distinguible del target.
const CONTRASENA_DEL_TARGET: &str = "contrasena-distinguible-1-19-000000000000000001";

/// Las tres credenciales que ningún camino puede publicar.
const CREDENCIALES: [&str; 3] = [
    CREDENCIAL_DE_ACCESO,
    PASSPHRASE_DEL_REPOSITORIO,
    CONTRASENA_DEL_TARGET,
];

/// Nombre distinguible del repositorio: **es el control**, y tiene que poder
/// encontrarse en los bytes para que la ausencia de las credenciales signifique algo.
const CONTROL_BUSCABLE: &str = "repo-1-20-buscable-en-el-fichero";
/// Credencial de acceso de la prueba de los bytes del fichero.
const ACCESO_1_20: &str = "AKIAIOSFODNN7EXAMPLE-1-20-en-claro-no";
/// Passphrase de la prueba de los bytes del fichero.
const PASSPHRASE_1_20: &str = "passphrase-distinguible-1-20-000000000000000020";
/// Contraseña de base de la prueba de los bytes del fichero.
const CONTRASENA_1_20: &str = "contrasena-distinguible-1-20-000000000000000020";

/// Nombre del repositorio que solo viaja en el cuerpo de la petición rechazada.
const NOMBRE_DEL_RECHAZO: &str = "repo-1-22-rechazado";

/// Las credenciales de la prueba de los bytes del fichero.
const CREDENCIALES_1_20: [&str; 3] = [ACCESO_1_20, PASSPHRASE_1_20, CONTRASENA_1_20];

/// Busca una aguja de texto **literalmente** en los bytes dados.
fn contiene(pajar: &[u8], aguja: &str) -> bool {
    !aguja.is_empty()
        && pajar
            .windows(aguja.len())
            .any(|ventana| ventana == aguja.as_bytes())
}

/// Los bytes del fichero de la base y de sus `-wal` y `-shm`, concatenados.
///
/// Se leen los tres porque con WAL el grueso de lo escrito vive en `-wal` hasta el
/// `CHECKPOINT`, y buscar solo en el `.db` sería buscar donde no está.
fn bytes_de_los_ficheros(directorio: &DirectorioDePrueba) -> Vec<u8> {
    let mut bytes = Vec::new();

    for sufijo in ["", crate::db::SUFIJO_WAL, "-shm"] {
        let ruta = PathBuf::from(format!(
            "{}{sufijo}",
            directorio.ruta_de_la_base().display()
        ));

        if let Ok(contenido) = std::fs::read(&ruta) {
            bytes.extend_from_slice(&contenido);
        }
    }

    bytes
}

/// Afirma que un texto de respuesta no lleva ninguna credencial.
fn sin_credenciales(etiqueta: &str, texto: &str, credenciales: &[&str]) {
    for credencial in credenciales {
        assert!(
            !texto.contains(credencial),
            "la respuesta de {etiqueta} publicó una credencial: {texto}"
        );
    }
}

/// El `id` que trae el `data` de una respuesta, ya como texto.
///
/// Se saca del texto crudo porque `body_text` y `body_json` consumen la respuesta y
/// no se pueden llamar los dos: quien necesite el `id` y además el texto, lee el
/// texto y parsea de aquí.
fn id_de(etiqueta: &str, texto: &str) -> String {
    let cuerpo: serde_json::Value = serde_json::from_str(texto)
        .unwrap_or_else(|error| panic!("la respuesta de {etiqueta} debe traer JSON: {error}"));

    match cuerpo["data"]["id"].as_str() {
        Some(id) => id.to_owned(),
        None => panic!("la respuesta de {etiqueta} debe traer el `id` en `data`: {texto}"),
    }
}

/// Da de alta un recurso por la API y devuelve su `id`, afirmando que la respuesta no
/// publica ninguna credencial.
async fn alta_de(
    router: &Router,
    recurso: &str,
    cuerpo: &serde_json::Value,
    etiqueta: &str,
) -> String {
    let respuesta = post_json(router.clone(), recurso, cuerpo).await;

    assert_eq!(
        respuesta.status(),
        StatusCode::CREATED,
        "el alta de {etiqueta} debe contestar 201"
    );

    let texto = body_text(respuesta).await;

    sin_credenciales(&format!("el alta de {etiqueta}"), &texto, &CREDENCIALES);

    id_de(&format!("el alta de {etiqueta}"), &texto)
}

/// Recorre las cuatro operaciones de un recurso —alta, lectura, actualización y
/// borrado— afirmando en **cada** respuesta que el texto crudo no lleva ninguna
/// credencial, y devuelve el `id` del recurso.
async fn barrido(
    router: &Router,
    recurso: &str,
    cuerpo: &serde_json::Value,
    etiqueta: &str,
) -> String {
    let id = alta_de(router, recurso, cuerpo, etiqueta).await;

    let lectura = get(router.clone(), &format!("{recurso}/{id}")).await;

    assert_eq!(
        lectura.status(),
        StatusCode::OK,
        "la lectura de {etiqueta} debe contestar 200"
    );
    sin_credenciales(
        &format!("la lectura de {etiqueta}"),
        &body_text(lectura).await,
        &CREDENCIALES,
    );

    let actualizacion = put_json(router.clone(), &format!("{recurso}/{id}"), cuerpo).await;

    assert_eq!(
        actualizacion.status(),
        StatusCode::OK,
        "la actualización de {etiqueta} debe contestar 200"
    );
    sin_credenciales(
        &format!("la actualización de {etiqueta}"),
        &body_text(actualizacion).await,
        &CREDENCIALES,
    );

    let borrado = delete(router.clone(), &format!("{recurso}/{id}")).await;

    assert_eq!(
        borrado.status(),
        StatusCode::OK,
        "el borrado de {etiqueta} debe contestar 200"
    );
    sin_credenciales(
        &format!("el borrado de {etiqueta}"),
        &body_text(borrado).await,
        &CREDENCIALES,
    );

    id
}

/// El cuerpo de un alta de repositorio, con las dos credenciales que se le pasen.
fn cuerpo_de_repositorio(nombre: &str, credencial: &str, passphrase: &str) -> serde_json::Value {
    serde_json::json!({
        "name": nombre,
        "endpoint": "https://s3.example.test",
        "bucket": nombre,
        "region": "eu-west-1",
        "access_key_id": "AKIAIOSFODNN7EXAMPLE",
        "secret_access_key": credencial,
        "passphrase": passphrase,
    })
}

/// El cuerpo de un alta de target, con la contraseña que se le pase.
fn cuerpo_de_target(nombre: &str, contrasena: &str) -> serde_json::Value {
    serde_json::json!({
        "name": nombre,
        "target_type": "mariadb",
        "container_name": "mariadb-de-prueba",
        "db_user": "kesto",
        "db_password": contrasena,
        "db_name": "kesto",
    })
}

/// El cuerpo de un alta de tarea, apuntando a los dos recursos sembrados.
fn cuerpo_de_tarea(target_id: &str, repository_id: &str) -> serde_json::Value {
    serde_json::json!({
        "target_id": target_id,
        "repository_id": repository_id,
        "cron_expression": "0 3 * * *",
        "keep_daily": 7,
        "keep_weekly": 4,
        "keep_monthly": 12,
        "enabled": true,
    })
}

/// 1.19 — Ninguna respuesta de los tres recursos trae un secreto.
///
/// La red de tipo hace que una respuesta con un secreto **no compile** —[`crate::secret::SecretString`]
/// no implementa `Serialize`—, pero eso protege el tipo, no el camino: un campo que se colara
/// dentro de otro —un `data` que serializara la entidad entera, un `message` que repitiera el
/// cuerpo— no aparecería como clave propia y sí en el texto. Esta prueba mira el **byte que
/// viajó**, en las cuatro operaciones de los tres recursos.
///
/// La tarea no tiene credenciales propias, así que se siembran antes un target y un repositorio
/// —por el mismo camino de la API— y las credenciales que se buscan en las respuestas de la
/// tarea son las de esas dos filas: si un listado o un `data` las arrastrara, aquí se vería.
#[tokio::test]
async fn ninguna_respuesta_de_los_tres_recursos_trae_un_secreto() {
    let router = api_router(estado_desarrollo().await);

    barrido(
        &router,
        "/api/v1/repositories",
        &cuerpo_de_repositorio(
            "repo-1-19",
            CREDENCIAL_DE_ACCESO,
            PASSPHRASE_DEL_REPOSITORIO,
        ),
        "un repositorio",
    )
    .await;

    barrido(
        &router,
        "/api/v1/targets",
        &cuerpo_de_target("target-1-19", CONTRASENA_DEL_TARGET),
        "un target",
    )
    .await;

    let repository_id = alta_de(
        &router,
        "/api/v1/repositories",
        &cuerpo_de_repositorio(
            "repo-1-19-sembrado",
            CREDENCIAL_DE_ACCESO,
            PASSPHRASE_DEL_REPOSITORIO,
        ),
        "el repositorio sembrado",
    )
    .await;

    let target_id = alta_de(
        &router,
        "/api/v1/targets",
        &cuerpo_de_target("target-1-19-sembrado", CONTRASENA_DEL_TARGET),
        "el target sembrado",
    )
    .await;

    barrido(
        &router,
        "/api/v1/jobs",
        &cuerpo_de_tarea(&target_id, &repository_id),
        "una tarea",
    )
    .await;
}

/// 1.20 — La credencial que entró por la API no aparece en los bytes del fichero.
///
/// El §3 declara las tres columnas de secreto como `TEXT` y **no** se guardan en claro —el
/// sobre es `v1:<nonce>:<cifrado>`—, y la prueba 1.47 de la capa de acceso ya lo afirma para
/// el camino de la capa; ésta lo afirma para el camino **completo**, con la credencial entrando
/// por el `POST` de la API, que es el camino real del panel.
///
/// El control es lo que hace que la ausencia signifique algo: sin él, un fichero vacío o mal
/// leído pasaría la prueba sin haber mirado nada.
#[tokio::test]
async fn la_credencial_que_entro_por_la_api_no_aparece_en_los_bytes_del_fichero() {
    let (directorio, pool, clave) = base_con_clave().await;

    let estado = AppState::new(
        config_con_dsn(&directorio.dsn()),
        EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
    )
    .expect("el estado con pool y clave se construye");

    let router = api_router(estado);

    alta_de(
        &router,
        "/api/v1/repositories",
        &cuerpo_de_repositorio(CONTROL_BUSCABLE, ACCESO_1_20, PASSPHRASE_1_20),
        "un repositorio",
    )
    .await;

    alta_de(
        &router,
        "/api/v1/targets",
        &cuerpo_de_target("target-1-20", CONTRASENA_1_20),
        "un target",
    )
    .await;

    let bytes = bytes_de_los_ficheros(&directorio);

    assert!(
        contiene(&bytes, CONTROL_BUSCABLE),
        "el control no aparece en los bytes: la búsqueda no está mirando donde cree"
    );

    for credencial in CREDENCIALES_1_20 {
        assert!(
            !contiene(&bytes, credencial),
            "la credencial {credencial} quedó en claro en los ficheros de la base"
        );
    }
}

/// 1.21 — El registro de un alta no publica la credencial y deja la traza de que se atendió.
///
/// La spec del delta `secrets` pide las dos mitades: el registro **no** lleva el cuerpo —que es
/// por donde viajan las credenciales— y **sí** deja constancia de que la petición se atendió.
/// Sin la segunda mitad, un registro vacío cumpliría la primera por omisión y no habría forma de
/// saber si alguien está pasando por ahí.
///
/// La captura se coge **antes** de las peticiones y el buffer es **por hilo**: nunca
/// `set_default` ni `with_default`, que dejan un callsite cacheado como `Interest::never` para
/// todo el proceso y su registro desaparece en silencio.
#[tokio::test]
async fn el_registro_de_un_alta_no_publica_la_credencial_y_deja_la_traza_de_que_se_atendio() {
    let router = api_router(estado_desarrollo().await);
    let (buffer, _guard) = capturar_log();

    let respuesta = post_json(
        router,
        "/api/v1/repositories",
        &cuerpo_de_repositorio(
            "repo-1-21",
            CREDENCIAL_DE_ACCESO,
            PASSPHRASE_DEL_REPOSITORIO,
        ),
    )
    .await;

    assert_eq!(
        respuesta.status(),
        StatusCode::CREATED,
        "el alta del escenario tiene que atenderse"
    );

    let log = buffer.texto();

    for credencial in CREDENCIALES {
        assert!(
            !log.contains(credencial),
            "el registro del alta publicó la credencial {credencial}: {log}"
        );
    }

    assert!(
        log.contains("petición atendida"),
        "el registro no deja constancia de que la petición se atendió: {log}"
    );
}

/// 1.22 — Un rechazo de validación deja la traza del campo y no el cuerpo.
///
/// El rechazo de validación es un desenlace normal, pero la spec pide que quede la traza de
/// **qué campo** no cumplía —para poder diagnosticar un formulario que falla sin pedirle a nadie
/// que reproduzca— y, a la vez, que el cuerpo no acabe en el registro, porque el cuerpo en crudo
/// lleva la credencial y **no** pasa por ningún `SecretString` mientras está sin mapear.
///
/// La aguja del cuerpo es el **nombre** del repositorio: viaja solo en el cuerpo y en ningún
/// otro sitio, así que si aparece en el registro es que alguien lo registró.
#[tokio::test]
async fn un_rechazo_de_validacion_deja_la_traza_del_campo_y_no_el_cuerpo() {
    let router = api_router(estado_desarrollo().await);
    let (buffer, _guard) = capturar_log();

    let respuesta = post_json(
        router,
        "/api/v1/repositories",
        &cuerpo_de_repositorio(NOMBRE_DEL_RECHAZO, CREDENCIAL_DE_ACCESO, ""),
    )
    .await;

    assert_eq!(
        respuesta.status(),
        StatusCode::BAD_REQUEST,
        "una passphrase vacía no puede darse de alta"
    );

    let devuelto = body_json(respuesta).await;

    assert!(
        devuelto["message"]
            .as_str()
            .unwrap_or_default()
            .contains("passphrase"),
        "el 400 debe nombrar el campo que no cumplía: {devuelto}"
    );

    let log = buffer.texto();

    assert!(
        log.contains("passphrase"),
        "el registro no deja la traza del campo que no cumplía: {log}"
    );
    assert!(
        !log.contains(CREDENCIAL_DE_ACCESO),
        "el registro del rechazo publicó la credencial: {log}"
    );
    assert!(
        !log.contains(NOMBRE_DEL_RECHAZO),
        "el registro del rechazo publicó el cuerpo de la petición: {log}"
    );
}

/// Credencial de acceso distinguible del repositorio del cable.
const CREDENCIAL_DE_ACCESO_DEL_CABLE: &str = "AKIAIOSFODNN7EXAMPLE-1-17-no-debe-salir";
/// Passphrase distinguible del repositorio del cable.
const PASSPHRASE_DEL_CABLE: &str = "passphrase-1-17-no-debe-salir-0000000000001";
/// Contraseña de base distinguible del target del cable.
const CONTRASENA_DEL_CABLE: &str = "contrasena-1-17-no-debe-salir-00000000000001";

/// 1.17 — El `Debug` del tipo del cuerpo del cable no publica la credencial.
///
/// Cubre el delta `secrets`, escenario «El Debug del tipo del cuerpo no publica la
/// credencial». La prueba de `SecretString` ya fija que la pieza redacta; ésta fija
/// que el **DTO** no la puentea: el `Debug` de un tipo del cuerpo es lo primero que
/// se formatea al depurar un manejador que no se comporta, y un `Debug` manual sobre
/// el DTO —o dejar de derivarlo— publicaría el valor aunque `SecretString` siguiera
/// redactando. Como el DTO solo guarda el secreto envuelto, la redacción la hereda;
/// lo que la prueba afirma es que **sigue heredándola**.
///
/// Las dos mitades importan y por eso se afirman las dos: la primera —que el valor no
/// salga— es la que protege; la segunda —que la salida **diga** que está redactada— es
/// la que impide que la protección se confunda con un campo vacío, que es lo que haría
/// pensar a quien depura que el campo llegó sin rellenar.
#[test]
fn el_debug_del_cuerpo_del_cable_no_publica_la_credencial() {
    /// Comprueba que la salida no publica ninguna de las credenciales y que además
    /// dice que redacta. La etiqueta nombra el DTO para saber cuál falló sin volcar
    /// la credencial en el mensaje.
    fn comprobar_redaccion(etiqueta: &str, salida: &str, credenciales: &[&str]) {
        for (indice, credencial) in credenciales.iter().enumerate() {
            assert!(
                !salida.contains(*credencial),
                "el `Debug` de `{etiqueta}` publicó la credencial en la posición {indice}: {salida}"
            );
        }

        assert!(
            salida.to_lowercase().contains("redact"),
            "el `Debug` de `{etiqueta}` no dice que la credencial está redactada —y una salida que no lo dice se confunde con un campo vacío—: {salida}"
        );
    }

    /// Igual que la anterior, pero sin exigir la palabra: este DTO no declara ningún
    /// secreto, así que no tiene nada que redactar y pedirla ahí afirmaría algo que no
    /// es cierto. Lo que afirma es que una credencial no se coló en un tipo que no la
    /// declara.
    fn comprobar_sin_credenciales(etiqueta: &str, salida: &str, credenciales: &[&str]) {
        for (indice, credencial) in credenciales.iter().enumerate() {
            assert!(
                !salida.contains(*credencial),
                "el `Debug` de `{etiqueta}` publicó la credencial en la posición {indice}: {salida}"
            );
        }
    }

    let json_del_repositorio = format!(
        r#"{{"name":"repo-1-17","endpoint":"https://s3.example.test/1-17","bucket":"cubo","region":"eu-west-1","access_key_id":"AKIAIOSFODNN7EXAMPLE","secret_access_key":"{CREDENCIAL_DE_ACCESO_DEL_CABLE}","passphrase":"{PASSPHRASE_DEL_CABLE}"}}"#
    );
    let cuerpo_del_repositorio: AltaDeRepositorioDelCable =
        serde_json::from_str(&json_del_repositorio)
            .expect("el cuerpo del alta de un repositorio tiene que deserializar");

    comprobar_redaccion(
        "AltaDeRepositorioDelCable",
        &format!("{cuerpo_del_repositorio:?}"),
        &[CREDENCIAL_DE_ACCESO_DEL_CABLE, PASSPHRASE_DEL_CABLE],
    );

    let json_del_target = format!(
        r#"{{"name":"target-1-17","target_type":"mariadb","container_name":"contenedor","db_user":"usuario","db_password":"{CONTRASENA_DEL_CABLE}","db_name":"base"}}"#
    );
    let cuerpo_del_target: AltaDeTargetDelCable = serde_json::from_str(&json_del_target)
        .expect("el cuerpo del alta de un target tiene que deserializar");

    comprobar_redaccion(
        "AltaDeTargetDelCable",
        &format!("{cuerpo_del_target:?}"),
        &[CONTRASENA_DEL_CABLE],
    );

    let json_de_la_tarea = r#"{"target_id":"00000000-0000-4000-8000-000000000000","repository_id":"11111111-1111-4111-8111-111111111111","cron_expression":"0 3 * * *","keep_daily":7,"keep_weekly":4,"keep_monthly":12,"enabled":true}"#;
    let cuerpo_de_la_tarea: AltaDeTareaDelCable = serde_json::from_str(json_de_la_tarea)
        .expect("el cuerpo del alta de una tarea tiene que deserializar");

    comprobar_sin_credenciales(
        "AltaDeTareaDelCable",
        &format!("{cuerpo_de_la_tarea:?}"),
        &[
            CREDENCIAL_DE_ACCESO_DEL_CABLE,
            PASSPHRASE_DEL_CABLE,
            CONTRASENA_DEL_CABLE,
        ],
    );
}
