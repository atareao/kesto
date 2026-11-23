//! Las rutas de `/api/v1/tokens`: la gestión de la credencial de máquina.
//!
//! Son **tres** rutas y no las cinco del §5, porque un token no se edita ni se lee de uno en
//! uno: se emite, se lista —para saber cuáles hay y cuándo se usaron— y se revoca. No hay
//! `PUT` porque cambiar los scopes de un token vivo sería una forma silenciosa de escalada, y
//! no hay `GET /{id}` porque el listado ya trae lo poco que hay que saber.
//!
//! La decisión que condiciona el módulo: **el valor en claro se enseña una sola vez**, en la
//! respuesta del alta. La base guarda solo su hash, así que ni el listado, ni el `Debug`, ni un
//! registro pueden devolverlo: un token perdido se revoca y se emite otro. Por eso el alta
//! construye el token, lo hashea y entrega el valor en claro **sin volver a tocarlo**, y por eso
//! la vista del alta es distinta de la del listado —una lleva el valor y la otra no—: son dos
//! tipos, y el que no lo lleva **no tiene dónde** ponerlo.
//!
//! Estas rutas quedan detrás de la **sesión** y no del alcance de ningún token. Un token que
//! pudiera emitir otro sería una vía de escalada, así que la gestión queda fuera de la lista
//! blanca del *gate*; es la sesión la que administra credenciales.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::contenido;
use crate::api::extract::Json;
use crate::api::response::{ApiResponse, Data};
use crate::db::entities::{AltaDeToken, Scope, TokenDeApi};
use crate::error::AppError;
use crate::state::AppState;

/// El prefijo reconocible de un token emitido.
///
/// Va delante para que un token se distinga a simple vista —en una configuración, en un registro
/// ajeno, en un mensaje— y para poder rechazarlo sin consultar la base si un día el formato
/// cambia. No es un secreto: la entropía la pone el resto.
const PREFIJO_DEL_TOKEN: &str = "kesto_";

/// El número de bytes aleatorios de un token: 256 bits.
///
/// Es lo que hace que el valor no sea adivinable, y por eso el hash puede prescindir de sal: el
/// valor ya es aleatorio y largo.
const BYTES_DEL_TOKEN: usize = 32;

/// Las tres rutas de la credencial de máquina.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/tokens", get(listado).post(alta))
        // El identificador cuelga de `/api/v1/tokens`, así que `POST /api/v1/tokens/{id}` no
        // compite con el alta: el alta no lleva identificador en la ruta.
        .route("/api/v1/tokens/{id}", axum::routing::delete(revocar))
}

/// El cuerpo del alta, tal y como llega por el cable.
///
/// `deny_unknown_fields` por la regla de la casa: un campo de más es una petición mal formada y
/// se contesta `400`, no se ignora. `scopes` viaja como **texto** —`read`, `run`—, que es el
/// vocabulario del cable, y `expires_at` como cadena RFC 3339: la columna guarda `NULL` si no
/// caduca, así que el campo es opcional.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AltaDeTokenDelCable {
    /// Nombre con el que reconocer el token.
    pub name: String,
    /// Los permisos que se conceden.
    pub scopes: Vec<String>,
    /// Cuándo deja de valer, en RFC 3339; ausente si no caduca.
    pub expires_at: Option<String>,
}

/// La vista del listado: **sin** el valor en claro ni el hash.
///
/// No tiene dónde ponerlos —no hay campo—, que es la garantía más fuerte que se puede dar: una
/// estructura que no los lleva no puede publicarlos por descuido.
#[derive(Debug, Serialize)]
pub struct TokenDelCable {
    /// Identificador de la fila.
    pub id: String,
    /// Nombre con el que se emitió.
    pub name: String,
    /// Permisos concedidos.
    pub scopes: Vec<String>,
    /// Fecha de emisión.
    pub created_at: String,
    /// Cuándo caduca; `null` si no caduca.
    pub expires_at: Option<String>,
    /// Cuándo se revocó; `null` mientras sigue en vigor.
    pub revoked_at: Option<String>,
    /// Cuándo se usó por última vez; `null` si no se ha usado.
    pub last_used_at: Option<String>,
}

