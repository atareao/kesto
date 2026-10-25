//! Pruebas de la partición del router sobre las rutas **nuevas** del §5.
//!
//! La partición —una mitad pública por lista blanca y una de negocio detrás del
//! *gate* de sesión— ya tiene sus pruebas en [`crate::api`], pero se escribieron
//! cuando los recursos no existían y usan rutas ficticias. Aquí se comprueba lo
//! mismo sobre las quince rutas reales, que es donde la regla tiene consecuencias:
//! una ruta nueva que quedara del lado público no lo parecería hasta que alguien
//! leyera el registro de accesos del servidor.

use axum::Router;
use axum::http::StatusCode;
use axum::response::Response;

use crate::test_support::{body_text, delete, get, post, put_json, router_desarrollo, router_oidc};

/// Un identificador bien formado cualquiera.
///
/// No hace falta que exista: en las pruebas de la mitad protegida el *gate* corta
/// antes de que nadie mire la fila, y en las del modo de desarrollo el `400` que se
/// busca es el del identificador, que no depende de que la fila esté.
const UN_UUID: &str = "00000000-0000-4000-8000-000000000000";

/// Las cinco rutas de un recurso, con su método.
///
/// Se construyen en vez de escribirse a mano para que las tres pruebas que las
/// recorren —la del *gate*, la del modo de desarrollo y la del método no
/// declarado— no puedan discrepar entre sí: si el §5 cambia, se cambia aquí.
fn las_cinco_rutas(recurso: &str) -> [(&'static str, String); 5] {
    [
        ("GET", format!("/api/v1/{recurso}")),
        ("POST", format!("/api/v1/{recurso}")),
        ("GET", format!("/api/v1/{recurso}/{UN_UUID}")),
        ("PUT", format!("/api/v1/{recurso}/{UN_UUID}")),
        ("DELETE", format!("/api/v1/{recurso}/{UN_UUID}")),
    ]
}

/// Los tres recursos del §5.
const LOS_TRES_RECURSOS: [&str; 3] = ["repositories", "targets", "jobs"];

/// Lanza una petición por el método dado, sin cuerpo útil.
///
/// El cuerpo da igual donde se usa: el *gate* precede al extractor, así que una
/// petición sin sesión se corta antes de que nadie mire lo que traía.
async fn sin_cuerpo(router: Router, metodo: &str, uri: &str) -> Response {
    match metodo {
        "GET" => get(router, uri).await,
        "POST" => post(router, uri).await,
        "PUT" => put_json(router, uri, &serde_json::json!({})).await,
        "DELETE" => delete(router, uri).await,
        otro => panic!("método de prueba no contemplado: {otro}"),
    }
}

/// Afirma que una respuesta es un `401` con la envoltura de la API.
async fn es_un_401_con_envoltura(respuesta: Response, donde: &str) {
    let status = respuesta.status();
    let texto = body_text(respuesta).await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "{donde} tiene que exigir sesión y contestó {status} con {texto}"
    );

    let cuerpo: serde_json::Value =
        serde_json::from_str(&texto).expect("el 401 tiene que traer JSON en el cuerpo");

    assert_eq!(cuerpo["status"], 401, "la envoltura del 401: {texto}");

    assert!(
        cuerpo["message"]
            .as_str()
            .is_some_and(|mensaje| !mensaje.is_empty()),
        "el 401 tiene que traer un mensaje: {texto}"
    );
}

/// 1.43 — En modo OIDC, las quince rutas nuevas exigen sesión.
///
/// Cubre el requisito del §5 de que toda la superficie de negocio quede detrás
/// del *gate* de sesión. Se afirma `401` con la envoltura en cada una de las cinco
/// rutas de los tres recursos —`repositories`, `targets` y `jobs`—, recorridas sin
/// cookie: la lista blanca hace que una ruta nueva quede del lado protegido **por
/// construcción**, y ésta es la prueba que lo comprueba sobre las rutas que de
/// verdad existen; una ruta que se colara al lado público no daría ningún síntoma
/// hasta que alguien leyera el registro de accesos del servidor.
///
/// El router se levanta **una sola vez**: `router_oidc()` arranca un proveedor de
/// identidad falso y no hace falta uno por petición.
#[tokio::test]
async fn en_modo_oidc_cada_ruta_nueva_exige_sesion() {
    let router = router_oidc().await;

    for recurso in LOS_TRES_RECURSOS {
        for (metodo, uri) in las_cinco_rutas(recurso) {
            let respuesta = sin_cuerpo(router.clone(), metodo, &uri).await;
            es_un_401_con_envoltura(respuesta, &format!("{metodo} {uri}")).await;
        }
    }
}

