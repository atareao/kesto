//! API HTTP de Kesto: el router de `/api/v1` y la partición de su superficie.

pub mod auth;
pub mod events;
pub mod executions;
pub mod extract;
pub mod health;
pub mod jobs;
pub mod repositories;
pub mod response;
pub mod snapshots;
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
use crate::engine::ErrorDelMotor;
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
/// `/api/v1/events` está detrás del *gate* como cualquier otra ruta de negocio:
/// lo que su change declara en excepción es su **cuerpo** —no lleva la
/// envoltura—, no su sitio en el router.
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
                .merge(executions::router())
                // El explorador de instantáneas, como los tres recursos y no como el stream: su
                // respuesta lleva la envoltura, así que lo único que tiene que llegarle es el
                // *gate* de sesión, que es de esta mitad. No entra en la lista blanca.
                .merge(snapshots::router())
                // El stream de eventos cuelga de **esta** mitad y no de una tercera: en este
                // repositorio la envoltura no es una capa del router sino lo que produce un
                // manejador, así que el stream se salta la envoltura por lo que devuelve su
                // manejador —un `Sse`— y no por dónde está montado. Y lo que sí tiene que
                // llegarle es el *gate* de sesión, que es lo que lo protege, y la capa de
                // traza, que es la que deja constancia de que alguien se conectó.
                .merge(events::router());

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
            .merge(executions::router())
            // Y el explorador, igual: en este modo no hay ninguna capa que montar, así que queda
            // abierto como el resto —es lo que el modo de desarrollo significa— y el panel puede
            // explorar un repositorio sin un proveedor de identidad.
            .merge(snapshots::router())
            // Y el stream, como en la mitad protegida: en este modo no hay ninguna capa que montar,
            // así que queda abierto igual que el resto —es lo que el modo de desarrollo significa—
            // y el panel puede seguirlo sin un proveedor de identidad.
            .merge(events::router()),
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

/// El `404` de una tarea que no está, con un solo texto para las cuatro rutas que la buscan.
///
/// Lo usan la lectura, la edición, el borrado y el disparo —las cuatro que resuelven una tarea por
/// el identificador de la ruta—: el mensaje que el panel pinta no puede depender de por qué ruta se
/// buscó, así que la frase vive en un solo sitio y lo comparten la constancia de
/// [`crate::api::jobs`] y la traducción de [`ErrorDelMotor::TareaQueNoEsta`] de aquí abajo.
///
/// El texto **no** repite el valor de la ruta: la regla de la casa es que un rechazo nombre el
/// campo, nunca lo que traía.
pub(crate) const TAREA_QUE_NO_ESTA: &str = "no existe ninguna tarea con ese identificador";

