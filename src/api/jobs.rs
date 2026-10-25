//! Las rutas de `/api/v1/jobs`: las cinco del recurso del §5.
//!
//! Es el recurso de `backup_jobs`, la tarea programada del §3: un `cron_expression` que dispara una
//! copia, la retención de esa copia —`keep_daily`, `keep_weekly` y `keep_monthly`— y un `enabled`
//! que la suspende sin borrarla. Cada tarea **apunta a un target y a un repositorio**, y ésas son
//! sus dos únicas referencias: el target dice de dónde se extrae y el repositorio, a dónde se sube.
//!
//! Es el único recurso que **valida sus referencias antes de escribir**, y no por gusto: la tabla
//! del §3 tiene **dos** claves foráneas, y SQLite contesta «FOREIGN KEY constraint failed» a
//! cualquiera de las dos **sin decir cuál falló**. Sin la comprobación previa, un identificador que
//! no existe se traduce en un `500` genérico —el diagnóstico exactamente equivocado para lo que es
//! un formulario mal rellenado— o en un `409` que no puede decir qué corregir. La comprobación
//! previa es la única que puede contestar un `400` que **nombre el campo**, que es lo que hace
//! falta para arreglarlo. La clave foránea sigue siendo la última línea, y no sobra: entre la
//! comprobación y el `INSERT` cabe otra petición que borre la fila referenciada, y entonces quien
//! tiene que negarse es la base.
//!
//! Los tipos de este módulo son los **del cable** (sufijo `DelCable`) y los dos identificadores
//! viajan como texto en su forma canónica con guiones: es la forma que la capa de acceso escribe y
//! lee —nunca un `Uuid` de `sqlx` contra una columna `TEXT`, que dejaría el valor como un BLOB de
//! dieciséis bytes ilegible con `sqlite3`— y la que hace legible la tabla a mano.
//!
//! Las cinco rutas del recurso están implementadas en el módulo 2 del change `api-endpoints`.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::contenido;
use crate::api::extract::Json;
use crate::api::response::{ApiResponse, Data};
use crate::db::entities::{AltaDeTarea, Tarea};
use crate::error::AppError;
use crate::state::AppState;

/// Las cinco rutas del recurso, para colgar de la mitad de negocio.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/jobs", get(listado).post(alta))
        .route(
            "/api/v1/jobs/{id}",
            get(leer).put(actualizar).delete(borrar),
        )
}

/// El cuerpo del alta de una tarea, tal y como llega por el cable.
///
/// Los dos identificadores viajan como **texto** en su forma canónica con guiones y los nombres de
/// los campos son los del §3: `target_id`, `repository_id`, `cron_expression`, `keep_daily`,
/// `keep_weekly`, `keep_monthly` y `enabled`, que son los que el panel pinta en su formulario y los
/// que hacen legible la tabla con `sqlite3`. `deny_unknown_fields` es lo que convierte una errata
/// del formulario —un `keep_dailly` de más— en un `400`: sin él, serde ignoraría el campo que no
/// conoce y la retención se quedaría en su valor por defecto **sin que la respuesta dijera una
/// palabra**, que es el peor de los desenlaces posibles: una tarea que guarda otra cosa que la que
/// el operador cree haber programado.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AltaDeTareaDelCable {
    /// Identificador del target del que se extrae; tiene que existir.
    pub target_id: String,
    /// Identificador del repositorio al que se sube; tiene que existir.
    pub repository_id: String,
    /// Expresión cron que programa la copia.
    pub cron_expression: String,
    /// Copias diarias que se conservan.
    pub keep_daily: i64,
    /// Copias semanales que se conservan.
    pub keep_weekly: i64,
    /// Copias mensuales que se conservan.
    pub keep_monthly: i64,
    /// Si la tarea está activa.
    pub enabled: bool,
}

