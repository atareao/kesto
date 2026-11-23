//! El *gate* de la superficie protegida: sin credencial válida no se pasa.
//!
//! Admite **dos** credenciales, y no una en lugar de la otra: la cookie de sesión que emitió el
//! flujo OIDC —pensada para un navegador— y un token de API en `Authorization: Bearer`, que es
//! lo que puede llevar una máquina. Si llegan las dos, manda el token: quien lo presenta está
//! diciendo con qué identidad quiere que se le atienda, y el token es la credencial más
//! específica de las dos.
//!
//! La sesión no lleva scopes —es el operador entero— y el token sí: por eso el alcance se aplica
//! **sólo** al camino del token, y por eso la decisión vive aquí y no en cada manejador. La
//! regla se expresa como una **lista blanca por ruta y método** (ver [`acceso_con_token`]): lo
//! que no está en ella queda fuera del alcance de cualquier token, así que una ruta nueva no
//! queda abierta por olvido.

use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::auth::session::{self, SESSION_COOKIE_NAME};
use crate::db::entities::{Scope, hash_de_token};
use crate::error::AppError;
use crate::state::AppState;

/// Identidad de la sesión ya verificada, tal y como la ve un manejador.
///
/// Viaja en las *extensions* de la petición —el camino natural de axum para
/// pasar datos de una capa al manejador— y no vuelve a leerse de la cookie: el
/// manejador no tiene por qué saber que la identidad viaja dentro de un JWT, ni
/// tiene por qué verificarlo otra vez.
///
/// Lleva **sólo** `sub`, `email` y el `jti` de la sesión, no los `iat`/`exp` de
/// [`crate::auth::session::Claims`]: la vigencia ya la comprobó el *gate*, y
/// exponerla invitaría a reimplementar esa comprobación en cada manejador. El
/// `jti` sí viaja porque es lo que el `logout` necesita para revocar **esta**
/// sesión concreta, y no se expone en ninguna respuesta.
#[derive(Debug, Clone)]
pub struct SessionIdentity {
    /// Identificador del usuario en el proveedor de identidad.
    pub sub: String,
    /// Correo del usuario.
    pub email: String,
    /// Identificador único de la sesión: la clave con la que se revoca.
    pub jti: String,
}

/// Identidad de un token de API ya verificado, tal y como la ve un manejador.
///
/// Es la hermana de [`SessionIdentity`] en el camino del token, y va por las mismas
/// *extensions*: el manejador no vuelve a tocar la cabecera ni a hashear nada. Lleva los scopes
/// porque son lo que el token **puede** hacer —a diferencia de la sesión, que puede todo—, y un
/// manejador que quiera distinguir «quién llama» de «qué puede hacer» los tiene ahí sin volver a
/// consultar la base.
#[derive(Debug, Clone)]
pub struct IdentidadDeToken {
    /// Identificador de la fila del token.
    pub id: Uuid,
    /// Nombre con el que se emitió.
    pub name: String,
    /// Permisos concedidos.
    pub scopes: Vec<Scope>,
}

/// Motivo con el que se rechaza una petición que no trae cookie de sesión.
const SIN_SESION: &str = "hace falta una sesión válida para esta ruta";

/// Motivo con el que se rechaza un token que no vale.
///
/// Es **el mismo** para un token que no existe, uno revocado y uno caducado: decir cuál de los
/// tres es contaría a quien no ha entrado si su token sigue vivo o si alguien lo revocó.
const TOKEN_QUE_NO_VALE: &str = "el token de API no vale para esta petición";

/// Motivo del `403` de un token cuyo scope no llega.
const SCOPE_INSUFICIENTE: &str = "el token no tiene el permiso necesario para esta ruta";

/// Motivo del `403` de una acción que ningún token puede hacer.
const FUERA_DEL_ALCANCE: &str =
    "esta acción queda fuera del alcance de cualquier token: hace falta una sesión";

/// El prefijo de la cabecera `Authorization` que porta un token.
const PREFIJO_BEARER: &str = "Bearer ";