/// Traduce el desenlace del motor al error de la API.
///
/// Es la **única** tabla de correspondencias entre [`ErrorDelMotor`] y el código HTTP, y vive aquí
/// por dos motivos. El primero es de sitio: es una función de la capa HTTP y la usan **todos** los
/// módulos de recurso que hablan con el motor —el disparo de una tarea hoy; el alta de un
/// repositorio y el explorador de instantáneas en los lotes siguientes—, así que su casa es el
/// módulo que ya presta lo que los cuatro recursos necesitan, como [`contenido`]. Ponerla en el
/// módulo de un recurso obligaría a los otros a importarla de un hermano, y una tabla de códigos
/// duplicada en cada manejador es una tabla que se desincroniza.
///
/// El segundo es que la correspondencia es **contrato**, y está escrita variante a variante en la
/// documentación de [`ErrorDelMotor`]: aquí solo se copia, y una variante nueva rompe este `match`
/// —no hay rama comodín— de modo que no puede quedarse sin código sin que la compilación lo diga.
///
/// # La única variante que este `match` no puede cerrar
///
/// [`ErrorDelMotor::Repositorio`] es el desenlace en el que el código **lo elige quien llama**, y
/// su documentación lo explica: el alta de un repositorio tiene que contestar `502` si el destino no
/// se alcanza y `400` si la passphrase no abre uno que ya existe, y distinguirlos exige mirar la
/// [`causa`](ErrorDelMotor::Repositorio::causa) de la biblioteca, que viaja entera y con su tipo
/// precisamente para eso. Aquí se le da el desenlace por defecto —`502`, «el repositorio no ha
/// respondido»—, que es el correcto para los caminos en los que el motor no puede decir cuál de los
/// dos fue; quien tenga que elegir mira la causa **antes** de llamar a esta función, porque el error
/// entra por valor: eso es exactamente lo que hace [`del_motor_en_el_alta`].
///
/// # Por qué el fallo de base no pasa por [`AppError::Database`]
///
/// [`ErrorDelMotor::Base`] documenta que corresponde a un `500`, y aquí se le da el genérico. La
/// tentación es reenviarlo a [`AppError::Database`] —que sabe traducir la clave foránea violada a
/// `409` y el secreto vacío a `400`—, pero esos dos desenlaces los provoca **el cuerpo de una
/// petición**, y el encolado no escribe nada con cuerpo: la fila la compone el motor con valores que
/// acaba de leer. Lo que sí llegaría por ahí es una violación de clave foránea de una tarea borrada
/// entre la lectura y el `INSERT`, que para quien disparó es un fallo del servidor y no algo que
/// pueda corregir. El contrato dice `500`, y la causa —que nombra rutas del despliegue— se queda en
/// el registro.
pub(crate) fn del_motor(error: ErrorDelMotor) -> AppError {
    // El texto del motor se guarda **antes** de mirar la variante: es el que va al registro en todas
    // las ramas, y las que publican un mensaje propio lo hacen a propósito —el `404` y el `409` no
    // repiten el valor de la ruta, y el `503` no publica su motivo—. El texto nunca lleva un
    // secreto: el motor ya pasa por su red lo que viene de fuera.
    let motivo = error.to_string();

    match error {
        // 404 — la tarea no existe. Es la misma frase que la de las otras tres rutas que buscan una
        // tarea por su identificador, y sale de la misma constante.
        ErrorDelMotor::TareaQueNoEsta { .. } => AppError::NotFound(TAREA_QUE_NO_ESTA.to_owned()),

        // 409 — la guarda atómica de la capa de acceso. El mensaje no repite el identificador: la
        // traza de la petición ya registra la ruta, y la regla de la casa es que un rechazo nombre
        // el campo y no lo que traía.
        ErrorDelMotor::YaTieneUnaEnCurso { .. } => AppError::Conflict(
            "la tarea ya tiene una ejecución en curso: no se encola una segunda".to_owned(),
        ),

        // 503 — el motor no acepta trabajo ahora mismo: o ha empezado a apagarse, o la cola de ese
        // repositorio está llena, o su trabajador no está. El motivo —que incluye los
        // identificadores y, en el último caso, la causa de base— se queda en el registro.
        ErrorDelMotor::MotorParado
        | ErrorDelMotor::ColaDelRepositorioLlena { .. }
        | ErrorDelMotor::SinQuienAtiendaLaCola { .. } => AppError::ServiceUnavailable(motivo),

        // 500 — un fallo del servidor: la base no atendió el encolado, el hilo de trabajo se cayó, o
        // el motor se montó fuera de un runtime (que no es un desenlace de una petición, pero el
        // `match` es exhaustivo a propósito).
        ErrorDelMotor::Base { .. }
        | ErrorDelMotor::TrabajoInterrumpido { .. }
        | ErrorDelMotor::SinRuntime => AppError::Other(motivo),

        // 502 — un servicio de fuera no contesta: el motor de contenedores —desenlace de
        // `Motor::comprobar`, que el arranque usa para avisar—, o el repositorio. Ver arriba por qué
        // el del repositorio lleva aquí el desenlace por defecto.
        ErrorDelMotor::MotorDeContenedores { .. } | ErrorDelMotor::Repositorio { .. } => {
            AppError::BadGateway(motivo)
        }
    }
}

/// El `400` de una passphrase que no abre el repositorio que ya está en el destino.
///
/// Es texto **propio** y no el `motivo` que trae el error del motor, y no por escrúpulo de estilo:
/// el motivo de la biblioteca es el suyo, está escrito para el registro —nombra la comprobación que
/// falló, «no suitable key found»— y copiarlo al cuerpo ataría el contrato del cable a un texto de
/// una dependencia que puede cambiar en cualquier versión menor. Lo que el `400` tiene que decir es
/// lo que el operador necesita leer para arreglarlo, y eso cabe en una frase: el campo, y que no
/// abre. El destino sí se nombra —lo mandó la petición—, pero el mensaje no lo lleva: el panel tiene
/// el formulario delante, y el registro es quien guarda el motivo entero.
const PASSPHRASE_QUE_NO_ABRE: &str = "el campo `passphrase` no abre el repositorio que ya está en el destino: \
     revisa la passphrase, y que el repositorio del destino siga entero";