/// Una tarea tal y como viaja por el cable: lo que devuelve la API.
///
/// Las claves son **exactamente** las del §3 —los dos identificadores, el cron, la retención, el
/// `enabled` y la fecha del alta—, para que el formulario, el cuerpo de la petición y la columna de
/// la tabla se llamen igual; una vista que inventara un campo rompería esa correspondencia sin que
/// nada lo notara. Y no hay ninguna credencial que callar aquí: la tarea no guarda ninguna, porque
/// las dos que hay en el sistema viven en su target y en su repositorio, que son las filas a las que
/// ésta apunta.
#[derive(Debug, Serialize)]
pub struct TareaDelCable {
    /// Identificador en su forma canónica con guiones.
    pub id: String,
    /// Identificador del target, con guiones.
    pub target_id: String,
    /// Identificador del repositorio, con guiones.
    pub repository_id: String,
    /// Expresión cron.
    pub cron_expression: String,
    /// Copias diarias que se conservan.
    pub keep_daily: i64,
    /// Copias semanales que se conservan.
    pub keep_weekly: i64,
    /// Copias mensuales que se conservan.
    pub keep_monthly: i64,
    /// Si la tarea está activa.
    pub enabled: bool,
    /// Fecha de alta, en RFC3339.
    pub created_at: String,
}

impl TareaDelCable {
    /// Vuelca una fila de la capa de acceso en su forma del cable.
    ///
    /// Es privada porque los cinco manejadores viven en este módulo: no hay ningún llamador
    /// fuera, así que publicarla solo ensancharía la superficie sin que nada la necesite.
    fn desde(fila: &Tarea) -> Result<Self, AppError> {
        Ok(Self {
            id: fila.id.hyphenated().to_string(),
            target_id: fila.target_id.hyphenated().to_string(),
            repository_id: fila.repository_id.hyphenated().to_string(),
            cron_expression: fila.cron_expression.clone(),
            keep_daily: fila.keep_daily,
            keep_weekly: fila.keep_weekly,
            keep_monthly: fila.keep_monthly,
            enabled: fila.enabled,
            created_at: fila.created_at.format(&Rfc3339).map_err(|error| {
                AppError::Other(format!("la fecha de la fila no se pudo formatear: {error}"))
            })?,
        })
    }
}

/// Convierte el identificador de la ruta en un `Uuid`, o contesta `400`.
///
/// El mensaje **nombra el campo y no repite el valor**: el segmento de la ruta llegó por el
/// cable, y la regla de la casa es que un `400` diga qué campo está mal, nunca lo que traía.
fn identificador(id: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(id).map_err(|_| {
        AppError::InvalidInput("el campo `id` de la ruta no tiene la forma de un UUID".to_owned())
    })
}

/// El `404` de una fila que no está, con el mismo texto en las tres rutas del identificador.
fn tarea_ausente() -> AppError {
    AppError::NotFound("no existe ninguna tarea con ese identificador".to_owned())
}

/// Convierte una referencia del cuerpo en un `Uuid`, o contesta `400` nombrando su campo.
///
/// Es un `400` y no un `500` porque lo que llegó por el cable es una cadena que no puede ser la
/// forma canónica de ningún identificador, y eso es una petición mal formada. El mensaje nombra
/// el campo y **no** repite el valor.
fn referencia(campo: &'static str, valor: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(valor).map_err(|_| {
        AppError::InvalidInput(format!("el campo `{campo}` no tiene la forma de un UUID"))
    })
}

/// Resuelve el `target_id` del cuerpo: tiene que ser un UUID y tiene que existir.
///
/// La comprobación previa es la razón de ser de este recurso. La tabla del §3 tiene **dos** claves
/// foráneas y SQLite contesta «FOREIGN KEY constraint failed» a cualquiera de las dos **sin decir
/// cuál falló**, así que sin esta comprobación un identificador que no existe saldría como un `500`
/// genérico —el diagnóstico exactamente equivocado para un formulario mal rellenado— o como un
/// `409` que no puede decir qué corregir. La clave foránea sigue siendo la última línea: entre esta
/// lectura y el `INSERT` cabe otra petición que borre la fila, y entonces quien tiene que negarse es
/// la base.
async fn target_resuelto(estado: &AppState, valor: &str) -> Result<Uuid, AppError> {
    let uuid = referencia("target_id", valor)?;

    if estado.db.targets.leer(&uuid).await?.is_none() {
        return Err(AppError::InvalidInput(
            "el campo `target_id` no apunta a ningún target existente".to_owned(),
        ));
    }

    Ok(uuid)
}