/// *Gate* de la superficie protegida.
///
/// Con un `Authorization: Bearer <token>` atiende por el camino del token; sin él, por el de la
/// cookie. Sin ninguna de las dos contesta `401` con la envoltura `{status, message}` —nunca con
/// el cuerpo vacío, nunca con HTML y nunca con una redirección: la API contesta JSON y es el
/// panel quien decide qué hacer—.
///
/// Con una credencial válida deja la identidad en las *extensions* y pasa al manejador.
pub async fn gate(
    State(estado): State<AppState>,
    jar: CookieJar,
    mut request: Request,
    next: Next,
) -> Response {
    // El token manda si está: quien lo presenta está eligiendo con qué identidad se le atienda.
    // Se copia el valor antes de mover la petición: el préstamo de la cabecera no puede
    // sobrevivir al `move` de `request`.
    let en_claro = token_del_caller(&request).map(str::to_owned);

    if let Some(en_claro) = en_claro {
        return con_token(&estado, &en_claro, request, next).await;
    }

    let identidad = match con_cookie(&estado, &jar).await {
        Ok(identidad) => identidad,
        Err(rechazo) => return rechazo.into_response(),
    };

    // La identidad viaja en las *extensions*: el manejador la lee de ahí y no
    // vuelve a tocar la cookie.
    request.extensions_mut().insert(identidad);

    next.run(request).await
}

/// Atiende la petición por el camino del token: lo verifica, aplica su alcance y pasa.
async fn con_token(
    estado: &AppState,
    en_claro: &str,
    mut request: Request,
    next: Next,
) -> Response {
    // Se busca por el hash: la base no guarda el valor en claro, así que lo único que se puede
    // preguntar es por el hash del token entrante. Hashear es un HMAC que no puede fallar con
    // nuestra clave de 32 bytes, pero el fallo se propaga —y no se degrada a un hash vacío—.
    let hash = match hash_de_token(en_claro, estado.db.clave.api_token_key()) {
        Ok(hash) => hash,
        Err(error) => return AppError::Database(error).into_response(),
    };
    let token = match estado.db.tokens.buscar_por_hash(&hash).await {
        Ok(Some(token)) => token,
        Ok(None) => return AppError::Unauthorized(TOKEN_QUE_NO_VALE.to_owned()).into_response(),
        Err(error) => return AppError::Database(error).into_response(),
    };

    // La revocación y la caducidad se comprueban **en cada petición**, sin caché ni ventana de
    // gracia: el contrato pide que la revocación surta efecto en la petición siguiente.
    if !token.en_vigor_en(OffsetDateTime::now_utc()) {
        return AppError::Unauthorized(TOKEN_QUE_NO_VALE.to_owned()).into_response();
    }

    match acceso_con_token(request.method(), request.uri().path()) {
        Acceso::Lectura if token.tiene(Scope::Read) => {}
        Acceso::Accion if token.tiene(Scope::Run) => {}
        // El token vale pero no llega: es un `403` y no un `401`.
        Acceso::Lectura | Acceso::Accion => {
            return AppError::Forbidden(SCOPE_INSUFICIENTE.to_owned()).into_response();
        }
        // Y ninguna credencial de este tipo alcanza estas rutas.
        Acceso::Vetado => {
            return AppError::Forbidden(FUERA_DEL_ALCANCE.to_owned()).into_response();
        }
    }

    // El último uso se anota **después** de decidir y **fuera del camino crítico**: es
    // informativo —cuándo se usó por última vez—, así que ni bloquea la respuesta ni puede
    // tumbar una petición ya autorizada. Va desprendido en su propia tarea para no pagar una
    // escritura en la base en **cada** petición con token: aquí solo se deja la orden.
    let tokens = Arc::clone(&estado.db.tokens);
    let id_de_token = token.id;
    tokio::spawn(async move {
        if let Err(error) = tokens.anotar_uso(&id_de_token).await {
            tracing::warn!(error = %error, "no se pudo anotar el último uso del token");
        }
    });

    request.extensions_mut().insert(IdentidadDeToken {
        id: token.id,
        name: token.name,
        scopes: token.scopes,
    });

    next.run(request).await
}

