//! Servido del panel compilado (`ui/dist`) en el mismo origen que la API.
//!
//! El panel es una aplicación de una sola página: su documento y sus assets se sirven desde el
//! mismo binario que `/api/v1`, y cualquier ruta del cliente que no sea un fichero se resuelve con
//! `index.html`. La composición vive **aquí** y no en [`crate::api::api_router`] a propósito: el
//! router de la API es el objeto que las pruebas de `api-shell` conocen, y el panel no puede
//! cambiar por accidente el contrato de `/api/v1`.
//!
//! # Dónde está el panel
//!
//! En [`PANEL_POR_DEFECTO`], relativa al directorio de trabajo del proceso. No hay variable de
//! entorno en este change: el despliegue deja `ui/dist` al lado del binario y arranca desde ahí.
//! La ruta entra por parámetro en [`app`] para que las pruebas usen un directorio temporal sin
//! abrir un puerto ni tocar el entorno.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, Method, header};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt;
use tower_http::services::ServeFile;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::state::AppState;

/// Ruta del panel compilado, relativa al directorio de trabajo del proceso.
pub const PANEL_POR_DEFECTO: &str = "ui/dist";

/// El documento que resuelve toda ruta del cliente que no sea un fichero.
const DOCUMENTO: &str = "index.html";

/// Política de caché de un asset con hash en el nombre.
///
/// Un año y `immutable`: el nombre cambia cuando cambia el contenido, así que el navegador no
/// tiene por qué volver a preguntar. Es lo que evita que un despliegue nuevo descargue otra vez
/// todo el bundle.
const CACHE_DEL_ASSET: &str = "public, max-age=31536000, immutable";

/// Política de caché del documento.
///
/// `no-cache` **no** significa «no guardes»: significa «revalida antes de usarlo». Es lo que hace
/// que un despliegue nuevo no deje a un navegador con un `index.html` viejo apuntando a assets que
/// ya no existen.
const CACHE_DEL_DOCUMENTO: &str = "no-cache";

/// Compone el servidor completo: la API y, detrás, el panel.
///
/// El panel entra como **fallback** del router de la API, y no como rutas propias, por dos
/// motivos. El primero es que así no puede tapar ninguna ruta declarada: lo que el router de la
/// API resuelve —incluido su `405` por método y su `401` por sesión— se atiende antes de llegar
/// aquí. El segundo es que el fallback de la API ya contestaba `404` a toda ruta desconocida, y
/// sustituirlo por éste conserva ese desenlace para `/api/` —lo devuelve el guardia de prefijo—
/// sin que el router de la API cambie una línea.
///
/// `panel` es el directorio del panel compilado. Entra por parámetro para que las pruebas usen un
/// directorio temporal; el arranque le pasa [`PANEL_POR_DEFECTO`].
pub fn app(state: AppState, panel: impl Into<PathBuf>) -> Router {
    let panel = Arc::new(panel.into());

    let router = crate::api::api_router(state).fallback(move |peticion: Request| {
        let panel = Arc::clone(&panel);

        async move { atender(&panel, peticion).await }
    });

    con_cabeceras_de_seguridad(router)
}

