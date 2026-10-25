//! API HTTP de Kesto: el router de `/api/v1` y la partición de su superficie.

pub mod auth;
pub mod executions;
pub mod extract;
pub mod health;
pub mod jobs;
pub mod repositories;
pub mod response;
pub mod targets;

#[cfg(test)]
mod secretos;

#[cfg(test)]
mod particion;

use axum::Router;
use axum::routing::{get, post};
use serde::Serialize;

use crate::api::response::Data;
use crate::auth::middleware;
use crate::error::AppError;
use crate::state::{AppState, AuthState};

/// Deja constancia de que una petición se atendió, sin su cuerpo.
///
/// Lo exige el requisito «Un manejador no registra el cuerpo de la petición» del delta `secrets`:
/// el registro tiene que decir **qué** ocurrió —el método, la ruta y el código de la respuesta, que
/// llevan la identidad del recurso afectado— y **nunca** el contenido del cuerpo, que es por donde
/// viajan las credenciales y que, mientras está en crudo, no pasa por ningún `SecretString` que lo
/// redacte.
///
/// Va a **`debug`** y no a `info`: una línea por petición atendida es ruido de operación —el panel
/// manda una petición por clic—, y a `debug` está disponible cuando hace falta sin inundar el
/// registro que se lee de verdad.
///
/// Va en **un solo sitio** —una capa sobre el router— y no en cada manejador: son quince rutas, y
/// quince llamadas a mano son quince sitios donde olvidarse. Aquí cubre por construcción cualquier
/// ruta que se declare mañana.
///
/// Se registra **después** de atender, con el código de la respuesta, para que la línea diga el
/// desenlace y no solo el intento. La ruta se registra tal cual llegó: en las rutas del §5 el
/// identificador es un UUID, y no hay ningún otro dato del cliente en la URI.
async fn trazar(
    peticion: axum::extract::Request,
    siguiente: axum::middleware::Next,
) -> axum::response::Response {
    let metodo = peticion.method().clone();
    let ruta = peticion.uri().path().to_owned();

    let respuesta = siguiente.run(peticion).await;

    tracing::debug!(
        %metodo,
        %ruta,
        estado = respuesta.status().as_u16(),
        "petición atendida"
    );

    respuesta
}

/// Router de `/api/v1`.
///
/// La superficie se parte en dos mitades por **lista blanca**: una pública —la
/// salud y las dos rutas del flujo de entrada— y una protegida con el resto de
/// la superficie de negocio, a la que el *gate* de sesión se cuelga con
/// `route_layer` para que no alcance a las públicas.
///
/// La lista blanca y no una lista negra: así una ruta nueva queda del lado
/// protegido por construcción, sin que nadie tenga que acordarse. El SSE de
/// `/api/v1/events` quedará detrás del *gate* el día que su change lo declare.
pub fn api_router(state: AppState) -> Router {
    // La salud es lo único público en cualquier modo.
    let mut public: Router<AppState> = Router::new().route("/api/v1/health", get(health::health));

    let router = match &state.auth {
        // Con proveedor, el flujo de entrada es público y el resto, protegido.
        Some(auth) => {
            public = public
                .route("/api/v1/auth/login", get(auth::login))
                .route("/api/v1/auth/callback", get(auth::callback));

            // La superficie de negocio: las rutas del flujo de entrada y las de los
            // recursos, **declaradas aquí** y no en la mitad pública. No hay ninguna
            // excepción a la lista blanca: una ruta de negocio cuelga de esta mitad por
            // construcción, así que el *gate* de sesión la alcanza sin que nadie tenga
            // que acordarse de colgarlo.
            let negocio = Router::new()
                .route("/api/v1/auth/logout", post(auth::logout))
                .route("/api/v1/auth/session", get(auth::session))
                .merge(repositories::router())
                .merge(targets::router())
                .merge(jobs::router())
                .merge(executions::router());

            public.merge(protected_router(negocio, auth))
        }
        // En desarrollo no se monta la mitad protegida —no hay proveedor, así que
        // `/api/v1/auth/*` **no** se declara y contesta el `404` de la envoltura en vez
        // de un `401`—, pero **sí** la superficie de recursos: en este modo no se monta
        // ninguna capa, así que las rutas de negocio quedan públicas, que es lo que el
        // modo de desarrollo significa. Sin esto, `just dev` contestaría `404` en cada
        // recurso y no habría forma de dar de alta nada sin un proveedor de identidad.
        None => public
            .merge(repositories::router())
            .merge(targets::router())
            .merge(jobs::router())
            .merge(executions::router()),
    };

    router
        // Dos fallbacks, no uno: el de rutas cubre lo que no está declarado, y
        // el de métodos cubre lo que sí lo está con otro verbo. Sin el segundo,
        // axum contesta `405` con el cuerpo vacío y rompe la envoltura única.
        //
        // Este `method_not_allowed_fallback` global solo alcanza a la mitad
        // pública: sobre las rutas protegidas no tiene efecto, porque su
        // *fallback* ya no es el de por defecto —lo sustituyó la mitad
        // protegida— y sustituir solo se sustituye el de por defecto.
        .fallback(not_found)
        .method_not_allowed_fallback(metodo_no_permitido)
        .layer(axum::middleware::from_fn(trazar))
        .with_state(state)
}