/// Resuelve la identidad por el camino de la cookie de sesión.
///
/// Verifica la firma y la vigencia, y **además** consulta si la sesión está revocada
/// —el cierre de sesión deja su `jti` en `sesiones_revocadas`—, porque la firma sola no
/// permite cerrar un JWT antes de su `exp`. Es una lectura más de la base, de la misma
/// clase que ya hace el camino del token, y va en **cada** petición: sin caché ni ventana
/// de gracia, para que la revocación surta efecto en la petición siguiente.
async fn con_cookie(estado: &AppState, jar: &CookieJar) -> Result<SessionIdentity, AppError> {
    // Por construcción el *gate* sólo se monta donde hay proveedor, así que un `None` aquí es un
    // fallo de invariante: se rechaza la petición en vez de abrirla.
    let auth = estado
        .auth
        .as_ref()
        .ok_or_else(|| AppError::Unauthorized(SIN_SESION.to_owned()))?;

    let cookie = jar
        .get(SESSION_COOKIE_NAME)
        .ok_or_else(|| AppError::Unauthorized(SIN_SESION.to_owned()))?;

    let claims = match session::verify(&auth.secret, cookie.value()) {
        // El rechazo no distingue «no hay cookie» de «la cookie no vale»: decir cuál de las dos
        // es contaría a quien no ha entrado si su token caducó o si la firma no cuadra. El motivo
        // concreto se queda en el registro, dentro de `session::verify`.
        Ok(claims) => claims,
        Err(error) => return Err(error),
    };

    // La revocación y el rechazo comparten el mismo `401` y el mismo motivo que la cookie
    // ausente o inválida: quien no ha entrado no aprende si su sesión se cerró.
    if estado
        .db
        .sesiones
        .esta_revocada(&claims.jti)
        .await
        .map_err(AppError::Database)?
    {
        return Err(AppError::Unauthorized(SIN_SESION.to_owned()));
    }

    Ok(SessionIdentity {
        sub: claims.sub,
        email: claims.email,
        jti: claims.jti,
    })
}

/// El token en claro que trae la petición, si trae uno.
///
/// Sólo cuenta una cabecera `Authorization: Bearer <valor>` con valor no vacío: una cabecera
/// ausente, con otro esquema o con el valor en blanco **no** es una credencial de token, así que
/// la petición sigue por el camino de la cookie en vez de rechazarse por una cabecera mal puesta.
fn token_del_caller(request: &Request) -> Option<&str> {
    let valor = request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = valor.strip_prefix(PREFIJO_BEARER)?.trim();

    if token.is_empty() { None } else { Some(token) }
}

/// Qué puede hacer un token de API en una ruta y con un método.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Acceso {
    /// Consultar sin modificar: basta `read`.
    Lectura,
    /// Accionar: dispara trabajo y exige `run`.
    Accion,
    /// Fuera del alcance de cualquier token.
    Vetado,
}

