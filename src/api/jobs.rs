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
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::extract::Json;
use crate::api::response::{ApiResponse, Data};
use crate::api::{TAREA_QUE_NO_ESTA, contenido, del_motor};
use crate::db::entities::{AltaDeTarea, Ejecucion, Tarea};
use crate::error::AppError;
use crate::state::AppState;

/// Las rutas del recurso, para colgar de la mitad de negocio.
///
/// Son las cinco del §5 más la del disparo, y van todas en el **mismo** `Router`: la mitad
/// protegida las cuelga juntas —`api_router` mergea este router dentro de la superficie de
/// negocio—, así que añadir el disparo aquí es lo que lo deja detrás del *gate* de sesión y **no**
/// en la lista blanca. Una ruta declarada en la mitad pública no pasa por el *gate*.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/jobs", get(listado).post(alta))
        .route(
            "/api/v1/jobs/{id}",
            get(leer).put(actualizar).delete(borrar),
        )
        // El disparo es una **subruta**: cuelga del identificador de la tarea y con el verbo que el
        // recurso no declara —`{id}` tiene `GET`, `PUT` y `DELETE`, no `POST`—, así que no compite
        // con ninguna de las cinco y el enrutado de axum no ve dos caminos que se pisen.
        .route("/api/v1/jobs/{id}/run", post(disparar))
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

/// La ejecución que un disparo acaba de encolar, tal y como viaja por el cable.
///
/// Son **tres** campos y no los nueve de [`crate::api::executions`]: lo que el `202` entrega es un
/// asa para seguir la copia que se acaba de aceptar —su identificador, la tarea de la que es y en
/// qué estado ha nacido—, no una fila del histórico. Los otros seis campos serían mentira en el
/// momento de la respuesta —todavía no hay `snapshot_id`, ni bytes, ni fecha de fin— y el panel los
/// recibe en cuanto la ejecución avanza, por el histórico o por el stream.
///
/// El `status` sale como el texto del `CHECK` —`pending`, que es como nace— y los dos
/// identificadores en su forma canónica con guiones, igual que en el resto de la API.
#[derive(Debug, Serialize)]
pub struct EjecucionEncoladaDelCable {
    /// Identificador de la ejecución, que es lo que el panel seguirá.
    pub id: String,
    /// Tarea a la que pertenece.
    pub job_id: String,
    /// Estado en el que ha nacido, en el texto del `CHECK`.
    pub status: String,
}