/// 1.44 — En modo de desarrollo, las rutas nuevas están montadas y no contestan `404`.
///
/// Cubre el escenario del modo `Disabled`: el modo sin proveedor **no monta la
/// mitad protegida** —`/api/v1/auth/*` ni se declara— pero **sí** la superficie de
/// recursos, y es una decisión con motivo: en ese modo no hay ninguna capa que
/// exigir, así que las rutas de negocio quedan abiertas, que es lo que el modo de
/// desarrollo significa. Se afirma `200` en el listado de cada recurso: sin esto,
/// `just dev` contestaría `404` en cada recurso y no habría forma de dar de alta
/// nada sin levantar un proveedor de identidad. Es una prueba de que las rutas
/// están montadas, y no de que estén protegidas, que es justo lo contrario de lo
/// que mira la 1.43.
#[tokio::test]
async fn en_desarrollo_las_rutas_nuevas_estan_montadas_y_no_contesta_404() {
    let router = router_desarrollo().await;

    for recurso in LOS_TRES_RECURSOS {
        let uri = format!("/api/v1/{recurso}");
        let respuesta = get(router.clone(), &uri).await;
        let status = respuesta.status();

        let texto = body_text(respuesta).await;

        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "GET {uri} no está montada en modo de desarrollo: contestó {status} con {texto}"
        );

        assert_eq!(
            status,
            StatusCode::OK,
            "GET {uri} en modo de desarrollo contestó {status} con {texto}"
        );
    }
}

/// 1.45 — Un método no declarado sobre una ruta nueva sin sesión contesta `401`.
///
/// Cubre el candado del **orden** del router: la capa de sesión precede al
/// enrutado por método. Se manda `POST` a la ruta **con identificador** de cada
/// recurso, que no declara `POST` —declara `GET`, `PUT` y `DELETE`— y sin cookie,
/// y se afirma `401` con la envoltura en las tres; en particular, que **no** sea
/// un `405`, porque un `405` sin sesión revela a quien no ha entrado qué métodos
/// existen. Ya existe una prueba igual para el flujo de entrada; ésta la fija
/// sobre las rutas nuevas, que son las que el panel va a llamar con verbos
/// distintos.
#[tokio::test]
async fn un_metodo_no_declarado_sobre_una_ruta_nueva_sin_sesion_contesta_401() {
    let router = router_oidc().await;

    for recurso in LOS_TRES_RECURSOS {
        let uri = format!("/api/v1/{recurso}/{UN_UUID}");
        let respuesta = post(router.clone(), &uri).await;
        let status = respuesta.status();
        let texto = body_text(respuesta).await;

        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "POST {uri} sin sesión contestó {status} con {texto}: un 405 revela qué métodos \
             existen a quien no ha entrado"
        );

        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "POST {uri} sin sesión tiene que exigir sesión y contestó {status} con {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 401 tiene que traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 401, "la envoltura del 401: {texto}");

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 401 tiene que traer un mensaje: {texto}"
        );
    }
}

/// 1.46 — Un identificador mal formado contesta `400` en los tres recursos.
///
/// Cubre la validación del identificador de ruta. Un identificador mal formado es
/// una petición mal formada y no un recurso ausente —un `404` le diría a quien
/// pregunta que su fila se ha borrado cuando lo que hay es una errata de teclado—
/// ni un fallo del servidor. Los dos recursos ya tienen su prueba individual, y
/// ésta fija que **los tres** contestan lo mismo, que es lo que el panel puede dar
/// por supuesto: el mismo `400` con un `message` de texto en la envoltura.
#[tokio::test]
async fn un_identificador_mal_formado_contesta_400_en_los_tres_recursos() {
    let router = router_desarrollo().await;

    for recurso in LOS_TRES_RECURSOS {
        let uri = format!("/api/v1/{recurso}/no-es-un-uuid");
        let respuesta = get(router.clone(), &uri).await;
        let status = respuesta.status();
        let texto = body_text(respuesta).await;

        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "GET {uri}: un identificador mal formado no es un recurso ausente ({texto})"
        );

        assert_ne!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "GET {uri}: un identificador mal formado no es un fallo del servidor ({texto})"
        );

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "GET {uri} contestó {status} con {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 400 tiene que traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 400, "la envoltura del 400: {texto}");

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 400 tiene que traer un mensaje de texto: {texto}"
        );
    }
}