/// La mitad protegida: las rutas de negocio, con el *gate* de sesión colgado.
///
/// Toma las rutas ya construidas en vez de declararlas aquí. Es lo que hace que
/// la partición sea verificable hoy: mientras los changes de negocio no
/// declaren ninguna ruta (`/targets`, `/jobs`, `/events`…), un test puede
/// colgar el *gate* sobre una ruta ficticia **por el mismo camino** que la
/// producción, sin deformar el router de producción, que la llama con las que
/// existan.
pub fn protected_router<S>(routes: Router<S>, auth: &AuthState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    routes
        // AVISO — el orden de estas dos líneas es semántico, no cosmético.
        //
        // `method_not_allowed_fallback` **sustituye** el *fallback* del
        // `MethodRouter`, y solo lo hace si el *fallback* es el de por defecto.
        // Declarado **antes** de colgar la capa, la capa envuelve también ese
        // *fallback* y el *gate* decide antes que el enrutado por método: un
        // método no declarado sobre una ruta protegida contesta `401` sin
        // sesión.
        //
        // Declarado **después**, sustituye un *fallback* que ya estaba envuelto
        // por otro que nunca vio la capa —el envoltorio conserva la variante
        // «por defecto», y por eso la sustitución posterior lo pisa—, y ese
        // `405` sale **sin haber exigido sesión**, revelando a quien no ha
        // entrado qué métodos existen.
        //
        // Invertir las dos líneas no rompe la compilación ni hace caer ninguna
        // prueba de ruta válida: solo abre ese agujero. Lo cierran **dos** tests
        // —medido invirtiendo las líneas y corriendo la suite entera, con
        // `54 passed; 2 failed`—:
        //
        // - `un_metodo_no_declarado_sin_sesion_no_contesta_405` (tarea 1.42),
        //   aquí en este módulo;
        // - `sin_sesion_no_se_aprende_que_metodos_hay` (tarea 1.45), en
        //   `src/api/auth.rs`, sobre `GET /api/v1/auth/logout`.
        //
        // Los dos con el mismo `left: 405, right: 405`. Si se filtran los tests
        // por nombre antes de correrlos, el de 1.45 se queda fuera del filtro
        // —su nombre no menciona el método no declarado— y parece que el candado
        // es más débil de lo que es: se comprueba con la suite entera.
        //
        // Y lo que **no** sirve de candado, porque pasa en los dos órdenes: el
        // test `un_metodo_no_declarado_con_sesion_contesta_405` (tarea 1.43).
        // Con sesión el `405` sale igual por el *fallback* envuelto que por el
        // que lo pisó, así que su verde no dice nada del orden.
        .method_not_allowed_fallback(metodo_no_permitido)
        .route_layer(axum::middleware::from_fn_with_state(
            auth.clone(),
            middleware::gate,
        ))
}