impl EjecucionEncoladaDelCable {
    /// Vuelca la ejecución que devolvió el motor en su vista del cable.
    ///
    /// No devuelve `Result`, al revés que las otras vistas del recurso: no formatea ninguna fecha
    /// —es lo único que puede fallar en las demás— y los tres campos son cadenas que el motor ya
    /// tiene resueltas.
    fn desde(ejecucion: &Ejecucion) -> Self {
        Self {
            id: ejecucion.id.hyphenated().to_string(),
            job_id: ejecucion.job_id.hyphenated().to_string(),
            status: ejecucion.status.texto().to_owned(),
        }
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
    AppError::NotFound(TAREA_QUE_NO_ESTA.to_owned())
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

/// El `400` de una retención que no conservaría nada.
///
/// Con las **tres** reglas a cero no hay ninguna que conserve: la retención no conserva de menos,
/// **borra todo** el historial del repositorio en la copia siguiente, incluida la que se acaba de
/// hacer. El §3 pone un valor por defecto a las tres columnas pero **no una cota**, así que el valor
/// entra por esta puerta y entra a mano, y rechazarlo aquí es lo que evita que el error se cometa
/// sin querer. Un cero suelto acompañado de otra regla informada **sí** se acepta: es una decisión
/// del operador —«de esta regla no conserves ninguna»— y lo que no es una decisión es que las tres
/// sean cero.
///
/// # Por qué esta comprobación no es la del motor
///
/// El motor tiene el suyo —`rustic::retencion::reglas`, que devuelve
/// `rustic::Error::RetencionQueLoBorraTodo`— y **ninguna de las dos sobra**: protegen de dos caminos
/// distintos. Ésta protege el formulario, que es por donde entra una fila de verdad y donde el
/// operador puede corregir en la misma petición; la del motor protege la copia de una fila escrita
/// **directamente** en la base, que aquí no pasa y que el motor no puede rechazar al leerla sin
/// dejar la copia sin hacer. La de aquí es la que puede contestar un `400` que nombre las tres
/// columnas; la de allí es la que impide que la retención de una fila ya escrita borre el
/// historial.
fn retencion_que_conserva(cuerpo: &AltaDeTareaDelCable) -> Result<(), AppError> {
    if cuerpo.keep_daily == 0 && cuerpo.keep_weekly == 0 && cuerpo.keep_monthly == 0 {
        return Err(AppError::InvalidInput(
            "las tres reglas de retención (`keep_daily`, `keep_weekly` y `keep_monthly`) no pueden \
             ser cero a la vez: sin ninguna regla que conserve, la copia siguiente borraría todo el \
             historial del repositorio"
                .to_owned(),
        ));
    }

    Ok(())
}

/// Valida el cuerpo y lo convierte en el alta de la capa de acceso.
///
/// La retención se comprueba **antes de tocar la base**: un cuerpo que no puede ser válido por sí
/// mismo no necesita leer ninguna fila para rechazarse, y así el `400` de las tres reglas a cero no
/// depende de que las referencias existan.
///
/// Las dos referencias se resuelven después, y en este orden: el target primero y el repositorio
/// después. No es un detalle de estilo —cuando las dos fallan, el `400` nombra **el target**, y eso
/// lo fija una prueba— además de ser el orden del formulario y el del §3.
///
/// La usan **las dos** rutas que escriben una tarea, el alta y la edición: el requisito de la
/// retención es del par, y con la comprobación aquí no hay forma de añadir una tercera puerta de
/// escritura que se olvide de ella.
async fn alta_desde(
    estado: &AppState,
    cuerpo: &AltaDeTareaDelCable,
) -> Result<AltaDeTarea, AppError> {
    retencion_que_conserva(cuerpo)?;

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

/// `POST /api/v1/jobs/{id}/run` — el disparo manual de una tarea.
///
/// Es la sexta ruta del recurso y la única que no habla ni de la fila de la tarea ni de su
/// formulario: encola una copia **ahora**, sin esperar al `cron_expression`, y contesta `202` con la
/// ejecución que ha nacido. Un `202` y no un `201` porque la copia no está hecha, está aceptada; el
/// desenlace de verdad se sigue por el histórico o por el stream.
///
/// # El manejador traduce y nada más
///
/// Toda la decisión —que la tarea existe, que no tiene ya una ejecución en curso, en qué estado
/// nace la fila y en qué cola entra— vive en [`Motor::encolar`](crate::engine::Motor::encolar), y
/// aquí solo se convierte su desenlace en el código que le corresponde con
/// [`del_motor`]. Y eso incluye **no leer la tabla de ejecuciones para decidir**: la guarda del
/// `409` es **atómica** —vive en la capa de acceso, en `alta_si_no_hay_ninguna_en_curso`—, y una
/// lectura previa aquí reintroduciría la carrera que esa guarda cerró: entre la lectura y el
/// `INSERT` caben las peticiones de otro. Está medido: con la forma «leer y luego insertar» pasaban
/// **tres de seis** disparos simultáneos sobre la misma tarea.
///
/// # Una tarea desactivada también se dispara
///
/// Y no se mira `enabled`, a propósito. `enabled` gobierna al **planificador** —decide si la tarea
/// se dispara sola por su `cron_expression`—, y el disparo manual es otra cosa: es la mano del
/// operador. Un botón que se negara sin poder explicar que el problema está en el planificador
/// —que en este change ni siquiera existe— sería peor que uno que ejecuta. Hay una prueba que lo
/// fija, porque es el caso que más fácil se implementa al revés.
///
/// # Errors
///
/// `400` si el identificador de la ruta no es un UUID, `404` si la tarea no existe, `409` si ya
/// tiene una ejecución en curso y `503` si el motor ha dejado de aceptar trabajo o su cola está
/// llena; los tres últimos son la traducción de [`del_motor`].
pub async fn disparar(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    let ejecucion = state.motor.encolar(uuid).await.map_err(del_motor)?;

    let vista = EjecucionEncoladaDelCable::desde(&ejecucion);

    Ok(ApiResponse::new(
        StatusCode::ACCEPTED,
        "ejecución encolada",
        contenido(&vista)?,
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use axum::Router;
    use axum::http::StatusCode;
    use serde_json::Value;
    use sqlx::SqlitePool;
    use uuid::Uuid;

    use crate::api::api_router;
    use crate::db::entities::{
        AltaDeEjecucion, AltaDeRepositorio, AltaDeTarea, AltaDeTarget, Estado, TipoDeTarget,
    };
    use crate::secret::SecretString;
    use crate::state::{AppState, EstadoDeBase};
    use crate::test_support::{
        CLAVE_SECRETA_DE_PRUEBA, DirectorioDePrueba, PASSPHRASE_DE_PRUEBA, base_con_clave,
        body_json, body_text, capturar_log, config_con_dsn, delete, destino_local_de_prueba,
        directorio_de_prueba, estado_de_prueba, estado_desarrollo, get, post, post_json, put_json,
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
    /// que las dos tienen que existir de verdad para que el alta llegue a escribir. El `endpoint`
    /// es la ruta de un directorio temporal y no una URL de mentira porque el alta **habla con el
    /// destino**: con una URL que no existe, el alta fallaría por destino inalcanzable y esta
    /// siembra no sembraría nada.
    async fn repositorio_de_prueba(router: &Router, nombre: &str) -> String {
        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &serde_json::json!({
                "name": nombre,
                "endpoint": destino_local_de_prueba(nombre),
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
    async fn el_alta_devuelve_los_campos_con_los_nombres_del_esquema() {
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

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

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

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

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

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

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

    // ── El disparo manual ────────────────────────────────────────────────────
    //
    // Cubre `api-resources`, requisito «El disparo manual de una tarea encola y contesta 202, y 409
    // si ya está corriendo», y `engine`, requisito «El alta de una ejecución contesta 202 con la
    // ejecución en `pending`, y 409 si la tarea ya está corriendo». Aquí se mide **lo que la API
    // promete**; el ciclo de vida de la fila lo mide el motor.
    //
    // Toda la lógica —incluida la guarda atómica del `409`— vive en `Motor::encolar`, así que estos
    // manejadores no vuelven a leer la tabla para decidir: reintroducirían la carrera que esa
    // guarda cerró. Lo que se afirma aquí es la traducción de sus desenlaces a códigos.

    /// La cadena del §3 que un disparo necesita, sembrada por la capa de acceso, y el pool con el
    /// que se cuentan las filas.
    ///
    /// La tarea apunta a un **directorio** —que no pasa por Docker— y a un repositorio **local**,
    /// así que una ejecución que llegue a correr no necesita contenedores. El target es de tipo
    /// `mariadb` apuntando a un contenedor que no existe: es lo que hace que la copia falle **en el
    /// acto** y que la prueba no espere a que se copie nada —la conexión por el socket muerto no
    /// tarda— sin dejar una copia en vuelo cuando la prueba acabe.
    struct AparatoDelDisparo {
        /// La base: al soltarse se borra, y con ella la fila que se afirme.
        _base: DirectorioDePrueba,
        /// El destino del repositorio, vivo mientras corra la prueba.
        _destino: DirectorioDePrueba,
        /// El pool, para contar filas con SQL directo.
        pool: SqlitePool,
        /// El estado con el que se levanta el router.
        estado: AppState,
        /// La tarea que se dispara.
        tarea: Uuid,
    }

    impl AparatoDelDisparo {
        /// Siembra la cadena entera y devuelve el aparato.
        async fn nuevo() -> Self {
            let (base, pool, clave) = base_con_clave().await;

            let estado = estado_de_prueba(
                config_con_dsn(&base.dsn()),
                EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
            );

            let destino = directorio_de_prueba();

            let target = estado
                .db
                .targets
                .alta(AltaDeTarget {
                    name: "target-del-disparo".to_owned(),
                    target_type: TipoDeTarget::Mariadb,
                    container_name: Some("kesto-prueba-que-no-existe".to_owned()),
                    db_user: Some("usuario-de-pruebas".to_owned()),
                    db_password: Some(SecretString::new(
                        "contrasena-de-pruebas-000000000001".to_owned(),
                    )),
                    db_name: Some("base-de-pruebas".to_owned()),
                    host_path: None,
                })
                .await
                .expect("el target de la siembra se da de alta");

            let repositorio = estado
                .db
                .repositorios
                .alta(AltaDeRepositorio {
                    name: "repositorio-del-disparo".to_owned(),
                    endpoint: destino.ruta().to_string_lossy().into_owned(),
                    bucket: "cubo-del-disparo".to_owned(),
                    region: "region-de-prueba".to_owned(),
                    access_key_id: "clave-de-acceso-de-prueba".to_owned(),
                    secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
                    passphrase: SecretString::new(PASSPHRASE_DE_PRUEBA.to_owned()),
                })
                .await
                .expect("el repositorio de la siembra se da de alta");

            let tarea = estado
                .db
                .tareas
                .alta(AltaDeTarea {
                    target_id: target.id,
                    repository_id: repositorio.id,
                    cron_expression: "0 3 * * *".to_owned(),
                    keep_daily: 7,
                    keep_weekly: 4,
                    keep_monthly: 12,
                    enabled: true,
                })
                .await
                .expect("la tarea de la siembra se da de alta");

            Self {
                _base: base,
                _destino: destino,
                pool,
                estado,
                tarea: tarea.id,
            }
        }

        /// El router de producción sobre la base sembrada.
        fn router(&self) -> Router {
            api_router(self.estado.clone())
        }

        /// Cuántas ejecuciones hay en la tabla.
        async fn ejecuciones(&self) -> i64 {
            sqlx::query_scalar("SELECT COUNT(*) FROM job_executions")
                .fetch_one(&self.pool)
                .await
                .expect("la tabla de ejecuciones tiene que poder contarse")
        }
    }

    /// El disparo contesta `202` con la ejecución creada, y la fila queda escrita.
    ///
    /// Cubre el escenario «El disparo contesta 202 con la ejecución»: `202` y no `201` —la
    /// ejecución no está hecha, está aceptada—, con el `id` de la ejecución y su `job_id` en el
    /// `data`, que es lo que el panel necesita para seguirla.
    ///
    /// La fila se lee con SQL directo, y su estado puede ser ya `running`: el trabajador la toma en
    /// cuanto entra en la cola, así que afirmar `pending` sobre la **tabla** sería una carrera. Lo
    /// que sí se afirma de la fila es que existe, que está en uno de los dos estados en curso y que
    /// acaba —en `failed`, porque el contenedor del target no existe— sin quedarse en `running`,
    /// que es la peor de las salidas posibles para el panel.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_disparo_contesta_202_con_la_ejecucion_encolada() {
        let aparato = AparatoDelDisparo::nuevo().await;

        let respuesta = post(
            aparato.router(),
            &format!("/api/v1/jobs/{}/run", aparato.tarea),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::ACCEPTED,
            "el disparo tiene que contestar 202 —aceptada, no creada— y contestó {status} con \
             {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["job_id"],
            aparato.tarea.hyphenated().to_string(),
            "el `data` del disparo tiene que traer el `job_id` de la ejecución: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["status"], "pending",
            "la ejecución del disparo nace en `pending`: {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el disparo tiene que devolver el identificador de la ejecución")
            .to_owned();

        assert_eq!(
            aparato.ejecuciones().await,
            1,
            "el disparo tiene que dejar exactamente una ejecución escrita"
        );

        let estado_escrito: String =
            sqlx::query_scalar("SELECT status FROM job_executions WHERE id = ?1")
                .bind(&id)
                .fetch_one(&aparato.pool)
                .await
                .expect("la fila de la ejecución tiene que estar en la base");

        assert!(
            ["pending", "running"].contains(&estado_escrito.as_str()),
            "la fila del disparo tiene que estar en curso, y está en `{estado_escrito}`"
        );

        // Y ninguna ejecución se queda en `running`: el trabajo acaba, aunque sea fallando.
        let estado_final: String = esperar_el_final(&aparato.pool, &id).await;

        assert_ne!(
            estado_final, "running",
            "la ejecución del disparo se quedó en `running`: el panel la pintaría como en marcha \
             para siempre"
        );
    }

    /// Espera a que una ejecución deje de estar en curso y devuelve su estado final.
    async fn esperar_el_final(pool: &SqlitePool, id: &str) -> String {
        let limite = std::time::Instant::now() + std::time::Duration::from_secs(90);

        loop {
            let estado: String =
                sqlx::query_scalar("SELECT status FROM job_executions WHERE id = ?1")
                    .bind(id)
                    .fetch_one(pool)
                    .await
                    .expect("la fila de la ejecución tiene que estar en la base");

            if !["pending", "running"].contains(&estado.as_str()) {
                return estado;
            }

            assert!(
                std::time::Instant::now() < limite,
                "la ejecución no ha terminado en el plazo de la prueba: su estado sigue siendo \
                 `{estado}`"
            );

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Una tarea que ya tiene una ejecución en curso contesta `409` y **no crea una segunda**.
    ///
    /// Cubre el escenario «Una tarea ya en marcha contesta 409». La ejecución en curso se
    /// **siembra** en la tabla en vez de provocarla con una copia lenta: lo que esta prueba afirma
    /// es la traducción del desenlace de la guarda atómica al código, y montar una copia de verdad
    /// para bloquear la tarea mediría el motor, que ya tiene sus pruebas. La cuenta final es la
    /// otra mitad: un `409` que además encolara dejaría dos filas y el panel con dos barras para un
    /// solo trabajo.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_disparo_sobre_una_tarea_que_ya_corre_contesta_409() {
        let aparato = AparatoDelDisparo::nuevo().await;

        aparato
            .estado
            .db
            .ejecuciones
            .alta(AltaDeEjecucion {
                job_id: aparato.tarea,
                status: Estado::Running,
                snapshot_id: None,
                bytes_processed: 0,
                bytes_uploaded: 0,
                error_message: None,
                started_at: time::OffsetDateTime::now_utc(),
                finished_at: None,
            })
            .await
            .expect("la ejecución en curso de la siembra se escribe");

        let respuesta = post(
            aparato.router(),
            &format!("/api/v1/jobs/{}/run", aparato.tarea),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "una tarea con una ejecución en curso es un conflicto con el estado actual del \
             recurso, y contestó {status} con {cuerpo}"
        );
        assert_eq!(
            cuerpo["status"], 409,
            "la envoltura tiene que repetir el código: {cuerpo}"
        );
        assert_eq!(
            aparato.ejecuciones().await,
            1,
            "el 409 creó una segunda ejecución para la misma tarea"
        );
    }

    /// Un disparo sobre una tarea que no existe contesta `404` y no crea ninguna ejecución.
    ///
    /// Cubre el escenario «Una tarea que no existe contesta 404». El identificador es un UUID bien
    /// formado que no corresponde a ninguna fila, así que el `404` lo decide la ausencia de la
    /// tarea y no el rechazo del identificador, que ya tiene su prueba.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_disparo_sobre_una_tarea_que_no_existe_contesta_404() {
        let aparato = AparatoDelDisparo::nuevo().await;
        let inexistente = Uuid::new_v4();

        let respuesta = post(aparato.router(), &format!("/api/v1/jobs/{inexistente}/run")).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "no hay nada que encolar para una tarea que no está: contestó {status} con {cuerpo}"
        );
        assert_eq!(
            aparato.ejecuciones().await,
            0,
            "un disparo sobre una tarea que no existe dejó una fila escrita"
        );
    }

    /// Un identificador de disparo que no es un UUID contesta `400` nombrando el campo.
    ///
    /// Cubre el escenario «Un identificador que no es un UUID contesta 400»: es una petición mal
    /// formada y no una tarea ausente, así que un `404` mandaría a quien pregunta a buscar una
    /// tarea que nunca existió por una errata de teclado.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_identificador_de_disparo_mal_formado_contesta_400() {
        let aparato = AparatoDelDisparo::nuevo().await;

        let respuesta = post(aparato.router(), "/api/v1/jobs/no-es-un-uuid/run").await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un identificador que no es un UUID no puede ser un recurso ausente: contestó {status} \
             con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto");

        assert!(
            mensaje.contains("id"),
            "el rechazo tiene que nombrar el campo que no resuelve, y el mensaje fue {mensaje:?}"
        );
        assert_eq!(
            aparato.ejecuciones().await,
            0,
            "una petición rechazada no puede dejar ninguna ejecución escrita"
        );
    }

    /// Una tarea con `enabled = 0` **sí** se dispara a mano, y contesta `202`.
    ///
    /// Cubre el escenario «Una tarea con `enabled = 0` sí se dispara a mano» del requisito del
    /// motor, que es el quinto caso del alta de ejecución y **no** estaba escrito: las otras cuatro
    /// pruebas de este grupo cubren los tres códigos de rechazo y el `202` de una tarea activa, y
    /// sin ésta quedaba sin fijar el caso que más fácil se implementa al revés —una guarda de
    /// `enabled` delante del disparo—, que además es el que el panel va a usar cuando alguien
    /// quiera lanzar una copia sin esperar al planificador.
    ///
    /// `enabled` decide si el **planificador** dispara la tarea por su `cron_expression`, y el
    /// disparo manual es otra cosa: un botón que se niega sin explicar que el problema está en el
    /// planificador —que en este change ni siquiera existe— es peor que un botón que ejecuta.
    ///
    /// La desactivación se escribe **con SQL directo** y no editando la tarea por la capa de
    /// acceso: lo que se mide es el disparo, no la edición, y montar una segunda tarea desactivada
    /// cambiaría el aparato que ya usan las otras cuatro pruebas. Se afirma además que la ejecución
    /// encolada es de **esa** tarea, que es lo que ata la fila al disparo y no a cualquier otra.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn una_tarea_desactivada_tambien_se_dispara_a_mano() {
        let aparato = AparatoDelDisparo::nuevo().await;

        sqlx::query("UPDATE backup_jobs SET enabled = 0 WHERE id = ?")
            .bind(aparato.tarea.hyphenated().to_string())
            .execute(&aparato.pool)
            .await
            .expect("la tarea del aparato tiene que poder desactivarse");

        let respuesta = post(
            aparato.router(),
            &format!("/api/v1/jobs/{}/run", aparato.tarea),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::ACCEPTED,
            "una tarea desactivada se dispara a mano —`enabled` gobierna al planificador, no al \
             botón— y contestó {status} con {cuerpo}"
        );

        assert_eq!(
            cuerpo["data"]["job_id"],
            aparato.tarea.hyphenated().to_string(),
            "la ejecución que se encola tiene que ser de la tarea que se disparó: {cuerpo}"
        );
    }

    // ── La retención que no conservaría nada ─────────────────────────────────
    //
    // Cubre `api-resources`, requisito «El alta y la edición de una tarea rechazan una retención
    // que no conservaría nada». El porqué no es de estilo: con las tres reglas a cero no hay
    // ninguna que conserve nada, así que la retención no conserva de menos —**borra todo** el
    // historial del repositorio en la copia siguiente, incluida la que se acaba de hacer—. El §3
    // pone un valor por defecto a esas tres columnas pero **no una cota**, así que el valor entra
    // por aquí y entra a mano; el motor tiene además su propio guardarraíl para las filas escritas
    // directamente en la base, y las dos capas existen porque protegen de dos caminos distintos.

    /// Con las tres reglas a cero, el alta contesta `400` nombrando las tres columnas.
    #[tokio::test]
    async fn una_tarea_con_las_tres_reglas_a_cero_se_rechaza_al_darla_de_alta() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-de-la-retencion").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-de-la-retencion").await;

        let mut cuerpo_de_la_peticion = cuerpo_de_tarea(&target, &repositorio);
        cuerpo_de_la_peticion["keep_daily"] = serde_json::json!(0);
        cuerpo_de_la_peticion["keep_weekly"] = serde_json::json!(0);
        cuerpo_de_la_peticion["keep_monthly"] = serde_json::json!(0);

        let respuesta = post_json(router, "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "una retención con las tres reglas a cero borra todo el historial en la copia \
             siguiente, así que no se acepta: contestó {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto");

        for columna in ["keep_daily", "keep_weekly", "keep_monthly"] {
            assert!(
                mensaje.contains(columna),
                "el rechazo tiene que nombrar las tres columnas —{columna} no está—, y el mensaje \
                 fue {mensaje:?}"
            );
        }

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect("la tabla de tareas tiene que poder contarse");

        assert_eq!(
            filas, 0,
            "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
        );
    }

    /// Con las tres reglas a cero, la edición contesta `400` y la fila queda **como estaba**.
    ///
    /// Es la mitad de la edición del mismo requisito, y la que impide un rechazo a medias: si el
    /// `400` llegara **después** de escribir, la tarea quedaría con una retención que borra todo el
    /// historial y con una respuesta de error —el peor de los dos mundos—.
    #[tokio::test]
    async fn una_tarea_con_las_tres_reglas_a_cero_se_rechaza_al_editarla() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-de-la-retencion-editada").await;
        let repositorio =
            repositorio_de_prueba(&router, "repositorio-de-la-retencion-editada").await;

        let respuesta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea(&target, &repositorio),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "la tarea tiene que darse de alta antes de poder editarla"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let mut edicion = cuerpo_de_tarea(&target, &repositorio);
        edicion["keep_daily"] = serde_json::json!(0);
        edicion["keep_weekly"] = serde_json::json!(0);
        edicion["keep_monthly"] = serde_json::json!(0);

        let respuesta = put_json(router, &format!("/api/v1/jobs/{id}"), &edicion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "la edición que deja las tres reglas a cero tiene que rechazarse: contestó {status} \
             con {cuerpo}"
        );

        let guardadas: (i64, i64, i64) = sqlx::query_as(
            "SELECT keep_daily, keep_weekly, keep_monthly FROM backup_jobs WHERE id = ?1",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .expect("la fila de la tarea tiene que seguir en la base");

        assert_eq!(
            guardadas,
            (7, 4, 6),
            "el rechazo de la edición dejó la fila cambiada: {guardadas:?}"
        );
    }

    /// Un cero suelto acompañado de otra regla informada **se acepta**.
    ///
    /// Es la otra mitad del requisito, y la que impide «arreglarlo» prohibiendo los ceros: un cero
    /// es una decisión del operador —«de esta regla no conserves ninguna»— y lo que no es una
    /// decisión es que las **tres** sean cero. Se afirma además que los valores llegan a la fila,
    /// porque un `201` que guardara otra cosa dejaría la prueba sin contenido.
    #[tokio::test]
    async fn un_cero_suelto_se_acepta() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-del-cero-suelto").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-cero-suelto").await;

        let mut cuerpo_de_la_peticion = cuerpo_de_tarea(&target, &repositorio);
        cuerpo_de_la_peticion["keep_daily"] = serde_json::json!(0);

        let respuesta = post_json(router, "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "un cero acompañado de otra regla informada es una decisión, no un error: contestó \
             {status} con {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let guardadas: (i64, i64, i64) = sqlx::query_as(
            "SELECT keep_daily, keep_weekly, keep_monthly FROM backup_jobs WHERE id = ?1",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .expect("la fila de la tarea aceptada tiene que estar en la base");

        assert_eq!(
            guardadas,
            (0, 4, 6),
            "la fila no guardó la retención que informó la petición: {guardadas:?}"
        );
    }
}