/// Traduce el desenlace del motor al error de la API **en el alta de un repositorio**.
///
/// Es el único sitio que elige el código de [`ErrorDelMotor::Repositorio`], y existe porque
/// [`del_motor`] no puede elegirlo: su documentación lo dice —el `502` que da por defecto vale para
/// los caminos en los que el motor no sabe cuál de los dos desenlaces fue—, y aquí sí se sabe,
/// porque la [`causa`](ErrorDelMotor::Repositorio::causa) viaja entera y con su tipo. Vive junto a
/// la tabla que NO elige, y no en el manejador, por el mismo motivo por el que vive la tabla: una
/// segunda correspondencia de código escrita en un manejador es una correspondencia que se
/// desincroniza del contrato, y el día que haya otro sitio que abra un repositorio con las
/// credenciales de un formulario tendrá que elegir igual que éste.
///
/// La frontera es **de quién es la culpa**, que es lo que decide si el operador puede arreglarlo
/// mandando otra petición:
///
/// * **La credencial no abre un repositorio que ya existe** ⇒ `400`. El destino contestó —se acaba
///   de listar—, así que lo que falla es un dato del formulario, y quien lo mandó lo tiene delante y
///   puede corregirlo. Un `502` aquí mandaría a mirar el servicio de fuera cuando lo que hay que
///   mirar es lo que se escribió.
/// * **Un campo del cuerpo que no puede funcionar** —vacío— ⇒ `400` **nombrando el campo**, que es
///   la forma que el rechazo de un formulario tiene en toda la API. Dos de los tres casos son un
///   rechazo que ya existía antes de este change: un `secret_access_key` o una `passphrase` vacíos
///   los rechazaba la capa de acceso con [`crate::db::DbError::SecretoVacio`], y ese camino deja de
///   recorrerse porque ahora el alta habla con el destino **antes** de escribir nada; el mensaje se
///   mantiene palabra por palabra para que el cliente vea el mismo. El tercero es nuevo y es una
///   mejora medida: un `endpoint`, un `bucket` o una credencial vacíos se escribían hasta hoy en la
///   fila y el alta contestaba `201`, y ahora esa fila no se puede ni abrir, así que lo que
///   corresponde es decir qué campo está mal y no culpar al destino con un `502`.
/// * **Todo lo demás** ⇒ el desenlace por defecto de [`del_motor`], que es el `502`: el destino no
///   se alcanza, o se alcanza y no se deja inicializar. Eso no se arregla cambiando la petición.
///
/// # Por qué la causa se mira por variante y no por su texto
///
/// Porque el texto no es contrato de nadie: `Error::PassphraseNoAbre` y `Error::DestinoNoAlcanzable`
/// son las dos variantes de la biblioteca, están construidas por el módulo que las midió —que
/// distingue los desenlaces **preguntando al destino** y por el código que publica `rustic_core`,
/// nunca por el mensaje— y una variante nueva rompe este `match` en la compilación si algún día
/// aparece. Y ninguno de los dos mensajes que salen de aquí publica un secreto: el de la passphrase
/// es una constante, y el `502` no publica motivo.
///
/// # Cuándo se llama, y cuándo **no**
///
/// Solo el alta de un repositorio. El `PUT` de un repositorio **no** conecta ni inicializa —editar
/// una fila no puede depender de que el destino esté en pie, y el porqué está escrito en su
/// manejador—, así que no debe pasar por aquí.
pub(crate) fn del_motor_en_el_alta(error: ErrorDelMotor) -> AppError {
    if let ErrorDelMotor::Repositorio { causa, .. } = &error {
        match causa {
            crate::rustic::Error::PassphraseNoAbre { .. } => {
                return AppError::InvalidInput(PASSPHRASE_QUE_NO_ABRE.to_owned());
            }
            crate::rustic::Error::AccesoInvalido { campo } => {
                return AppError::InvalidInput(format!("el campo `{campo}` no puede ir vacío"));
            }
            _ => {}
        }
    }

    del_motor(error)
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