/// Fallback de las rutas desconocidas: `404` con la envoltura de error.
///
/// Una ruta **no declarada** no pasa por el *gate* —`route_layer` no envuelve el
/// *fallback* que las resuelve— y contesta `404` sin exigir sesión. Es correcto
/// y no hay que «arreglarlo» envolviéndolo a mano: un `404` no revela ningún
/// recurso ni ningún método, así que no hay nada que proteger detrás, y forzar
/// el *gate* ahí acoplaría las dos mitades y rompería la partición.
async fn not_found() -> AppError {
    AppError::NotFound("la ruta solicitada no existe".to_owned())
}

/// Fallback de los métodos no declarados: `405` con la envoltura de error.
async fn metodo_no_permitido() -> AppError {
    AppError::MethodNotAllowed("el método no está permitido en esta ruta".to_owned())
}

/// Serializa una vista del cable al `data` de la envoltura.
///
/// Vive aquí, y no en cada módulo de recurso, porque **los cuatro** recursos lo necesitan
/// igual: una copia por recurso serían cuatro sitios donde arreglar el mismo fallo. Existe
/// porque `serde_json::to_value` devuelve `Result` y un manejador no puede abortar con
/// `panic!`: un fallo aquí es un `500` con la causa al registro, que es lo que el resto de la
/// API hace con lo que no debería ocurrir.
///
/// El mensaje **no** lleva el valor serializado, y eso no es una garantía de `serde_json`
/// sino de estos tipos: `serde_json` mete en su error el texto que le dé un serializador
/// propio —`ser::Error::custom`—, así que la propiedad se sostiene mientras ninguna vista del
/// cable declare un `serialize_with` que formatee un valor. La red de verdad es de antes: una
/// respuesta que llevara un secreto no compila, porque [`crate::secret::SecretString`] no
/// implementa `Serialize`.
pub(crate) fn contenido<T: Serialize>(vista: &T) -> Result<Data, AppError> {
    serde_json::to_value(vista)
        .map(Data::Some)
        .map_err(|error| AppError::Other(format!("la respuesta no se pudo serializar: {error}")))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::Router;
    use axum::http::{StatusCode, header};

    use super::{api_router, protected_router};
    use crate::api::response::{ApiResponse, Data};
    use crate::test_support::{
        auth_oidc, body_json, body_text, cookie_de_sesion, estado_desarrollo, estado_oidc, get,
        get_with_cookie, header_text, post, post_with_cookie, router_desarrollo, router_oidc,
    };

    /// Ruta de negocio ficticia: hoy ningún change declara rutas de negocio, así
    /// que la partición se prueba con esta, colgada de la mitad protegida por el
    /// mismo camino que la producción.
    async fn negocio() -> ApiResponse {
        ApiResponse::new(StatusCode::OK, "atendida", Data::None)
    }

    #[tokio::test]
    async fn los_estados_de_prueba_no_heredan_el_puerto_por_defecto() {
        // Es un candado del arnés, no del router, y nace de una regresión real:
        // `crate::serve` abre un socket de verdad, así que un estado de prueba
        // que herede el puerto por defecto pone la suite roja por `AddrInUse` en
        // cuanto algo ocupe el 8080 —el propio Kesto en desarrollo lo ocupa—, y
        // eso es un fallo del entorno disfrazado de fallo del código. El puerto
        // `0` es el efímero: lo elige el sistema y no hay que acordarse test a
        // test.
        for (modo, estado) in [
            ("desarrollo", estado_desarrollo().await),
            ("Oidc", estado_oidc().await),
        ] {
            assert_eq!(
                estado.config.bind_address.port(),
                0,
                "el estado de {modo} escucha en {} y no en un puerto efímero",
                estado.config.bind_address
            );
        }
    }

    #[tokio::test]
    async fn en_modo_oidc_el_flujo_protegido_no_se_atiende_sin_sesion() {
        // Escenario 1.2.
        let respuesta = get(router_oidc().await, "/api/v1/auth/session").await;

        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 401 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 401 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 401);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 401 debe traer un mensaje: {texto}"
        );
    }

    #[tokio::test]
    async fn en_desarrollo_el_flujo_no_esta_montado_y_contesta_404() {
        // Escenario 1.1.
        let respuesta = get(router_desarrollo().await, "/api/v1/auth/session").await;

        assert_ne!(
            respuesta.status(),
            StatusCode::UNAUTHORIZED,
            "sin proveedor no hay capa de autenticación que exigir"
        );
        assert_eq!(respuesta.status(), StatusCode::NOT_FOUND);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 404 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 404 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 404);
    }

    #[tokio::test]
    async fn la_salud_sigue_siendo_publica_con_la_autenticacion_encendida() {
        // Escenario 1.3.
        let respuesta = get(router_oidc().await, "/api/v1/health").await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["data"]["auth_mode"], "oidc");
    }

    #[tokio::test]
    async fn la_superficie_publica_es_una_lista_blanca() {
        // Escenario 1.4.
        //
        // La lista blanca se comprueba por los dos lados, y el segundo es el que
        // de verdad fija la regla:
        //
        // 1. Las tres declaradas públicas no exigen sesión.
        // 2. Cualquier otra ruta declarada queda detrás del *gate*, cuelgue de
        //    donde cuelgue. Se prueba con `/api/v1/events` —el SSE que el SRS
        //    quiere detrás del *gate*— sobre la misma mitad protegida que usa la
        //    producción: contesta `401` sin cookie y deja pasar con ella, que es
        //    exactamente «NO está en la lista blanca».
        for ruta in [
            "/api/v1/health",
            "/api/v1/auth/login",
            "/api/v1/auth/callback",
        ] {
            let respuesta = get(router_oidc().await, ruta).await;

            assert_ne!(
                respuesta.status(),
                StatusCode::UNAUTHORIZED,
                "la ruta pública {ruta} exige sesión"
            );
        }

        let eventos = protected_router(
            Router::new().route("/api/v1/events", axum::routing::get(negocio)),
            &auth_oidc().await,
        );

        let sin_cookie = get(eventos.clone(), "/api/v1/events").await;

        assert_eq!(
            sin_cookie.status(),
            StatusCode::UNAUTHORIZED,
            "el SSE no puede quedar del lado público"
        );

        let con_cookie = get_with_cookie(eventos, "/api/v1/events", &cookie_de_sesion()).await;

        assert_eq!(
            con_cookie.status(),
            StatusCode::OK,
            "con sesión el SSE atendido debe pasar"
        );
    }

    #[tokio::test]
    async fn las_rutas_publicas_no_pasan_por_el_gate_y_la_superficie_de_negocio_si() {
        // Escenario 1.41.
        let salud = get(router_oidc().await, "/api/v1/health").await;

        assert_eq!(salud.status(), StatusCode::OK);
        assert_ne!(salud.status(), StatusCode::UNAUTHORIZED);

        // Una ruta de negocio cualquiera, colgada de la mitad protegida como lo
        // estará la de verdad.
        let negocio_router = protected_router(
            Router::new().route("/api/v1/targets", axum::routing::get(negocio)),
            &auth_oidc().await,
        );

        let sin_cookie = get(negocio_router.clone(), "/api/v1/targets").await;

        assert_eq!(sin_cookie.status(), StatusCode::UNAUTHORIZED);

        let con_cookie =
            get_with_cookie(negocio_router, "/api/v1/targets", &cookie_de_sesion()).await;

        assert_eq!(
            con_cookie.status(),
            StatusCode::OK,
            "con sesión la superficie de negocio debe pasar"
        );
    }

    #[tokio::test]
    async fn un_metodo_no_declarado_sin_sesion_no_contesta_405() {
        // Escenario 1.42 — **el candado del orden del router**.
        //
        // `POST /api/v1/auth/session` solo declara `GET`. Sin cookie tiene que
        // contestar `401`, no `405`: el *gate* precede al enrutado por método.
        // Este es el único test que cae si las dos líneas de `protected_router`
        // se invierten; el de con sesión pasa en los dos órdenes.
        let respuesta = post(router_oidc().await, "/api/v1/auth/session").await;

        assert_ne!(
            respuesta.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "el 405 sin sesión revela a quien no ha entrado qué métodos existen"
        );
        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 401 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 401 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 401);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 401 debe traer un mensaje: {texto}"
        );
    }

    #[tokio::test]
    async fn un_metodo_no_declarado_con_sesion_contesta_405() {
        // Escenario 1.43.
        let respuesta = post_with_cookie(
            router_oidc().await,
            "/api/v1/auth/session",
            &cookie_de_sesion(),
        )
        .await;

        assert_eq!(respuesta.status(), StatusCode::METHOD_NOT_ALLOWED);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 405 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 405 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 405);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 405 debe traer un mensaje: {texto}"
        );
    }

    #[tokio::test]
    async fn una_ruta_no_declarada_no_pasa_por_el_gate() {
        // Escenario 1.44.
        let respuesta = get(router_oidc().await, "/api/v1/no-existe").await;

        assert_ne!(
            respuesta.status(),
            StatusCode::UNAUTHORIZED,
            "el 404 no pasa por el gate y no puede exigir sesión"
        );
        assert_eq!(respuesta.status(), StatusCode::NOT_FOUND);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 404 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 404 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 404);
    }

    #[tokio::test]
    async fn el_401_no_es_una_redireccion_ni_un_cuerpo_vacio() {
        // Escenario 1.32.
        let respuesta = get(router_oidc().await, "/api/v1/auth/session").await;

        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);
        assert!(
            !respuesta.status().is_redirection(),
            "el rechazo no puede ser un 3xx: la API contesta JSON"
        );
        assert!(
            header_text(&respuesta, header::LOCATION).is_none(),
            "el rechazo no puede traer Location"
        );

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 401 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 401 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 401);
    }

    #[tokio::test]
    async fn una_ruta_desconocida_contesta_404_con_envoltura() {
        let respuesta = get(api_router(estado_desarrollo().await), "/api/v1/no-existe").await;

        assert_eq!(respuesta.status(), StatusCode::NOT_FOUND);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 404 llegó con el cuerpo vacío");

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

    #[tokio::test]
    async fn un_metodo_no_declarado_contesta_405_con_envoltura() {
        let respuesta = post(api_router(estado_desarrollo().await), "/api/v1/health").await;

        assert_eq!(respuesta.status(), StatusCode::METHOD_NOT_ALLOWED);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 405 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 405 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 405);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 405 debe traer un mensaje: {texto}"
        );
    }

    #[tokio::test]
    async fn el_servidor_devuelve_cuando_la_senal_de_cierre_se_resuelve() {
        let (senal, espera) = tokio::sync::oneshot::channel::<()>();

        let servidor = tokio::spawn(async move {
            crate::serve(estado_desarrollo().await, async move {
                let _ = espera.await;
            })
            .await
        });

        senal.send(()).expect("la señal de cierre debe entregarse");

        let transcurrido = tokio::time::timeout(Duration::from_secs(5), servidor).await;

        transcurrido
            .expect("el servidor no devolvió al resolverse la señal de cierre")
            .expect("la tarea del servidor no debe entrar en pánico")
            .expect("el servidor debe cerrar sin error");
    }
}