impl TokenDelCable {
    /// Convierte una fila en su vista del cable.
    fn desde(fila: &TokenDeApi) -> Result<Self, AppError> {
        Ok(Self {
            id: fila.id.hyphenated().to_string(),
            name: fila.name.clone(),
            scopes: fila
                .scopes
                .iter()
                .map(|scope| scope.texto().to_owned())
                .collect(),
            created_at: fecha(fila.created_at)?,
            expires_at: fecha_opcional(fila.expires_at)?,
            revoked_at: fecha_opcional(fila.revoked_at)?,
            last_used_at: fecha_opcional(fila.last_used_at)?,
        })
    }
}

/// La vista del alta: la **única** que lleva el valor en claro.
///
/// Es un tipo aparte del de listado y no el mismo con un campo opcional, porque el valor solo
/// existe en el instante del alta: un tipo que puedan devolver las dos rutas sería un tipo por
/// el que el valor se podría colar en el listado sin que nada lo delatara.
#[derive(Serialize)]
pub struct TokenEmitidoDelCable {
    /// Identificador de la fila.
    pub id: String,
    /// Nombre con el que se emitió.
    pub name: String,
    /// Permisos concedidos.
    pub scopes: Vec<String>,
    /// El valor en claro: se enseña **una sola vez**, aquí.
    pub token: String,
    /// Fecha de emisión.
    pub created_at: String,
    /// Cuándo caduca; `null` si no caduca.
    pub expires_at: Option<String>,
}

impl TokenEmitidoDelCable {
    /// Convierte una fila en su vista, con el valor en claro que se enseña una vez.
    fn desde(fila: &TokenDeApi, en_claro: String) -> Result<Self, AppError> {
        Ok(Self {
            id: fila.id.hyphenated().to_string(),
            name: fila.name.clone(),
            scopes: fila
                .scopes
                .iter()
                .map(|scope| scope.texto().to_owned())
                .collect(),
            token: en_claro,
            created_at: fecha(fila.created_at)?,
            expires_at: fecha_opcional(fila.expires_at)?,
        })
    }
}

/// El `Debug` **redacta** el valor en claro.
///
/// La vista del alta lleva el token por necesidad —es la respuesta que lo enseña una vez—, y un
/// `Debug` derivado lo volcaría en cualquier traza que imprimiera la respuesta. Se redacta y se
/// dice que está redactado, como en la entidad.
impl std::fmt::Debug for TokenEmitidoDelCable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenEmitidoDelCable")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("scopes", &self.scopes)
            .field("token", &"redactado")
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Emite un token: prefijo reconocible y 256 bits aleatorios.
///
/// La aleatoriedad sale del `OsRng` con `try_fill_bytes` y **no** con `fill_bytes`: el segundo
/// entra en pánico si el `getrandom` subyacente falla, y una emisión de credenciales no es sitio
/// para un pánico.
fn token_nuevo() -> Result<String, AppError> {
    let mut bytes = [0u8; BYTES_DEL_TOKEN];

    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|error| AppError::Other(format!("no se pudo generar el token: {error}")))?;

    Ok(format!(
        "{PREFIJO_DEL_TOKEN}{}",
        URL_SAFE_NO_PAD.encode(bytes)
    ))
}

/// Formatea una fecha del contrato, o falla en alto si no se puede.
fn fecha(valor: OffsetDateTime) -> Result<String, AppError> {
    valor.format(&Rfc3339).map_err(|error| {
        AppError::Other(format!("la fecha de la fila no se pudo formatear: {error}"))
    })
}

/// Formatea una fecha opcional, conservando el `None`.
fn fecha_opcional(valor: Option<OffsetDateTime>) -> Result<Option<String>, AppError> {
    valor.map(fecha).transpose()
}

/// Convierte el identificador de la ruta en un `Uuid`, o contesta `400`.
fn identificador(id: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(id).map_err(|_| {
        AppError::InvalidInput("el campo `id` de la ruta no tiene la forma de un UUID".to_owned())
    })
}

/// Devuelve el nombre del alta, o el `400` que lo nombra.
fn nombre_del_cable(nombre: &str) -> Result<String, AppError> {
    if nombre.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "el campo `name` no puede ir vacío".to_owned(),
        ));
    }

    Ok(nombre.to_owned())
}