/// Las rutas del §5 que **no** entran en la tabla de las quince, con su método.
///
/// El listado del histórico y las tres rutas nuevas del change —el disparo manual, el stream de
/// eventos y el explorador de instantáneas— cuelgan de la misma mitad de negocio por la misma lista
/// blanca. Se recorren aparte porque no encajan en el molde del resto —un recurso con sus cinco
/// verbos—: dos son rutas de un solo verbo y la del disparo es una subruta con el verbo que **no**
/// comparte con el recurso del que cuelga, que declara `GET`, `PUT` y `DELETE` con identificador y
/// no `POST`.
///
/// Se construyen en vez de escribirse en cada prueba por el mismo motivo que [`las_cinco_rutas`]:
/// las que las recorren no pueden discrepar entre sí.
fn las_cuatro_rutas_sueltas() -> [(&'static str, String); 4] {
    [
        ("GET", "/api/v1/executions".to_owned()),
        ("POST", format!("/api/v1/jobs/{UN_UUID}/run")),
        ("GET", "/api/v1/events".to_owned()),
        ("GET", "/api/v1/snapshots".to_owned()),
    ]
}

/// 1.30 — Las rutas del §5 que no están en la tabla de las quince siguen la partición del resto.
///
/// Es la prueba que comprueba que **la lista blanca no ha crecido**, que es el requisito propio de
/// las tres rutas nuevas: las tres son superficie de negocio —el disparo escribe, y el stream y el
/// explorador publican qué se está copiando y qué hay en el repositorio—, así que dejarlas públicas
/// por comodidad del panel sería publicar el estado del sistema a quien pregunte.
///
/// Las tres se **añadieron aquí**, a la prueba que ya recorría la ruta suelta del listado de
/// ejecuciones, y no en una prueba nueva: la regla se mira en un solo sitio.
///
/// # Por qué la mitad de «está montada» se queda con una sola ruta
///
/// La mitad de abajo —que en el modo sin autenticación la ruta exista— se deja como estaba, sobre
/// el listado, porque es lo que esta prueba medía y no es su objeto: el objeto es la lista blanca, y
/// eso lo mide el `401` de arriba.
///
/// Y no se extiende a las tres por un motivo distinto en cada una: el stream y el explorador ya
/// tienen su comprobación de montaje donde les corresponde —en `api::events` y en
/// `api::snapshots`—, y en el **disparo no se puede medir por el código**: `POST /api/v1/jobs/<id>/run`
/// sobre una tarea que no existe contesta `404` tanto si la ruta no está montada como si lo está y
/// la tarea no está, así que un `assert_ne!(404)` ahí pasaría hoy sin medir nada y seguiría
/// pasando mañana por el mismo azar. Lo único que se mide en las tres, y en el disparo lo único que
/// se puede medir, es el `401` sin sesión: la capa de sesión precede al manejador, así que un `401`
/// solo lo da una ruta que está detrás de ella.
#[tokio::test]
async fn las_rutas_del_srs_que_no_estan_en_la_tabla_de_las_quince_siguen_la_particion_del_resto() {
    // Sin cookie, en modo `Oidc`: la puerta corta antes que el manejador. El router se levanta una
    // sola vez y se clona: `router_oidc()` arranca un proveedor de identidad falso y no hace falta
    // uno por petición.
    let router = router_oidc().await;

    for (metodo, uri) in las_cuatro_rutas_sueltas() {
        let respuesta = sin_cuerpo(router.clone(), metodo, &uri).await;
        es_un_401_con_envoltura(respuesta, &format!("{metodo} {uri} sin sesión")).await;
    }

    // Con la autenticación apagada, la ruta está **montada**: `200`, no `404`.
    let respuesta = get(router_desarrollo().await, "/api/v1/executions").await;
    let status = respuesta.status();
    let texto = body_text(respuesta).await;

    assert_eq!(
        status,
        StatusCode::OK,
        "en el modo sin autenticación las rutas de recursos están montadas y contestan, \
         no dan 404; contestó {status} con {texto}"
    );
}