/// La lista blanca de lo que un token puede hacer, por ruta y método.
///
/// Es una **lista blanca** y no una negra, y ahí está la garantía: lo que no está aquí queda
/// vetado para cualquier token, así que una ruta nueva —o una acción nueva sobre un recurso que
/// ya existe— nace fuera del alcance de los tokens y hay que venir a esta función a decir qué
/// permiso pide. Una lista negra habría dejado abierta cualquier ruta que nadie recordara
/// apuntar.
///
/// Y lo que un token alcanza es **solo lo que dispara trabajo**: lanzar una copia y comprobar la
/// salud de un repositorio —de uno o de todos—. El alta y la edición de recursos quedan fuera
/// aunque `run` suene a «puede hacer cosas»: un token `run` filtrado que pudiera dar de alta un
/// repositorio le daría al atacante un sitio donde apuntar sus propias credenciales, así que
/// esas rutas piden sesión. Ningún token restaura ni borra —`DELETE` ni siquiera figura— y la
/// gestión de credenciales —`/api/v1/tokens`— tampoco: un token que pudiera emitir otro sería
/// una vía de escalada.
fn acceso_con_token(metodo: &axum::http::Method, ruta: &str) -> Acceso {
    let Some(resto) = ruta.strip_prefix("/api/v1/") else {
        return Acceso::Vetado;
    };

    let mut segmentos = resto.split('/');
    let recurso = segmentos.next().unwrap_or_default();
    let cola: Vec<&str> = segmentos.collect();
    let cola = cola.as_slice();

    match metodo.as_str() {
        // Las lecturas: la salud, el stream, los cuatro recursos con su identificador y los dos
        // listados derivados.
        "GET" | "HEAD" => match (recurso, cola) {
            ("health" | "events" | "executions" | "snapshots" | "restorations", []) => {
                Acceso::Lectura
            }
            ("repositories" | "targets" | "jobs", [] | [_]) => Acceso::Lectura,
            _ => Acceso::Vetado,
        },
        // Las acciones que disparan trabajo, y **solo** ésas: el disparo de una tarea y la
        // comprobación de salud de un repositorio —de uno o de todos, que las dos lanzan el
        // trabajo de comprobación—. El alta de un recurso (`POST` sin cola) no entra: nace vetada.
        "POST" => match (recurso, cola) {
            ("repositories", ["health"]) | ("repositories", [_, "health"]) => Acceso::Accion,
            ("jobs", [_, "run"]) => Acceso::Accion,
            _ => Acceso::Vetado,
        },
        // Todo lo demás —`PUT`, `DELETE`, la restauración, el alta de recursos, la gestión de
        // tokens— queda fuera: no hay ningún scope que lo habilite.
        _ => Acceso::Vetado,
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::extract::Extension;
    use axum::http::StatusCode;
    use time::OffsetDateTime;
    use time::macros::datetime;
    use tower::ServiceExt;
    use uuid::Uuid;

    use super::{Acceso, IdentidadDeToken, SessionIdentity, acceso_con_token};
    use crate::api::protected_router;
    use crate::api::response::{ApiResponse, Data};
    use crate::auth::session;
    use crate::db::entities::{AltaDeToken, Scope, hash_de_token};
    use crate::secret::SecretString;
    use crate::state::AppState;
    use crate::test_support::{
        EMAIL, OTRA_SESSION_SECRET, SUB, body_json, body_text, cookie_de_sesion,
        delete_with_bearer, estado_oidc, get, get_with_bearer, get_with_cookie, post_with_bearer,
        post_with_cookie, session_cookie_header,
    };

    /// Ruta ficticia de negocio, colgada de la mitad protegida como lo estará
    /// cualquier ruta real el día que su change la declare.
    const RUTA: &str = "/negocio";

    /// Un identificador bien formado cualquiera para las rutas con identificador.
    const UN_UUID: &str = "00000000-0000-4000-8000-000000000000";

    /// Manejador que devuelve la identidad de la **sesión** que el *gate* dejó en las
    /// *extensions*: es lo que prueba que el manejador la ve sin leer la cookie.
    async fn quien_soy(Extension(identidad): Extension<SessionIdentity>) -> ApiResponse {
        ApiResponse::new(
            StatusCode::OK,
            "sesión válida",
            Data::Some(serde_json::json!({
                "sub": identidad.sub,
                "email": identidad.email,
            })),
        )
    }

    /// Manejador que devuelve la identidad del **token** que el *gate* dejó en las *extensions*.
    async fn quien_es_el_token(Extension(identidad): Extension<IdentidadDeToken>) -> ApiResponse {
        ApiResponse::new(
            StatusCode::OK,
            "token válido",
            Data::Some(serde_json::json!({
                "id": identidad.id.hyphenated().to_string(),
                "name": identidad.name,
                "scopes": identidad.scopes.iter().map(|scope| scope.texto()).collect::<Vec<_>>(),
            })),
        )
    }

    /// Manejador que responde OK sin exigir identidad: sirve para las rutas donde lo que se
    /// mide es el *gate* —el `403` de un token vetado o el paso de la sesión— y no el manejador.
    async fn paso_autorizado() -> ApiResponse {
        ApiResponse::new(StatusCode::OK, "petición autorizada", Data::None)
    }

    /// La mitad protegida de producción con las rutas ficticias de cada caso.
    ///
    /// Las rutas **imitan el camino de producción** —`/api/v1/<recurso>…`— porque lo que se mide
    /// aquí es la lista blanca del alcance, que se resuelve por ruta y método: con rutas
    /// inventadas el *gate* no tendría nada que clasificar.
    async fn router(estado: &AppState) -> Router {
        let rutas = Router::new()
            .route(RUTA, axum::routing::get(quien_soy))
            .route(
                "/api/v1/repositories",
                axum::routing::get(quien_es_el_token).post(quien_es_el_token),
            )
            // El alta de un target y de una tarea: el *gate* las veta antes de llegar aquí.
            .route("/api/v1/targets", axum::routing::post(quien_es_el_token))
            .route("/api/v1/jobs", axum::routing::post(quien_es_el_token))
            .route(
                &format!("/api/v1/repositories/{UN_UUID}"),
                axum::routing::delete(quien_es_el_token),
            )
            .route(
                &format!("/api/v1/jobs/{UN_UUID}/run"),
                axum::routing::post(quien_es_el_token),
            )
            .route(
                &format!("/api/v1/snapshots/{UN_UUID}/restore"),
                axum::routing::post(quien_es_el_token),
            )
            // La gestión de tokens no lee identidad de token: con sesión el *gate* la deja pasar
            // y con un token la veta antes de llegar aquí.
            .route("/api/v1/tokens", axum::routing::post(paso_autorizado));

        protected_router(rutas, estado)
    }

    /// El estado con autenticación encendida, para colgar el *gate*.
    async fn estado() -> AppState {
        estado_oidc().await
    }

    /// Emite una sesión de prueba con la clave del estado y devuelve el token en claro.
    ///
    /// Se emite por la **capa de sesión** con el secreto del propio estado: así el token
    /// verifica contra el mismo `KESTO_SECRET` con el que el *gate* comprueba.
    fn token_de_sesion(estado: &AppState) -> String {
        let auth = estado
            .auth
            .as_ref()
            .expect("en modo Oidc hay estado de autenticación");

        session::issue(&auth.secret, SUB.to_owned(), EMAIL.to_owned())
            .expect("la emisión de la sesión de prueba no puede fallar")
    }

    /// Emite un token de prueba con los scopes dados y devuelve su valor en claro.
    ///
    /// Se emite por la **capa de acceso** y no por la API: lo que estas pruebas miden es el
    /// *gate*, no el alta del token, que tiene las suyas.
    async fn token_con(estado: &AppState, scopes: &[Scope]) -> String {
        let en_claro = format!("kesto_token-de-prueba-{}", Uuid::new_v4());

        estado
            .db
            .tokens
            .alta(AltaDeToken {
                name: "token-de-prueba".to_owned(),
                en_claro: en_claro.clone(),
                scopes: scopes.to_vec(),
                expires_at: None,
            })
            .await
            .expect("el alta del token de prueba no puede fallar");

        en_claro
    }

    #[tokio::test]
    async fn una_peticion_protegida_sin_cookie_contesta_401_con_la_envoltura() {
        // Escenario 1.27.
        let respuesta = get(router(&estado().await).await, RUTA).await;

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
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {texto}"
        );
    }

    #[tokio::test]
    async fn una_cookie_valida_deja_pasar_y_el_manejador_ve_la_identidad() {
        // Escenario 1.31.
        let respuesta =
            get_with_cookie(router(&estado().await).await, RUTA, &cookie_de_sesion()).await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["sub"], SUB,
            "el manejador no vio el usuario de la sesión: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["email"], EMAIL,
            "el manejador no vio el correo de la sesión: {cuerpo}"
        );
        // Y sin volver a leer la cookie: la única fuente del manejador son las
        // *extensions* que dejó el *gate*.
        assert!(
            cuerpo.to_string().contains(SUB),
            "la respuesta no trae la identidad: {cuerpo}"
        );
    }

    /// 3.1 — Una cookie bien firmada y en vigor cuyo `jti` está revocado contesta `401`.
    ///
    /// La firma sola no basta: el cierre de sesión revoca el `jti` en la base, y el *gate*
    /// tiene que consultarlo para que esa cookie deje de servir hasta su caducidad.
    #[tokio::test]
    async fn una_cookie_con_el_jti_revocado_no_se_acepta() {
        let estado = estado().await;
        let token = token_de_sesion(&estado);
        let claims = {
            let auth = estado
                .auth
                .as_ref()
                .expect("en modo Oidc hay estado de autenticación");
            session::verify(&auth.secret, &token).expect("el token emitido debe verificar")
        };

        // Se revoca por la **misma** capa que usará el logout: así lo que se mide es el
        // *gate*, no el camino de la revocación.
        estado
            .db
            .sesiones
            .revocar(&claims.jti, claims.exp)
            .await
            .expect("la revocación de prueba no puede fallar");

        let respuesta =
            get_with_cookie(router(&estado).await, RUTA, &session_cookie_header(&token)).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::UNAUTHORIZED,
            "una cookie con el `jti` revocado no puede pasar el *gate*"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 401);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 401 debe traer un mensaje: {cuerpo}"
        );
    }

    /// 3.2 — La misma cookie **sin** revocar deja pasar: el contrapeso del escenario anterior,
    /// para que el rechazo sea por la revocación y no por cualquier otra cosa.
    #[tokio::test]
    async fn la_misma_cookie_sin_revocar_deja_pasar() {
        let estado = estado().await;
        let token = token_de_sesion(&estado);

        let respuesta =
            get_with_cookie(router(&estado).await, RUTA, &session_cookie_header(&token)).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "una cookie en vigor y no revocada tiene que pasar"
        );
    }

    #[tokio::test]
    async fn una_cookie_firmada_con_otra_clave_no_se_acepta() {
        // Escenario «una cookie firmada con otra clave no se acepta» de
        // `specs/auth/spec.md`, esta vez atravesando el *gate*: el rechazo no
        // sale del verificador sino de quien lo llama, y hay que saber que ese
        // camino devuelve el 401 de la envoltura y no pasa al manejador.
        let token = session::issue(
            &SecretString::new(OTRA_SESSION_SECRET.to_owned()),
            SUB.to_owned(),
            EMAIL.to_owned(),
        )
        .expect("la emisión de la sesión de prueba no puede fallar");

        let respuesta = get_with_cookie(
            router(&estado().await).await,
            RUTA,
            &session_cookie_header(&token),
        )
        .await;

        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 401);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 401 debe traer un mensaje: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn un_bearer_valido_sin_cookie_se_acepta_y_el_manejador_ve_sus_scopes() {
        // api-tokens, escenario «Una petición con Bearer y sin cookie se acepta».
        let estado = estado().await;
        let token = token_con(&estado, &[Scope::Read]).await;

        let respuesta =
            get_with_bearer(router(&estado).await, "/api/v1/repositories", &token).await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["name"], "token-de-prueba",
            "el manejador no vio la identidad del token: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["scopes"],
            serde_json::json!(["read"]),
            "los scopes del token no llegaron al manejador: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn un_bearer_desconocido_revocado_o_caducado_contesta_401_igual() {
        // api-tokens: «sin distinguir en el mensaje si el token no existe, está revocado o ha
        // caducado».
        let estado = estado().await;
        let router = router(&estado).await;

        let revocado = token_con(&estado, &[Scope::Read]).await;
        let fila = estado
            .db
            .tokens
            .buscar_por_hash(
                &hash_de_token(&revocado, estado.db.clave.api_token_key())
                    .expect("el hash de prueba no puede fallar"),
            )
            .await
            .expect("la búsqueda no puede fallar")
            .expect("el token debe estar");
        estado
            .db
            .tokens
            .revocar(&fila.id)
            .await
            .expect("la revocación no puede fallar");

        let caducado = format!("kesto_caducado-{}", Uuid::new_v4());
        estado
            .db
            .tokens
            .alta(AltaDeToken {
                name: "caducado".to_owned(),
                en_claro: caducado.clone(),
                scopes: vec![Scope::Read],
                expires_at: Some(datetime!(2020-01-01 00:00:00 UTC)),
            })
            .await
            .expect("el alta del token caducado no puede fallar");

        let mut mensajes = Vec::new();

        for token in ["kesto_no-existe-000000000000000000", &revocado, &caducado] {
            let respuesta = get_with_bearer(router.clone(), "/api/v1/repositories", token).await;

            assert_eq!(
                respuesta.status(),
                StatusCode::UNAUTHORIZED,
                "un token que no vale tiene que contestar 401"
            );

            let cuerpo = body_json(respuesta).await;

            mensajes.push(
                cuerpo["message"]
                    .as_str()
                    .expect("el 401 trae mensaje")
                    .to_owned(),
            );
        }

        assert_eq!(
            mensajes[0], mensajes[1],
            "el 401 no puede distinguir por qué no vale el token"
        );
        assert_eq!(
            mensajes[1], mensajes[2],
            "el 401 no puede distinguir por qué no vale el token"
        );
        assert!(
            !mensajes[0].contains("revoc"),
            "el mensaje no puede decir que el token está revocado: {}",
            mensajes[0]
        );
    }

    #[tokio::test]
    async fn con_bearer_y_cookie_a_la_vez_manda_el_token() {
        // api-tokens, escenario «Bearer y cookie a la vez no se estorban»: la identidad resuelta
        // es la del token. Se prueba por donde se nota —un token `read` no puede disparar una
        // copia—: si mandara la cookie, la petición pasaría el *gate* y llegaría al manejador.
        let estado = estado().await;
        let token = token_con(&estado, &[Scope::Read]).await;

        let peticion = axum::http::Request::builder()
            .method(axum::http::Method::POST)
            .uri(format!("/api/v1/jobs/{UN_UUID}/run"))
            .header(axum::http::header::COOKIE, cookie_de_sesion())
            .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .expect("la petición de prueba se construye");

        let respuesta = router(&estado)
            .await
            .oneshot(peticion)
            .await
            .expect("el router no puede fallar");

        assert_eq!(
            respuesta.status(),
            StatusCode::FORBIDDEN,
            "con las dos credenciales tiene que mandar el token, y el token es de solo lectura"
        );
    }

    #[tokio::test]
    async fn un_token_read_lee_pero_no_acciona() {
        // api-tokens: «Un token read consulta todo» y «Un token read no puede lanzar una copia».
        let estado = estado().await;
        let router = router(&estado).await;
        let token = token_con(&estado, &[Scope::Read]).await;

        let lectura = get_with_bearer(router.clone(), "/api/v1/repositories", &token).await;

        assert_eq!(
            lectura.status(),
            StatusCode::OK,
            "un token read tiene que poder consultar"
        );

        for (donde, respuesta) in [
            (
                "el alta de un repositorio",
                post_with_bearer(router.clone(), "/api/v1/repositories", &token).await,
            ),
            (
                "el disparo de una copia",
                post_with_bearer(
                    router.clone(),
                    &format!("/api/v1/jobs/{UN_UUID}/run"),
                    &token,
                )
                .await,
            ),
        ] {
            assert_eq!(
                respuesta.status(),
                StatusCode::FORBIDDEN,
                "{donde} no puede quedar al alcance de un token read"
            );
        }
    }

    #[tokio::test]
    async fn un_token_run_lee_y_acciona() {
        // api-tokens: «Un token run sí puede lanzar una copia» y, además, cubre las lecturas.
        let estado = estado().await;
        let router = router(&estado).await;
        let token = token_con(&estado, &[Scope::Run]).await;

        let lectura = get_with_bearer(router.clone(), "/api/v1/repositories", &token).await;

        assert_eq!(
            lectura.status(),
            StatusCode::OK,
            "un token run cubre las lecturas: tiene que poder consultar"
        );

        let disparo =
            post_with_bearer(router, &format!("/api/v1/jobs/{UN_UUID}/run"), &token).await;

        assert_eq!(
            disparo.status(),
            StatusCode::OK,
            "un token run tiene que poder disparar una copia"
        );
    }

    #[tokio::test]
    async fn un_token_run_no_da_de_alta_ni_edita_recursos() {
        // api-tokens: `run` cubre «lanzar una copia y comprobar la salud de un repositorio», no
        // el alta ni la edición. Un token `run` filtrado que pudiera dar de alta un repositorio le
        // daría al atacante un sitio donde apuntar sus credenciales.
        let estado = estado().await;
        let router = router(&estado).await;
        let token = token_con(&estado, &[Scope::Run]).await;

        for (donde, respuesta) in [
            (
                "el alta de un repositorio",
                post_with_bearer(router.clone(), "/api/v1/repositories", &token).await,
            ),
            (
                "el alta de un target",
                post_with_bearer(router.clone(), "/api/v1/targets", &token).await,
            ),
            (
                "el alta de una tarea",
                post_with_bearer(router.clone(), "/api/v1/jobs", &token).await,
            ),
        ] {
            assert_eq!(
                respuesta.status(),
                StatusCode::FORBIDDEN,
                "{donde} no puede quedar al alcance de un token, ni siquiera con `run`"
            );
        }
    }

    #[tokio::test]
    async fn ningun_token_restaura_ni_borra_ni_gestiona_credenciales() {
        // api-tokens: «Ningún token restaura ni borra» y «Un token no puede crear otro token».
        let estado = estado().await;
        let router = router(&estado).await;
        let token = token_con(&estado, &[Scope::Run]).await;

        for (donde, respuesta) in [
            (
                "la restauración de una instantánea",
                post_with_bearer(
                    router.clone(),
                    &format!("/api/v1/snapshots/{UN_UUID}/restore"),
                    &token,
                )
                .await,
            ),
            (
                "el borrado de un repositorio",
                delete_with_bearer(
                    router.clone(),
                    &format!("/api/v1/repositories/{UN_UUID}"),
                    &token,
                )
                .await,
            ),
            (
                "la creación de otro token",
                post_with_bearer(router.clone(), "/api/v1/tokens", &token).await,
            ),
        ] {
            assert_eq!(
                respuesta.status(),
                StatusCode::FORBIDDEN,
                "{donde} queda fuera del alcance de cualquier token"
            );
        }
    }

    #[tokio::test]
    async fn la_gestion_de_tokens_sigue_siendo_de_la_sesion() {
        // El otro lado de la moneda: la cookie sí puede administrar credenciales.
        let estado = estado().await;
        let router = router(&estado).await;

        let respuesta = post_with_cookie(router, "/api/v1/tokens", &cookie_de_sesion()).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "con sesión, la gestión de tokens tiene que pasar el *gate*"
        );
    }

    #[tokio::test]
    async fn un_bearer_en_blanco_no_es_una_credencial() {
        // Una cabecera `Bearer` sin valor no es un token: la petición sigue por el camino de la
        // cookie, que tampoco trae, así que contesta 401 y no 403.
        let estado = estado().await;

        let peticion = axum::http::Request::builder()
            .method(axum::http::Method::GET)
            .uri("/api/v1/repositories")
            .header(axum::http::header::AUTHORIZATION, "Bearer ")
            .body(axum::body::Body::empty())
            .expect("la petición de prueba se construye");

        let respuesta = router(&estado)
            .await
            .oneshot(peticion)
            .await
            .expect("el router no puede fallar");

        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);
    }

    /// El token caduca por su `expires_at`, con el instante como parámetro.
    ///
    /// Se mide en la entidad y no a través del *gate*: aquí se fija la comparación —estrictamente
    /// mayor— para que un `expires_at` igual al instante **no** valga.
    #[test]
    fn un_token_caduca_en_su_fecha() {
        let mut token = crate::db::entities::TokenDeApi {
            id: Uuid::nil(),
            name: "prueba".to_owned(),
            token_hash: "hash".to_owned(),
            scopes: vec![Scope::Read],
            created_at: OffsetDateTime::UNIX_EPOCH,
            expires_at: Some(datetime!(2026-01-01 00:00:00 UTC)),
            revoked_at: None,
            last_used_at: None,
        };

        assert!(token.en_vigor_en(datetime!(2025-12-31 23:59:59 UTC)));
        assert!(
            !token.en_vigor_en(datetime!(2026-01-01 00:00:00 UTC)),
            "un token caduca en su fecha: el instante igual ya no vale"
        );

        token.revoked_at = Some(datetime!(2025-01-01 00:00:00 UTC));

        assert!(
            !token.en_vigor_en(datetime!(2025-06-01 00:00:00 UTC)),
            "un token revocado no vale aunque su fecha no haya llegado"
        );
    }

    /// La lista blanca se decide por método y ruta, y aquí se fija la tabla completa.
    ///
    /// Es una prueba de la función pura: solo entran las acciones que **disparan trabajo** —
    /// lanzar una copia y comprobar la salud de un repositorio, de uno o de todos, que las dos
    /// piden `run`—, las lecturas piden `read` y todo lo demás queda vetado. En particular el
    /// alta y la edición de recursos (`POST` sin cola y `PUT`) quedan **fuera**: un token, por
    /// mucho `run` que tenga, no da de alta un repositorio ni retoca una tarea.
    #[test]
    fn la_lista_blanca_decide_por_metodo_y_ruta() {
        use axum::http::Method;

        for (metodo, ruta, esperado) in [
            (Method::GET, "/api/v1/health", Acceso::Lectura),
            (Method::GET, "/api/v1/events", Acceso::Lectura),
            (Method::GET, "/api/v1/executions", Acceso::Lectura),
            (Method::GET, "/api/v1/snapshots", Acceso::Lectura),
            (Method::GET, "/api/v1/restorations", Acceso::Lectura),
            (Method::GET, "/api/v1/repositories", Acceso::Lectura),
            (Method::GET, "/api/v1/repositories/{id}", Acceso::Lectura),
            (Method::GET, "/api/v1/targets/{id}", Acceso::Lectura),
            (Method::GET, "/api/v1/jobs/{id}", Acceso::Lectura),
            (Method::POST, "/api/v1/repositories/health", Acceso::Accion),
            (
                Method::POST,
                "/api/v1/repositories/{id}/health",
                Acceso::Accion,
            ),
            (Method::POST, "/api/v1/jobs/{id}/run", Acceso::Accion),
            // Lo que crea o modifica estado no está al alcance de ningún token.
            (Method::POST, "/api/v1/repositories", Acceso::Vetado),
            (Method::POST, "/api/v1/targets", Acceso::Vetado),
            (Method::POST, "/api/v1/jobs", Acceso::Vetado),
            (Method::PUT, "/api/v1/repositories/{id}", Acceso::Vetado),
            (Method::PUT, "/api/v1/targets/{id}", Acceso::Vetado),
            (Method::PUT, "/api/v1/jobs/{id}", Acceso::Vetado),
            (Method::DELETE, "/api/v1/repositories/{id}", Acceso::Vetado),
            (Method::DELETE, "/api/v1/targets/{id}", Acceso::Vetado),
            (Method::DELETE, "/api/v1/jobs/{id}", Acceso::Vetado),
            (
                Method::POST,
                "/api/v1/snapshots/{id}/restore",
                Acceso::Vetado,
            ),
            (Method::GET, "/api/v1/tokens", Acceso::Vetado),
            (Method::POST, "/api/v1/tokens", Acceso::Vetado),
            (Method::DELETE, "/api/v1/tokens/{id}", Acceso::Vetado),
            (Method::GET, "/negocio", Acceso::Vetado),
        ] {
            assert_eq!(
                acceso_con_token(&metodo, ruta),
                esperado,
                "{metodo} {ruta} tiene que caer en {esperado:?}"
            );
        }
    }
}