/// Traduce y valida los scopes del cable.
///
/// Tienen que ser uno o más del vocabulario cerrado —`read`, `run`— y nada más: un valor fuera
/// de él contesta `400` nombrando el campo y sin repetir el valor. Las repeticiones no son un
/// error y se colapsan, porque el permiso es un **conjunto**.
fn scopes_del_cable(textos: &[String]) -> Result<Vec<Scope>, AppError> {
    if textos.is_empty() {
        return Err(AppError::InvalidInput(
            "el campo `scopes` tiene que traer al menos un permiso: `read` o `run`".to_owned(),
        ));
    }

    let mut scopes: Vec<Scope> = Vec::new();

    for texto in textos {
        let scope = Scope::desde_texto(texto).ok_or_else(|| {
            AppError::InvalidInput(
                "el campo `scopes` no admite ese valor: se espera `read` o `run`".to_owned(),
            )
        })?;

        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }

    Ok(scopes)
}

/// Traduce la caducidad del cable, o el `400` que nombra el campo.
fn caducidad_del_cable(expires_at: Option<&str>) -> Result<Option<OffsetDateTime>, AppError> {
    match expires_at {
        Some(valor) => OffsetDateTime::parse(valor, &Rfc3339)
            .map(Some)
            .map_err(|_| {
                AppError::InvalidInput(
                    "el campo `expires_at` no tiene la forma de una fecha RFC 3339".to_owned(),
                )
            }),
        None => Ok(None),
    }
}

/// `POST /api/v1/tokens` — emite un token y enseña su valor en claro **una sola vez**.
pub async fn alta(
    State(estado): State<AppState>,
    Json(cuerpo): Json<AltaDeTokenDelCable>,
) -> Result<ApiResponse, AppError> {
    let name = nombre_del_cable(&cuerpo.name)?;
    let scopes = scopes_del_cable(&cuerpo.scopes)?;
    let expires_at = caducidad_del_cable(cuerpo.expires_at.as_deref())?;

    // El valor se emite **antes** de tocar la base: si el `OsRng` falla, no se ha escrito nada.
    let en_claro = token_nuevo()?;

    // A la base va **solo el hash**, y el valor en claro sigue su camino hacia la respuesta sin
    // volver a pasar por ningún sitio que lo pueda imprimir.
    let fila = estado
        .db
        .tokens
        .alta(AltaDeToken {
            name,
            en_claro: en_claro.clone(),
            scopes,
            expires_at,
        })
        .await?;

    let vista = TokenEmitidoDelCable::desde(&fila, en_claro)?;

    Ok(ApiResponse::new(
        StatusCode::CREATED,
        "token creado; guárdelo ahora, no se volverá a mostrar",
        contenido(&vista)?,
    ))
}

/// `GET /api/v1/tokens` — lista los tokens, **sin** el valor en claro ni el hash.
pub async fn listado(State(estado): State<AppState>) -> Result<ApiResponse, AppError> {
    let filas = estado.db.tokens.listar().await?;

    let vistas: Vec<TokenDelCable> = filas
        .iter()
        .map(TokenDelCable::desde)
        .collect::<Result<_, _>>()?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "tokens listados",
        contenido(&vistas)?,
    ))
}