/// Resuelve el `repository_id` del cuerpo: tiene que ser un UUID y tiene que existir.
///
/// Es la mitad simétrica de [`target_resuelto`] y se comprueba **después**: una referencia que no
/// resuelve se nombra por su campo, y el orden fija cuál se nombra cuando las dos fallan —el
/// target, que es el primero del formulario y el primero del §3—.
async fn repositorio_resuelto(estado: &AppState, valor: &str) -> Result<Uuid, AppError> {
    let uuid = referencia("repository_id", valor)?;

    if estado.db.repositorios.leer(&uuid).await?.is_none() {
        return Err(AppError::InvalidInput(
            "el campo `repository_id` no apunta a ningún repositorio existente".to_owned(),
        ));
    }

    Ok(uuid)
}

/// Valida el cuerpo y lo convierte en el alta de la capa de acceso.
///
/// Las dos referencias se resuelven aquí, y en este orden: el target primero y el repositorio
/// después. No es un detalle de estilo —cuando las dos fallan, el `400` nombra **el target**, y eso
/// lo fija una prueba— además de ser el orden del formulario y el del §3.
async fn alta_desde(
    estado: &AppState,
    cuerpo: &AltaDeTareaDelCable,
) -> Result<AltaDeTarea, AppError> {
    let target_id = target_resuelto(estado, &cuerpo.target_id).await?;
    let repository_id = repositorio_resuelto(estado, &cuerpo.repository_id).await?;

    Ok(AltaDeTarea {
        target_id,
        repository_id,
        cron_expression: cuerpo.cron_expression.clone(),
        keep_daily: cuerpo.keep_daily,
        keep_weekly: cuerpo.keep_weekly,
        keep_monthly: cuerpo.keep_monthly,
        enabled: cuerpo.enabled,
    })
}

/// `GET /api/v1/jobs` — el listado.
///
/// Una tabla sin filas es un listado de cero elementos, **no** un recurso ausente: contesta `200`
/// con un array vacío, no `404` y no un `data` nulo. Es el estado normal de un Kesto recién
/// instalado, y el panel no tiene por qué tratarlo como un error.
pub async fn listado(State(state): State<AppState>) -> Result<ApiResponse, AppError> {
    let filas = state.db.tareas.listar().await?;

    let vistas: Vec<TareaDelCable> = filas
        .iter()
        .map(TareaDelCable::desde)
        .collect::<Result<_, _>>()?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "tareas listadas",
        contenido(&vistas)?,
    ))
}

/// `POST /api/v1/jobs` — el alta.
///
/// El `201` trae el recurso con el `id` que **generó la capa de acceso** —quien llama no lo
/// informa—, y no escribe hasta que las dos referencias resuelven: un alta rechazada no puede dejar
/// fila, que es lo que hace inútil un rechazo que escriba primero.
pub async fn alta(
    State(state): State<AppState>,
    Json(cuerpo): Json<AltaDeTareaDelCable>,
) -> Result<ApiResponse, AppError> {
    let alta = alta_desde(&state, &cuerpo).await?;

    let fila = state.db.tareas.alta(alta).await?;
    let vista = TareaDelCable::desde(&fila)?;

    Ok(ApiResponse::new(
        StatusCode::CREATED,
        "tarea creada",
        contenido(&vista)?,
    ))
}