/// Añade las cabeceras de seguridad a las respuestas del servidor.
///
/// Son cuatro, y responden a lo que un navegador hace con esta superficie —el panel pinta HTML y
/// ejecuta el JavaScript de sus assets— y no a lo que espera un cliente de datos:
///
/// - `X-Content-Type-Options: nosniff` corta el sniffing de tipo: sin ella un navegador puede
///   ejecutar como script un asset servido con un `Content-Type` que no lo es.
/// - `Content-Security-Policy: frame-ancestors 'none'` y `X-Frame-Options: DENY` prohíben
///   enmarcar el panel. Las dos, y no una: `frame-ancestors` es la que manda hoy, y `DENY` es lo
///   que todavía miran los navegadores que no la implementan. La política se limita a
///   `frame-ancestors` a propósito —una `default-src` inventada aquí rompería el panel, que
///   carga sus propios assets y pide estilos en línea— y se amplía cuando haya algo que
///   restringir de verdad.
/// - `Referrer-Policy: no-referrer` evita que la URL del panel, que puede llevar el identificador
///   de una tarea o de una ejecución, viaje en el `Referer` de una salida a otro origen.
///
/// # Por qué la capa envuelve el router entero
///
/// Porque estas cabeceras describen la respuesta HTTP de este origen, y este origen sirve dos
/// mitades. Una capa colgada del *fallback* cubriría lo que el panel produce —el documento, los
/// assets y también los `404` de `/api/` de su guardia de prefijo, porque el guardia corre
/// **dentro** del propio *fallback*—, pero dejaría fuera lo que contesta el router de la API por
/// sus rutas declaradas: `/api/v1/health` saldría sin `nosniff` ni política de enmarcado, y las
/// cabeceras de una misma superficie dependerían de qué mitad atendió la petición. Puesta
/// **alrededor** del router compuesto, la política es una sola para todo lo que sale de aquí.
/// Eso es lo que la spec declara y lo que el test de las cabeceras comprueba sobre las tres
/// formas de respuesta: el documento, un asset y una ruta de la API.
///
/// Lo que la capa **no** toca: el `Content-Type` y el `Cache-Control` de cada respuesta. Cada
/// `SetResponseHeaderLayer` fija **su** cabecera y ninguna otra, así que la decisión de tipo y de
/// caché del servido de ficheros (`ServeFile` y [`cache_de`]) sigue siendo suya.
fn con_cabeceras_de_seguridad(router: Router) -> Router {
    router
        .layer(cabecera(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(cabecera(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("frame-ancestors 'none'"),
        ))
        .layer(cabecera(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(cabecera(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
}

/// Una capa que fija una cabecera de seguridad en toda respuesta.
///
/// `overriding` y no `if_not_present`: son decisiones de este módulo y no un relleno de último
/// recurso, así que si algo las pusiera antes, manda ésta. Se dice explícitamente porque con
/// `if_not_present` el valor que llega al navegador lo elegiría otro y aquí no se vería.
fn cabecera(nombre: HeaderName, valor: HeaderValue) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(nombre, valor)
}

/// Atiende una petición que el router de la API ha declinado.
///
/// El orden de las comprobaciones es el contrato: primero la API —que nunca cae en el panel—,
/// después el método —que solo admite lectura—, después el camino —que no puede salir del
/// directorio— y solo entonces el sistema de ficheros.
async fn atender(panel: &Path, peticion: Request) -> Response {
    let camino = peticion.uri().path().trim_start_matches('/');

    if es_de_la_api(camino) {
        return crate::api::not_found().await.into_response();
    }

    if peticion.method() != Method::GET && peticion.method() != Method::HEAD {
        return crate::api::metodo_no_permitido().await.into_response();
    }

    let Some(relativa) = camino_seguro(camino) else {
        return crate::api::not_found().await.into_response();
    };

    let fichero = panel.join(&relativa);

    if !relativa.as_os_str().is_empty() && es_fichero(&fichero).await {
        return servir(&fichero, peticion, cache_de(&fichero)).await;
    }

    let documento = panel.join(DOCUMENTO);

    if es_fichero(&documento).await {
        return servir(&documento, peticion, CACHE_DEL_DOCUMENTO).await;
    }

    // Sin panel compilado el arranque sigue en pie y la petición contesta el `404` de la casa:
    // un backend sin `ui/dist` tiene que compilar, arrancar y pasar su suite.
    crate::api::not_found().await.into_response()
}

/// ¿La ruta pertenece a la API?
///
/// Se compara por segmento y no por prefijo de texto: `/apizar` no es de la API, y `/api` a secas
/// tampoco es una ruta del panel que haya que servir.
fn es_de_la_api(camino: &str) -> bool {
    camino == "api" || camino.starts_with("api/")
}

/// El camino relativo dentro del panel, o `None` si intenta salir de él.
///
/// Se rechaza **antes** de tocar el sistema de ficheros: un componente `..`, un segmento que
/// empiece por `.` —los ficheros ocultos, y el propio `.`— y cualquier separador que no sea el de
/// la URI. Lo que no se rechaza aquí no llega a `join`, así que no hay ninguna ruta construida
/// que pueda apuntar fuera del directorio.
fn camino_seguro(camino: &str) -> Option<PathBuf> {
    let mut relativa = PathBuf::new();

    for segmento in camino.split('/').filter(|segmento| !segmento.is_empty()) {
        if segmento.starts_with('.') || segmento.contains('\\') {
            return None;
        }

        relativa.push(segmento);
    }

    Some(relativa)
}

/// ¿Hay un fichero regular en esa ruta?
///
/// Un fallo al preguntar —no existe, no se puede leer— se trata como «no hay fichero»: el
/// desenlace es el mismo `404` de la casa y no hay nada que distinguir para quien pregunta.
async fn es_fichero(ruta: &Path) -> bool {
    tokio::fs::metadata(ruta)
        .await
        .map(|metadatos| metadatos.is_file())
        .unwrap_or(false)
}

/// La política de caché que le corresponde a un fichero del panel.
///
/// Un asset con hash en el nombre es inmutable; todo lo demás —el documento, y cualquier fichero
/// sin hash— exige revalidación. El hash se reconoce por la forma que produce Vite: el último
/// trozo del nombre, separado por `-`, es alfanumérico y largo. Es una heurística y se dice: un
/// fichero sin hash cacheado un año sería un despliegue que no llega a los navegadores.
fn cache_de(ruta: &Path) -> &'static str {
    let con_hash = ruta
        .file_stem()
        .and_then(|nombre| nombre.to_str())
        .and_then(|nombre| nombre.rsplit_once('-'))
        .is_some_and(|(_, hash)| {
            hash.len() >= 6 && hash.chars().all(|c| c.is_ascii_alphanumeric())
        });

    if con_hash {
        CACHE_DEL_ASSET
    } else {
        CACHE_DEL_DOCUMENTO
    }
}

/// Sirve un fichero con su tipo y su política de caché.
///
/// El tipo lo adivina `ServeFile` por la extensión, y con él llegan `HEAD` y las peticiones
/// condicionales sin una segunda implementación aquí. La cabecera de caché se pone **después**:
/// es la decisión de este módulo, no la del servidor de ficheros.
async fn servir(ruta: &Path, peticion: Request, cache: &'static str) -> Response {
    let respuesta = match ServeFile::new(ruta).oneshot(peticion).await {
        Ok(respuesta) => respuesta,
        // `ServeFile` no falla: su error es `Infallible`. La rama existe para no tener que
        // afirmarlo con un `unwrap` en producción.
        Err(nunca) => match nunca {},
    };

    let mut respuesta = respuesta.map(Body::new);

    respuesta
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));

    respuesta
}

#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, header};

    use super::{PANEL_POR_DEFECTO, app};
    use crate::test_support::{
        DirectorioDePrueba, body_json, body_text, directorio_de_prueba, estado_desarrollo,
        estado_oidc, get, header_text, post,
    };

    /// El documento del panel de mentira: distinguible para poder afirmar que se sirve **él**.
    const DOCUMENTO: &str = "<!doctype html><html><body><div id=\"root\">panel</div></body></html>";

    /// El asset de mentira, con un hash en el nombre como los que produce Vite.
    const ASSET: &str = "console.log(\"panel\");\n";

    /// Un `ui/dist` de mentira: el documento y un asset con hash.
    fn panel_de_prueba() -> DirectorioDePrueba {
        let directorio = directorio_de_prueba();

        std::fs::create_dir_all(directorio.ruta().join("assets"))
            .expect("el directorio de assets de la prueba se crea");
        std::fs::write(directorio.ruta().join("index.html"), DOCUMENTO)
            .expect("el documento de la prueba se escribe");
        std::fs::write(directorio.ruta().join("assets/app-abc123.js"), ASSET)
            .expect("el asset de la prueba se escribe");

        directorio
    }

    /// La ruta por defecto es la que el despliegue deja al lado del binario.
    #[test]
    fn la_ruta_por_defecto_es_la_del_panel_compilado() {
        assert_eq!(PANEL_POR_DEFECTO, "ui/dist");
    }

    /// Escenario «La raíz sirve el documento del panel».
    #[tokio::test]
    async fn la_raiz_sirve_el_documento() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        let respuesta = get(router, "/").await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let tipo = header_text(&respuesta, header::CONTENT_TYPE).unwrap_or_default();
        assert!(
            tipo.starts_with("text/html"),
            "la raíz tiene que contestar HTML: {tipo}"
        );

        assert_eq!(body_text(respuesta).await, DOCUMENTO);
    }

    /// Escenario «Una ruta del cliente se resuelve con el fallback».
    #[tokio::test]
    async fn una_ruta_del_cliente_se_resuelve_con_el_documento() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        let respuesta = get(router, "/tareas").await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "una ruta del cliente no puede contestar 404"
        );

        let tipo = header_text(&respuesta, header::CONTENT_TYPE).unwrap_or_default();
        assert!(
            tipo.starts_with("text/html"),
            "el fallback tiene que contestar HTML: {tipo}"
        );

        assert_eq!(body_text(respuesta).await, DOCUMENTO);
    }

    /// Escenario «Un asset existente se sirve con su tipo».
    #[tokio::test]
    async fn un_asset_existente_se_sirve_con_su_tipo() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        let respuesta = get(router, "/assets/app-abc123.js").await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let tipo = header_text(&respuesta, header::CONTENT_TYPE).unwrap_or_default();
        assert!(
            tipo.contains("javascript"),
            "el asset tiene que servirse como JavaScript: {tipo}"
        );

        assert_eq!(body_text(respuesta).await, ASSET);
    }

    /// Escenario «Un asset con hash se cachea como inmutable y el documento no».
    #[tokio::test]
    async fn el_asset_con_hash_se_cachea_como_inmutable_y_el_documento_no() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        let asset = get(router.clone(), "/assets/app-abc123.js").await;
        let documento = get(router, "/").await;

        let cache_del_asset = header_text(&asset, header::CACHE_CONTROL).unwrap_or_default();
        let cache_del_documento =
            header_text(&documento, header::CACHE_CONTROL).unwrap_or_default();

        assert!(
            cache_del_asset.contains("immutable") && cache_del_asset.contains("max-age=31536000"),
            "el asset con hash tiene que cachearse como inmutable: {cache_del_asset:?}"
        );
        assert!(
            cache_del_documento.contains("no-cache"),
            "el documento tiene que exigir revalidación: {cache_del_documento:?}"
        );
        assert_ne!(
            cache_del_asset, cache_del_documento,
            "el asset y el documento no pueden compartir política de caché"
        );
    }

    /// Escenario «Las cabeceras de seguridad acompañan a toda respuesta del origen».
    ///
    /// La capa envuelve el router **compuesto**, así que la política es la misma para las dos
    /// mitades: por eso las tres rutas del recorrido incluyen una de la API, que es la que
    /// distingue esta decisión de una capa colgada solo del *fallback*.
    #[tokio::test]
    async fn las_respuestas_del_servidor_llevan_las_cabeceras_de_seguridad() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        // Las tres formas de respuesta del origen: el documento, un asset y una ruta de la API.
        for ruta in ["/", "/assets/app-abc123.js", "/api/v1/health"] {
            let respuesta = get(router.clone(), ruta).await;

            assert_eq!(respuesta.status(), StatusCode::OK, "la ruta {ruta}");

            for (nombre, esperada) in [
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::X_FRAME_OPTIONS, "DENY"),
                (header::REFERRER_POLICY, "no-referrer"),
            ] {
                assert_eq!(
                    header_text(&respuesta, nombre.clone()).as_deref(),
                    Some(esperada),
                    "la ruta {ruta} no lleva la cabecera {} con el valor {esperada}",
                    nombre.as_str()
                );
            }

            let csp = header_text(&respuesta, header::CONTENT_SECURITY_POLICY).unwrap_or_default();
            assert!(
                csp.contains("frame-ancestors 'none'"),
                "la ruta {ruta} no prohíbe enmarcarse: {csp:?}"
            );
        }

        // Y la capa no cambia lo que ya había: el documento sigue siendo el documento, el asset
        // sigue siendo el asset y la salud sigue contestando su envoltura con su `200`.
        assert_eq!(body_text(get(router.clone(), "/").await).await, DOCUMENTO);
        assert_eq!(
            body_text(get(router.clone(), "/assets/app-abc123.js").await).await,
            ASSET
        );

        let salud = get(router, "/api/v1/health").await;

        assert_eq!(salud.status(), StatusCode::OK);
        assert_eq!(body_json(salud).await["data"]["auth_mode"], "disabled");
    }

    /// Escenario «Una ruta de la API no cae en el fallback».
    #[tokio::test]
    async fn una_ruta_de_la_api_no_cae_en_el_fallback() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        let respuesta = get(router, "/api/v1/no-existe").await;

        assert_eq!(respuesta.status(), StatusCode::NOT_FOUND);

        let texto = body_text(respuesta).await;

        assert!(
            !texto.contains("id=\"root\""),
            "la ruta de la API sirvió el documento del panel: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 404 de la API debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 404);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 404 debe traer un mensaje: {texto}"
        );
    }

    /// Escenario «Una ruta de la API sin sesión no se sirve por el panel».
    #[tokio::test]
    async fn una_ruta_de_la_api_sin_sesion_no_se_sirve_por_el_panel() {
        let panel = panel_de_prueba();
        let router = app(estado_oidc().await, panel.ruta());

        let respuesta = get(router, "/api/v1/jobs").await;

        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);

        let texto = body_text(respuesta).await;

        assert!(
            !texto.contains("id=\"root\""),
            "el panel sirvió el documento en vez de exigir sesión: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 401 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 401);
    }

    /// Escenario «No se sale del directorio del panel».
    #[tokio::test]
    async fn no_se_sale_del_directorio_del_panel() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        for ruta in ["/../Cargo.toml", "/assets/../../Cargo.toml", "/.env"] {
            let respuesta = get(router.clone(), ruta).await;

            assert!(
                respuesta.status() == StatusCode::NOT_FOUND
                    || respuesta.status() == StatusCode::BAD_REQUEST,
                "la ruta {ruta} contestó {} y tenía que rechazarse",
                respuesta.status()
            );

            let texto = body_text(respuesta).await;

            assert!(
                !texto.contains("[package]") && !texto.contains("id=\"root\""),
                "la ruta {ruta} entregó un fichero de fuera del panel: {texto}"
            );
        }
    }

    /// Escenario «Las formas codificadas del traversal se quedan dentro del panel».
    ///
    /// Lo que se afirma es el desenlace **completo**, no la ausencia de una cadena: el candado de
    /// [`camino_seguro`] no decodifica nada, así que `%2e%2e%2f` y `%2f` no llevan ni una barra ni
    /// un punto de verdad y la URI entera cae como ruta del cliente —`200` con `index.html`—. Eso
    /// es lo que hace que la prueba no sea vacua: si alguien decodificara antes de unir, ese
    /// `%2e%2e%2fCargo.toml` sería `../Cargo.toml`, el fichero existiría y aquí se serviría, en vez
    /// del documento. Un `!contains("[package]")` a secas pasaría igual sin candado ninguno,
    /// porque esos nombres nunca existen en disco.
    ///
    /// Y `..%2f` sí empieza por punto, así que lo rechaza el candado **sin** tocar el sistema de
    /// ficheros: `404` con la envoltura.
    #[tokio::test]
    async fn las_formas_codificadas_del_traversal_se_quedan_dentro_del_panel() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        for ruta in ["/%2e%2e%2fCargo.toml", "/%2fCargo.toml"] {
            let respuesta = get(router.clone(), ruta).await;

            assert_eq!(
                respuesta.status(),
                StatusCode::OK,
                "la ruta {ruta} no se decodifica: es una ruta del cliente"
            );
            assert_eq!(
                body_text(respuesta).await,
                DOCUMENTO,
                "la ruta {ruta} entregó algo que no es el documento del panel"
            );
        }

        let rechazada = get(router, "/..%2fCargo.toml").await;

        assert_eq!(
            rechazada.status(),
            StatusCode::NOT_FOUND,
            "una ruta que empieza por punto no puede pasar del candado"
        );
        assert!(
            !body_text(rechazada).await.contains("[package]"),
            "la ruta con el separador codificado entregó un fichero de fuera del panel"
        );
    }

    /// El `404` del panel es el de la API: una sola verdad del contrato.
    ///
    /// El texto no vive en el panel —se consume [`crate::api::not_found`]—, así que esta prueba
    /// comprueba el desenlace que el cliente ve: la ruta que el panel declina por salirse del
    /// directorio y la ruta de la API que no existe contestan el mismo mensaje. Con una segunda
    /// copia del literal en el panel, un cambio en el de la API dejaría mudas a las dos mitades
    /// por caminos distintos, que es justo lo que esta prueba delata.
    #[tokio::test]
    async fn el_404_del_panel_es_el_de_la_api() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        let de_la_api = body_json(get(router.clone(), "/api/v1/no-existe").await).await;
        let del_panel = body_json(get(router, "/../Cargo.toml").await).await;

        assert_eq!(
            de_la_api["message"], del_panel["message"],
            "el `404` del panel y el de la API tienen que compartir mensaje"
        );
        assert_eq!(de_la_api["status"], del_panel["status"]);
        // Y el valor, no solo la coincidencia: sin esto el `404` podría degradarse a cualquier
        // otro código en las dos mitades a la vez y la prueba seguiría verde.
        assert_eq!(del_panel["status"], 404);
    }

    /// Escenario «Sin panel compilado el backend sigue en pie».
    #[tokio::test]
    async fn sin_panel_el_servidor_sigue_en_pie() {
        let directorio = directorio_de_prueba();
        let inexistente = directorio.ruta().join("no-hay-dist");
        let router = app(estado_desarrollo().await, inexistente);

        let respuesta = get(router, "/").await;

        assert_eq!(respuesta.status(), StatusCode::NOT_FOUND);

        let texto = body_text(respuesta).await;
        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 404 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 404);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 404 debe traer un mensaje: {texto}"
        );
    }

    /// Escenario «Los métodos del fallback se limitan a lectura».
    #[tokio::test]
    async fn los_metodos_del_fallback_se_limitan_a_lectura() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        let respuesta = post(router, "/tareas").await;

        assert_eq!(respuesta.status(), StatusCode::METHOD_NOT_ALLOWED);

        let texto = body_text(respuesta).await;

        assert!(
            !texto.contains("id=\"root\""),
            "el 405 sirvió el documento del panel: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 405 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 405);
    }

    /// La salud sigue contestando su envoltura con el panel delante.
    #[tokio::test]
    async fn la_salud_sigue_contestando_la_envoltura() {
        let panel = panel_de_prueba();
        let router = app(estado_desarrollo().await, panel.ruta());

        let respuesta = get(router, "/api/v1/health").await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["data"]["auth_mode"], "disabled");
    }
}