/// `DELETE /api/v1/tokens/{id}` — revoca un token y surte efecto en la petición siguiente.
///
/// Contesta `200` con la envoltura y `data` nulo, **no** `204`: la regla de la casa
/// —`api-resources`— exige que **toda** respuesta sea JSON con `{status, message, data}`, y un
/// `204` no tiene cuerpo. La operación es idempotente —revocar un token ya revocado no es un
/// error, y la fecha de la primera revocación no se mueve— y también lo es sobre un
/// identificador que no existe. Distinguir «no existe» con un `404` haría que el reintento de un
/// `DELETE` que ya había surtido efecto fallara, que es justo lo que la idempotencia evita: el
/// estado final —«ese token no vale»— se alcanza igual en los dos casos.
pub async fn revocar(
    State(estado): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    estado.db.tokens.revocar(&uuid).await?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "token revocado",
        Data::None,
    ))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::api::api_router;
    use crate::db::entities::{Scope, hash_de_token};
    use crate::test_support::{
        body_json, body_text, cookie_de_sesion, delete, delete_with_bearer, delete_with_cookie,
        estado_desarrollo, estado_oidc, get, get_with_bearer, get_with_cookie, post,
        post_json_with_bearer, post_json_with_cookie, router_desarrollo, router_oidc,
    };

    /// Un identificador bien formado cualquiera.
    ///
    /// No hace falta que exista: en modo `Disabled` la ruta de revocación ni se monta, así que
    /// lo único que importa es que la forma sea válida —un identificador mal formado lo
    /// rechazaría el extractor con un `400`, que taparía el `404` que se busca—.
    const UN_UUID: &str = "00000000-0000-4000-8000-000000000000";

    /// Crea un token por la API con la sesión de prueba y devuelve su valor en claro.
    async fn crear_token(router: axum::Router, scopes: &serde_json::Value) -> String {
        let respuesta = post_json_with_cookie(
            router,
            "/api/v1/tokens",
            &serde_json::json!({"name": "agente-de-prueba", "scopes": scopes}),
            &cookie_de_sesion(),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta del token de prueba tiene que contestar 201"
        );

        body_json(respuesta).await["data"]["token"]
            .as_str()
            .expect("el alta tiene que devolver el valor en claro")
            .to_owned()
    }

    #[tokio::test]
    async fn al_crear_un_token_se_devuelve_el_valor_en_claro_una_sola_vez() {
        // api-tokens, escenario «Al crear un token se devuelve el valor en claro una sola vez».
        let estado = estado_oidc().await;

        let respuesta = post_json_with_cookie(
            api_router(estado.clone()),
            "/api/v1/tokens",
            &serde_json::json!({"name": "agente-lector", "scopes": ["read"]}),
            &cookie_de_sesion(),
        )
        .await;

        assert_eq!(respuesta.status(), StatusCode::CREATED);

        let cuerpo = body_json(respuesta).await;
        let en_claro = cuerpo["data"]["token"]
            .as_str()
            .expect("el alta tiene que traer el token en claro")
            .to_owned();

        assert!(
            en_claro.starts_with("kesto_"),
            "el token tiene que llevar un prefijo reconocible: {en_claro}"
        );
        assert!(
            en_claro.len() >= "kesto_".len() + 43,
            "el token tiene que llevar entropía suficiente: {en_claro}"
        );

        // La base guarda **solo el hash**, y ni el valor en claro ni el hash son el uno el otro.
        let filas = estado
            .db
            .tokens
            .listar()
            .await
            .expect("el listado no falla");

        assert_eq!(filas.len(), 1, "tiene que haber un token");
        assert_eq!(filas[0].name, "agente-lector");
        assert_eq!(filas[0].scopes, vec![Scope::Read]);
        assert_eq!(
            filas[0].token_hash,
            hash_de_token(&en_claro, estado.db.clave.api_token_key())
                .expect("el hash de prueba no puede fallar"),
            "la fila no guardó el hash del token entregado"
        );
        assert_ne!(
            filas[0].token_hash, en_claro,
            "la fila guardó el valor en claro en vez de su hash"
        );

        // Y el listado por la API no lo devuelve, ni el valor ni el hash.
        let listado = get_with_cookie(
            api_router(estado.clone()),
            "/api/v1/tokens",
            &cookie_de_sesion(),
        )
        .await;

        let texto = body_text(listado).await;

        assert!(
            texto.contains("agente-lector"),
            "el listado tiene que traer el nombre: {texto}"
        );
        assert!(
            !texto.contains(&en_claro),
            "el listado no puede devolver el valor en claro: {texto}"
        );
        assert!(
            !texto.contains(
                &hash_de_token(&en_claro, estado.db.clave.api_token_key())
                    .expect("el hash de prueba no puede fallar")
            ),
            "el listado no puede devolver el hash: {texto}"
        );
        assert!(
            !texto.contains("token_hash") && !texto.contains("\"token\""),
            "el listado no puede llevar campo de token ni de hash: {texto}"
        );
    }

    #[tokio::test]
    async fn el_listado_trae_identificador_nombre_scopes_fechas_y_estado() {
        // api-tokens, escenario «El listado no devuelve el token».
        let estado = estado_oidc().await;
        let en_claro = crear_token(api_router(estado.clone()), &serde_json::json!(["run"])).await;

        let listado = get_with_cookie(
            api_router(estado.clone()),
            "/api/v1/tokens",
            &cookie_de_sesion(),
        )
        .await;

        assert_eq!(listado.status(), StatusCode::OK);

        let cuerpo = body_json(listado).await;
        let filas = cuerpo["data"].as_array().expect("el listado es un array");

        assert_eq!(filas.len(), 1);
        assert!(filas[0]["id"].is_string(), "falta el identificador");
        assert_eq!(filas[0]["name"], "agente-de-prueba");
        assert_eq!(filas[0]["scopes"], serde_json::json!(["run"]));
        assert!(
            filas[0]["created_at"].is_string(),
            "falta la fecha de emisión"
        );
        assert!(
            filas[0]["expires_at"].is_null(),
            "un token sin caducidad tiene que traer `expires_at` a null"
        );
        assert!(
            filas[0]["revoked_at"].is_null(),
            "un token en vigor tiene que traer `revoked_at` a null"
        );
        assert!(
            filas[0]["last_used_at"].is_null(),
            "un token sin usar tiene que traer `last_used_at` a null"
        );

        // El valor en claro, por si acaso, no está en ninguna parte del cuerpo.
        assert!(!cuerpo.to_string().contains(&en_claro));
    }

    #[tokio::test]
    async fn el_token_read_consulta_la_lista_de_tokens_y_el_run_tambien() {
        // El listado es una **lectura** de la sesión, no del token: aquí se comprueba que la
        // identidad de la sesión basta para verlo.
        let estado = estado_oidc().await;
        crear_token(api_router(estado.clone()), &serde_json::json!(["read"])).await;

        let respuesta = get_with_cookie(
            api_router(estado.clone()),
            "/api/v1/tokens",
            &cookie_de_sesion(),
        )
        .await;

        assert_eq!(respuesta.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn la_revocacion_deja_de_aceptarlo_en_la_peticion_siguiente() {
        // api-tokens, escenario «Un token revocado deja de funcionar en la petición siguiente».
        let estado = estado_oidc().await;
        let en_claro = crear_token(api_router(estado.clone()), &serde_json::json!(["read"])).await;

        let fila = estado
            .db
            .tokens
            .buscar_por_hash(
                &hash_de_token(&en_claro, estado.db.clave.api_token_key())
                    .expect("el hash de prueba no puede fallar"),
            )
            .await
            .expect("la búsqueda no falla")
            .expect("el token tiene que estar");
        let id = fila.id.hyphenated().to_string();

        // Antes de revocar, el token vale.
        let antes = get_with_bearer(
            api_router(estado.clone()),
            "/api/v1/repositories",
            &en_claro,
        )
        .await;

        assert_eq!(antes.status(), StatusCode::OK);

        // La revocación, con la sesión (un token no administra tokens).
        let revocacion = delete_with_cookie(
            api_router(estado.clone()),
            &format!("/api/v1/tokens/{id}"),
            &cookie_de_sesion(),
        )
        .await;

        assert_eq!(
            revocacion.status(),
            StatusCode::OK,
            "la revocación contesta 200 con la envoltura, no 204"
        );

        // Y en la petición siguiente, el mismo token ya no vale.
        let despues = get_with_bearer(
            api_router(estado.clone()),
            "/api/v1/repositories",
            &en_claro,
        )
        .await;

        assert_eq!(
            despues.status(),
            StatusCode::UNAUTHORIZED,
            "un token revocado tiene que dejar de valer de inmediato"
        );

        // La fila sigue en la base, con su fecha de revocación.
        let fila = estado
            .db
            .tokens
            .buscar_por_hash(
                &hash_de_token(&en_claro, estado.db.clave.api_token_key())
                    .expect("el hash de prueba no puede fallar"),
            )
            .await
            .expect("la búsqueda no falla")
            .expect("la fila tiene que seguir en la base para poder auditarla");

        assert!(
            fila.revoked_at.is_some(),
            "la revocación tiene que dejar su fecha"
        );

        // Y revocar otra vez no es un error.
        let otra_vez = delete_with_cookie(
            api_router(estado),
            &format!("/api/v1/tokens/{id}"),
            &cookie_de_sesion(),
        )
        .await;

        assert_eq!(
            otra_vez.status(),
            StatusCode::OK,
            "revocar dos veces tiene que ser idempotente"
        );
    }

    #[tokio::test]
    async fn un_token_no_puede_crear_listar_ni_revocar_tokens() {
        // api-tokens, escenario «Un token no puede crear otro token».
        let estado = estado_oidc().await;
        let en_claro = crear_token(api_router(estado.clone()), &serde_json::json!(["run"])).await;

        let fila = estado
            .db
            .tokens
            .buscar_por_hash(
                &hash_de_token(&en_claro, estado.db.clave.api_token_key())
                    .expect("el hash de prueba no puede fallar"),
            )
            .await
            .expect("la búsqueda no falla")
            .expect("el token tiene que estar");

        let antes = estado
            .db
            .tokens
            .listar()
            .await
            .expect("el listado no falla")
            .len();

        // Con el token como credencial: las tres rutas de gestión quedan fuera de su alcance.
        let crear = post_json_with_bearer(
            api_router(estado.clone()),
            "/api/v1/tokens",
            &serde_json::json!({"name": "escalada", "scopes": ["run"]}),
            &en_claro,
        )
        .await;
        let listar = get_with_bearer(api_router(estado.clone()), "/api/v1/tokens", &en_claro).await;
        let revocar = delete_with_bearer(
            api_router(estado.clone()),
            &format!("/api/v1/tokens/{}", fila.id.hyphenated()),
            &en_claro,
        )
        .await;

        for (donde, respuesta) in [
            ("el alta de otro token", crear),
            ("el listado de tokens", listar),
            ("la revocación de otro token", revocar),
        ] {
            assert_eq!(
                respuesta.status(),
                StatusCode::FORBIDDEN,
                "{donde} queda fuera del alcance de cualquier token"
            );
        }

        // Y no se ha creado ningún token nuevo.
        let despues = estado
            .db
            .tokens
            .listar()
            .await
            .expect("el listado no falla")
            .len();

        assert_eq!(despues, antes, "un token no puede haber creado otro");
    }

    #[tokio::test]
    async fn en_modo_disabled_la_gestion_de_tokens_no_esta_montada() {
        // api-tokens, escenario «En modo disabled la gestión de tokens no está montada».
        //
        // Sin proveedor de identidad no hay sesión que exigir, así que una ruta de gestión
        // solo puede quedarse abierta —y emitir credenciales sin credencial alguna— o no
        // existir. Se elige lo segundo: las tres rutas contestan el `404` de la envoltura,
        // como cualquier ruta desconocida, y el alta no llega a escribir ninguna fila.
        let estado = estado_desarrollo().await;
        let router = api_router(estado.clone());

        for (metodo, uri) in [
            ("GET", "/api/v1/tokens".to_owned()),
            ("POST", "/api/v1/tokens".to_owned()),
            ("DELETE", format!("/api/v1/tokens/{UN_UUID}")),
        ] {
            let respuesta = match metodo {
                "GET" => get(router.clone(), &uri).await,
                "POST" => post(router.clone(), &uri).await,
                _ => delete(router.clone(), &uri).await,
            };

            let status = respuesta.status();
            let texto = body_text(respuesta).await;

            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "{metodo} {uri} tiene que contestar 404 en modo disabled y contestó {status} \
                 con {texto}"
            );

            let cuerpo: serde_json::Value =
                serde_json::from_str(&texto).expect("el 404 tiene que traer JSON en el cuerpo");

            assert_eq!(cuerpo["status"], 404, "la envoltura del 404: {texto}");
            assert!(
                cuerpo["message"]
                    .as_str()
                    .is_some_and(|mensaje| !mensaje.is_empty()),
                "el 404 tiene que traer un mensaje: {texto}"
            );
        }

        // Y el alta rechazada no ha emitido ningún token: no queda ninguna fila.
        let filas = estado
            .db
            .tokens
            .listar()
            .await
            .expect("el listado no falla");

        assert!(
            filas.is_empty(),
            "en modo disabled el alta no puede emitir ningún token"
        );
    }

    #[tokio::test]
    async fn en_modo_disabled_un_bearer_invalido_no_bloquea_el_resto() {
        // api-tokens, escenario «En modo disabled un Bearer inválido no bloquea».
        //
        // El endurecimiento retira la **gestión** de tokens de este modo, no la capa de
        // recursos: en modo disabled no se monta ninguna capa, así que el resto de la
        // superficie sigue abierta y un Bearer inválido no la bloquea. Es el contrapeso de la
        // prueba de arriba: si el modo dejara de montar la superficie entera, ésta lo diría.
        let router = router_desarrollo().await;

        let respuesta = get_with_bearer(router, "/api/v1/repositories", "basura").await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "en modo disabled un Bearer inválido no puede bloquear"
        );
    }

    #[tokio::test]
    async fn los_scopes_fuera_del_vocabulario_o_vacios_contestan_400() {
        let estado = estado_oidc().await;

        for (donde, scopes) in [
            (
                "un scope fuera del vocabulario",
                serde_json::json!(["admin"]),
            ),
            ("una lista vacía", serde_json::json!([])),
        ] {
            let respuesta = post_json_with_cookie(
                api_router(estado.clone()),
                "/api/v1/tokens",
                &serde_json::json!({"name": "malo", "scopes": scopes}),
                &cookie_de_sesion(),
            )
            .await;

            let status = respuesta.status();
            let texto = body_text(respuesta).await;

            assert_eq!(status, StatusCode::BAD_REQUEST, "{donde} contesta {status}");
            assert!(
                texto.contains("scopes"),
                "{donde}: el 400 tiene que nombrar el campo: {texto}"
            );
        }

        // Y no se ha creado ningún token con esos cuerpos.
        let filas = estado
            .db
            .tokens
            .listar()
            .await
            .expect("el listado no falla");

        assert!(filas.is_empty(), "un alta rechazada no puede crear fila");
    }

    #[tokio::test]
    async fn un_nombre_vacio_o_una_caducidad_mal_formada_contestan_400() {
        let estado = estado_oidc().await;

        let vacio = post_json_with_cookie(
            api_router(estado.clone()),
            "/api/v1/tokens",
            &serde_json::json!({"name": "  ", "scopes": ["read"]}),
            &cookie_de_sesion(),
        )
        .await;

        assert_eq!(vacio.status(), StatusCode::BAD_REQUEST);

        let mal_formada = post_json_with_cookie(
            api_router(estado.clone()),
            "/api/v1/tokens",
            &serde_json::json!({
                "name": "agente",
                "scopes": ["read"],
                "expires_at": "no-es-una-fecha",
            }),
            &cookie_de_sesion(),
        )
        .await;

        let texto = body_text(mal_formada).await;

        assert!(
            texto.contains("expires_at"),
            "el 400 tiene que nombrar el campo: {texto}"
        );
    }

    #[tokio::test]
    async fn un_token_con_caducidad_futura_se_emite_y_la_traduce_a_rfc3339() {
        let estado = estado_oidc().await;

        let respuesta = post_json_with_cookie(
            api_router(estado.clone()),
            "/api/v1/tokens",
            &serde_json::json!({
                "name": "agente",
                "scopes": ["read", "run"],
                "expires_at": "2030-01-01T00:00:00Z",
            }),
            &cookie_de_sesion(),
        )
        .await;

        assert_eq!(respuesta.status(), StatusCode::CREATED);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["data"]["scopes"], serde_json::json!(["read", "run"]));
        assert_eq!(cuerpo["data"]["expires_at"], "2030-01-01T00:00:00Z");
    }

    #[tokio::test]
    async fn la_revocacion_de_un_identificador_mal_formado_contesta_400() {
        let estado = estado_oidc().await;

        let respuesta = delete_with_cookie(
            api_router(estado),
            "/api/v1/tokens/no-es-un-uuid",
            &cookie_de_sesion(),
        )
        .await;

        assert_eq!(respuesta.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn en_modo_oidc_la_gestion_sin_credencial_contesta_401() {
        let respuesta = get(router_oidc().await, "/api/v1/tokens").await;

        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);
    }
}