/// `GET /api/v1/jobs/{id}` — la lectura.
///
/// Un identificador que no es un UUID es un `400` que nombra el campo de la ruta y **no** repite el
/// valor; una fila que no está es un `404`. Distinguirlos importa: un `404` a una errata de teclado
/// le diría a quien pregunta que su tarea se ha borrado.
pub async fn leer(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    let Some(fila) = state.db.tareas.leer(&uuid).await? else {
        return Err(tarea_ausente());
    };

    let vista = TareaDelCable::desde(&fila)?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "tarea encontrada",
        contenido(&vista)?,
    ))
}

/// `PUT /api/v1/jobs/{id}` — la actualización, que es reemplazo completo.
///
/// El cuerpo sustituye a la fila entera, con las dos referencias revalidadas —una tarea que apunte a
/// un target borrado es una fila que el motor no sabrá usar—; el identificador y la fecha de alta
/// **no** se reescriben, porque el identificador no es editable y `created_at` es la fecha de
/// nacimiento de la fila, no un dato del formulario. Si la fila desaparece entre la lectura y la
/// escritura, contesta el mismo `404` que si no hubiera estado nunca: es la carrera que la lectura
/// previa no puede cerrar, y el desenlace tiene que ser el del contrato.
pub async fn actualizar(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(cuerpo): Json<AltaDeTareaDelCable>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    let Some(actual) = state.db.tareas.leer(&uuid).await? else {
        return Err(tarea_ausente());
    };

    let alta = alta_desde(&state, &cuerpo).await?;

    let fila = Tarea {
        id: actual.id,
        target_id: alta.target_id,
        repository_id: alta.repository_id,
        cron_expression: alta.cron_expression,
        keep_daily: alta.keep_daily,
        keep_weekly: alta.keep_weekly,
        keep_monthly: alta.keep_monthly,
        enabled: alta.enabled,
        created_at: actual.created_at,
    };

    if !state.db.tareas.actualizar(&fila).await? {
        return Err(tarea_ausente());
    }

    let vista = TareaDelCable::desde(&fila)?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "tarea actualizada",
        contenido(&vista)?,
    ))
}

/// `DELETE /api/v1/jobs/{id}` — el borrado, que contesta `200` con `data` nulo.
///
/// `data` nulo y **no** `204`: la envoltura de la API lleva `{status, message, data}` en **todas**
/// sus respuestas, y un `204` no tiene cuerpo —sería la única capaz de romperla—. Borrar una tarea
/// no arrastra nada: sus ejecuciones cuelgan de ella con `ON DELETE CASCADE` (§3), así que el
/// histórico de esa tarea se va con ella, que es lo que se quiere al desprogramar una copia.
pub async fn borrar(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    if !state.db.tareas.borrar(&uuid).await? {
        return Err(tarea_ausente());
    }

    Ok(ApiResponse::new(
        StatusCode::OK,
        "tarea borrada",
        Data::None,
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use axum::Router;
    use axum::http::StatusCode;
    use serde_json::Value;
    use uuid::Uuid;

    use crate::api::api_router;
    use crate::state::{AppState, EstadoDeBase};
    use crate::test_support::{
        base_con_clave, body_json, body_text, capturar_log, config_con_dsn, delete,
        estado_desarrollo, get, post_json, put_json,
    };

    /// Credencial distinguible que ningún rechazo puede ver salir.
    ///
    /// Es la que usa el SRS como ejemplo medido: `{"keep_daily": "AKIA…"}` produce
    /// `invalid type: string "AKIA…", expected i64`, y la credencial viaja dentro del mensaje.
    const CREDENCIAL_EN_UN_CAMPO_NUMERICO: &str = "AKIAIOSFODNN7EXAMPLE-credencial-numerica-0001";

    /// Da de alta un target de base de datos por la API y devuelve su identificador.
    ///
    /// Siembra **por el mismo camino que usará el panel** y no escribiendo la fila a mano: el alta
    /// de una tarea exige que el target exista, así que la prueba ejercita la comprobación de
    /// referencias contra una fila creada por la API de verdad. Un `INSERT` directo probaría la
    /// comprobación contra una fila que la API no habría aceptado.
    async fn target_de_prueba(router: &Router, nombre: &str) -> String {
        let respuesta = post_json(
            router.clone(),
            "/api/v1/targets",
            &serde_json::json!({
                "name": nombre,
                "target_type": "mariadb",
                "container_name": "contenedor-de-pruebas",
                "db_user": "usuario-de-pruebas",
                "db_password": "contrasena-de-pruebas-000000000001",
                "db_name": "base-de-pruebas",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el target de la siembra tiene que darse de alta antes de hablar de tareas, y contestó \
             {status} con {cuerpo}"
        );

        cuerpo["data"]["id"]
            .as_str()
            .expect("el alta del target debe devolver su identificador")
            .to_owned()
    }

    /// Da de alta un repositorio por la API y devuelve su identificador.
    ///
    /// Hermano de [`target_de_prueba`] y con el mismo motivo: la tarea apunta a las dos filas, así
    /// que las dos tienen que existir de verdad para que el alta llegue a escribir.
    async fn repositorio_de_prueba(router: &Router, nombre: &str) -> String {
        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &serde_json::json!({
                "name": nombre,
                "endpoint": format!("https://s3.example.test/{nombre}"),
                "bucket": format!("cubo-{nombre}"),
                "region": "eu-west-1",
                "access_key_id": "AKIAIOSFODNN7EXAMPLE",
                "secret_access_key": "AKIAIOSFODNN7EXAMPLE-secreto-de-pruebas",
                "passphrase": "passphrase-distinguible-00000000000000000000000001",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el repositorio de la siembra tiene que darse de alta antes de hablar de tareas, y \
             contestó {status} con {cuerpo}"
        );

        cuerpo["data"]["id"]
            .as_str()
            .expect("el alta del repositorio debe devolver su identificador")
            .to_owned()
    }

    /// El cuerpo de un alta de tarea, con las dos referencias que recibe.
    ///
    /// La tarea no tiene nombre —no hay nada que distinguir en el listado—, así que lo que
    /// parametriza el cuerpo son sus dos referencias: el target del que se extrae y el repositorio
    /// al que se sube. Los tres campos de retención llevan valores distintos a propósito, para que
    /// una vista que cruzara dos columnas no pasara inadvertida.
    fn cuerpo_de_tarea(target_id: &str, repository_id: &str) -> serde_json::Value {
        serde_json::json!({
            "target_id": target_id,
            "repository_id": repository_id,
            "cron_expression": "0 3 * * *",
            "keep_daily": 7,
            "keep_weekly": 4,
            "keep_monthly": 6,
            "enabled": true,
        })
    }

    /// 1.6 — Las cinco rutas del recurso contestan su código.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «Las cinco rutas del recurso contestan su
    /// código»). Las cinco peticiones salen del **mismo** router —el de producción, con su
    /// estado— y encadenadas: el alta devuelve el identificador que consumen las cuatro
    /// siguientes, así que una ruta mal colgada —o un método que no esté montado— se ve aquí y no
    /// en la primera petición que la use de verdad. Los dos recursos referenciados se siembran
    /// antes, porque el alta de una tarea no escribe si sus dos referencias no existen.
    #[tokio::test]
    async fn las_cinco_rutas_del_recurso_contestan_su_codigo() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-de-1-6").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-de-1-6").await;

        let respuesta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea(&target, &repositorio),
        )
        .await;

        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el alta tiene que contestar 201 con el recurso creado, y contestó {status}"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let respuesta = get(router.clone(), "/api/v1/jobs").await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado tiene que contestar 200 con un array en `data`, y contestó {status}"
        );
        let _ = body_json(respuesta).await;

        let respuesta = get(router.clone(), &format!("/api/v1/jobs/{id}")).await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::OK,
            "la lectura de una fila recién creada tiene que contestar 200, y contestó {status}"
        );
        let _ = body_json(respuesta).await;

        let mut actualizacion = cuerpo_de_tarea(&target, &repositorio);
        actualizacion["keep_daily"] = serde_json::json!(3);

        let respuesta = put_json(
            router.clone(),
            &format!("/api/v1/jobs/{id}"),
            &actualizacion,
        )
        .await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::OK,
            "la actualización tiene que contestar 200 con el recurso, y contestó {status}"
        );
        let _ = body_json(respuesta).await;

        let respuesta = delete(router, &format!("/api/v1/jobs/{id}")).await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::OK,
            "el borrado tiene que contestar 200 con `data` nulo, y contestó {status}"
        );
        let _ = body_json(respuesta).await;
    }

    /// 1.13j — El alta devuelve los campos con los nombres del §3.
    ///
    /// Cubre el requisito de `api-resources` sobre la correspondencia entre la vista del cable y
    /// las columnas del §3. Los nombres son los del esquema para que el formulario, el cuerpo y la
    /// columna se llamen igual, y una vista que inventara un campo rompería esa correspondencia sin
    /// que nada lo notara: por eso la prueba afirma el **conjunto exacto** de claves y no solo la
    /// presencia de las que espera. Las dos cosas que **no** pueden estar: ninguna credencial —la
    /// tarea no guarda ninguna, porque las dos del sistema viven en su target y en su repositorio—
    /// y ningún nombre inventado que el panel no sepa pintar.
    #[tokio::test]
    async fn el_alta_devuelve_los_campos_con_los_nombres_del_s3() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-de-1-13j").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-de-1-13j").await;

        let respuesta = post_json(
            router,
            "/api/v1/jobs",
            &cuerpo_de_tarea(&target, &repositorio),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el alta tiene que contestar 201 con el recurso creado, y contestó {status} con \
             {cuerpo}"
        );

        let claves: BTreeSet<&str> = cuerpo["data"]
            .as_object()
            .expect("el `data` del alta tiene que ser un objeto con la tarea del cable")
            .keys()
            .map(String::as_str)
            .collect();

        let esperadas: BTreeSet<&str> = BTreeSet::from([
            "created_at",
            "cron_expression",
            "enabled",
            "id",
            "keep_daily",
            "keep_monthly",
            "keep_weekly",
            "repository_id",
            "target_id",
        ]);

        assert_eq!(
            claves, esperadas,
            "la vista del cable tiene que traer exactamente los campos del §3, con sus nombres: \
             {cuerpo}"
        );
    }

    /// 1.29 — Una tarea con un target inexistente contesta `400` nombrando el campo.
    ///
    /// Cubre el requisito de `api-resources` sobre la validación de referencias, y es la mitad del
    /// porqué de este recurso: sin la comprobación previa, la violación de la clave foránea saldría
    /// como un `500` genérico —el diagnóstico exactamente equivocado para un formulario mal
    /// rellenado— o como un `409` que no puede decir **cuál** de las dos referencias falló, porque
    /// SQLite contesta «FOREIGN KEY constraint failed» a cualquiera de las dos. La base está
    /// migrada y **sin sembrar**: no hay ningún target, así que el identificador no tiene a qué
    /// apuntar. Se afirma además que la petición rechazada **no deja fila**: un rechazo que
    /// escribiera primero sería peor que no rechazar.
    #[tokio::test]
    async fn una_tarea_con_un_target_inexistente_contesta_400_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = AppState::new(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        )
        .expect("el estado se construye sobre la base de prueba");

        let cuerpo_de_la_peticion =
            cuerpo_de_tarea(&Uuid::new_v4().to_string(), &Uuid::new_v4().to_string());

        let respuesta = post_json(api_router(estado), "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "una referencia que no resuelve es un formulario mal rellenado, no un servidor roto: \
             contestó {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("target_id"),
            "el rechazo tiene que nombrar el campo que no resuelve —y la clave foránea no puede \
             decirlo—, y el mensaje fue {mensaje:?}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect(
                "la tabla tiene que ser contable sobre el fichero con el que se abrió el estado",
            );

        assert_eq!(
            filas, 0,
            "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
        );
    }

    /// 1.29b — Una tarea con un repositorio inexistente contesta `400` nombrando el campo.
    ///
    /// Es la mitad **simétrica** de la 1.29, y la que de verdad justifica la comprobación previa:
    /// aquí el target **sí** existe, así que lo único que falla es la segunda referencia. Sin
    /// comprobar una por una, las dos referencias se confundirían en el mismo mensaje, y quien
    /// tiene que arreglar el formulario no sabría cuál de las dos casillas corregir; el `400` de
    /// esta prueba nombra `repository_id` y no `target_id`.
    #[tokio::test]
    async fn una_tarea_con_un_repositorio_inexistente_contesta_400_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = AppState::new(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        )
        .expect("el estado se construye sobre la base de prueba");

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-de-1-29b").await;

        let cuerpo_de_la_peticion = cuerpo_de_tarea(&target, &Uuid::new_v4().to_string());

        let respuesta = post_json(router, "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "con el target resuelto, lo que no resuelve es el repositorio: contestó {status} con \
             {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("repository_id"),
            "el rechazo tiene que nombrar la referencia que falla —y no la otra—, y el mensaje fue \
             {mensaje:?}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect(
                "la tabla tiene que ser contable sobre el fichero con el que se abrió el estado",
            );

        assert_eq!(
            filas, 0,
            "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
        );
    }

    /// 1.29c — Un `target_id` que no es un UUID contesta `400` nombrando el campo.
    ///
    /// Un identificador que no parsea tampoco resuelve, así que la petición se atiende con el mismo
    /// `400` que nombra el campo y no con un `500` de conversión: lo que llegó por el cable es una
    /// cadena que no puede ser la forma canónica de ningún identificador, y eso es una petición mal
    /// formada. El mensaje nombra el campo y **no** repite el valor, que es la regla de la casa.
    #[tokio::test]
    async fn un_target_id_que_no_es_un_uuid_contesta_400_nombrando_el_campo() {
        let router = api_router(estado_desarrollo().await);

        let cuerpo_de_la_peticion = cuerpo_de_tarea("no-es-un-uuid", &Uuid::new_v4().to_string());

        let respuesta = post_json(router, "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "una cadena que no es un identificador es una petición mal formada: contestó {status} \
             con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("target_id"),
            "el rechazo tiene que nombrar el campo que no resuelve, y el mensaje fue {mensaje:?}"
        );
        assert!(
            !mensaje.contains("no-es-un-uuid"),
            "el rechazo nombra el campo, nunca el valor que traía: lo puso quien lo mandó, y \
             repetirlo lo llevaría al cuerpo de la respuesta y de ahí a cualquier registro \
             intermedio; el mensaje fue {mensaje:?}"
        );
    }

    /// 1.30j — Un campo de más en el cuerpo se rechaza.
    ///
    /// Cubre `api-resources`, requisito sobre el cuerpo del alta: `deny_unknown_fields` convierte
    /// una errata en un `400`. Sin el rechazo, serde ignoraría el campo que no conoce —el
    /// `keep_dailly` de la errata— y la retención se quedaría en su valor por defecto **sin que la
    /// respuesta dijera una palabra**: la tarea guardaría otra cosa que la que el operador cree
    /// haber programado, y el error solo se vería el día que falten copias. La base se cuenta al
    /// final para afirmar que el rechazo no escribió nada.
    #[tokio::test]
    async fn un_campo_de_mas_en_el_cuerpo_se_rechaza() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = AppState::new(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        )
        .expect("el estado se construye sobre la base de prueba");

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-de-1-30j").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-de-1-30j").await;

        let mut cuerpo_de_la_peticion = cuerpo_de_tarea(&target, &repositorio);
        cuerpo_de_la_peticion["keep_dailly"] = serde_json::json!(7);

        let respuesta = post_json(router, "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un campo que el §3 no conoce es una errata del formulario, y aceptarlo en silencio \
             guardaría otra cosa que la pedida: contestó {status} con {cuerpo}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect(
                "la tabla tiene que ser contable sobre el fichero con el que se abrió el estado",
            );

        assert_eq!(
            filas, 0,
            "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
        );
    }

    /// 1.24j — El rechazo de un campo con el tipo equivocado no publica la credencial.
    ///
    /// Es el vector que el propio SRS mide, y basta con que un campo se llame `keep_daily` y quien
    /// llama pegue la credencial en la casilla equivocada: el mensaje del intérprete
    /// —`invalid type: string "AKIA…", expected i64`— lleva el valor dentro. Por eso la respuesta
    /// pierde el texto del intérprete y se queda con la envoltura, y el registro no lo recibe
    /// tampoco: **queda constancia de que una petición no se pudo interpretar, sin su texto**, que
    /// es lo que hace falta para depurar sin repartir credenciales. El segundo vector es el mismo
    /// accidente por el otro lado —la credencial como **nombre** de un campo de más, que el
    /// mensaje de un campo desconocido repetiría—. La credencial es larga y con coletilla propia:
    /// un `contains` sobre el texto crudo no puede dar un falso positivo por casualidad.
    #[tokio::test]
    async fn el_rechazo_de_un_campo_con_el_tipo_equivocado_no_publica_la_credencial() {
        let (buffer, _guard) = capturar_log();

        let router = api_router(estado_desarrollo().await);

        let mut en_el_valor = cuerpo_de_tarea("target-de-1-24j", "repositorio-de-1-24j");
        en_el_valor["keep_daily"] = Value::String(CREDENCIAL_EN_UN_CAMPO_NUMERICO.to_owned());

        let mut en_el_nombre = cuerpo_de_tarea("target-de-1-24j", "repositorio-de-1-24j");
        en_el_nombre[CREDENCIAL_EN_UN_CAMPO_NUMERICO] = serde_json::json!(1);

        let vectores = [
            ("la credencial entera en un campo numérico", en_el_valor),
            ("la credencial como nombre de un campo de más", en_el_nombre),
        ];

        for (motivo, cuerpo_de_la_peticion) in vectores {
            let respuesta = post_json(router.clone(), "/api/v1/jobs", &cuerpo_de_la_peticion).await;

            let status = respuesta.status();
            let texto = body_text(respuesta).await;

            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "el rechazo tiene que salir por la envoltura ({motivo}); contestó {status} con \
                 {texto}"
            );

            assert!(
                !texto.contains(CREDENCIAL_EN_UN_CAMPO_NUMERICO),
                "el rechazo publicó la credencial al cliente ({motivo}): {texto}"
            );
        }

        let registrado = buffer.texto();

        assert!(
            !registrado.contains(CREDENCIAL_EN_UN_CAMPO_NUMERICO),
            "el rechazo escribió la credencial en el registro ({registrado})"
        );

        assert!(
            registrado.contains("no se pudo interpretar"),
            "tiene que quedar registrado que una petición no se pudo interpretar —sin su \
             texto—, y el registro capturado fue: {registrado}"
        );
    }

    /// 1.46j — Un identificador de tarea mal formado contesta `400`.
    ///
    /// Cubre el requisito de `api-resources` sobre los identificadores. Un identificador mal
    /// formado es una petición mal formada, no un recurso ausente: un `404` le diría a quien
    /// pregunta que su tarea se ha borrado cuando lo que hay es una errata de teclado, y un `500`
    /// culparía al servidor de una petición que no llegó a ser una consulta.
    #[tokio::test]
    async fn un_identificador_de_tarea_mal_formado_contesta_400() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = get(router, "/api/v1/jobs/no-es-un-uuid").await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un identificador que no es un UUID no puede ser un recurso ausente ni un fallo del \
             servidor: contestó {status} con {cuerpo}"
        );
        assert_eq!(
            cuerpo["status"], 400,
            "el rechazo tiene que llegar en la envoltura del resto de la API, con el código \
             dentro: {cuerpo}"
        );
        assert!(
            cuerpo["message"].is_string(),
            "la envoltura de error tiene que traer un `message` de texto: {cuerpo}"
        );
    }
}
