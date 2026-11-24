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
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::extract::Json;
use crate::api::response::{ApiResponse, Data, PaginatedResponse};
use crate::api::texto;
use crate::api::{TAREA_QUE_NO_ESTA, contenido, del_motor};
use crate::db::entities::{AltaDeTarea, Ejecucion, Tarea};
use crate::error::AppError;
use crate::state::AppState;

/// Parámetros de consulta para listados paginados.
///
/// `limit` por defecto `50`, tope `200`; `offset` por defecto `0`. Valida que `limit` esté en
/// `1..=200` y que `offset` no sea negativo; devuelve `400` en caso contrario.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsultaDelCable {
    /// Cuántas filas devolver como máximo.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Cuántas filas saltar.
    #[serde(default)]
    pub offset: Option<i64>,
}

/// Parámetros de consulta para listados paginados.
///
/// `limit` por defecto `50`, tope `200`; `offset` por defecto `0`. Valida que `limit` esté en
/// `1..=200` y que `offset` no sea negativo; devuelve `400` en caso contrario.
impl ConsultaDelCable {
    /// Valida la paginación y la resuelve a sus dos números, o contesta `400` nombrando el campo.
    pub fn paginacion(&self) -> Result<(i64, i64), AppError> {
        let limite = match self.limit {
            Some(valor) if !(1..=LIMITE_MAXIMO).contains(&valor) => {
                return Err(AppError::InvalidInput(format!(
                    "el campo `limit` de la consulta tiene que ser un número entre 1 y {LIMITE_MAXIMO}"
                )));
            }
            Some(valor) => valor,
            None => LIMITE_POR_DEFECTO,
        };

        let desplazamiento = match self.offset {
            Some(valor) if valor < 0 => {
                return Err(AppError::InvalidInput(
                    "el campo `offset` de la consulta no puede ser negativo".to_owned(),
                ));
            }
            Some(valor) => valor,
            None => 0,
        };

        Ok((limite, desplazamiento))
    }
}

/// Límite de paginación por defecto.
const LIMITE_POR_DEFECTO: i64 = 50;

/// Límite máximo de paginación.
const LIMITE_MAXIMO: i64 = 200;

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
/// `keep_weekly`, `keep_monthly`, `enabled` y `name` —este último, el que añadió la `0005` (issue
/// #65)—, que son los que el panel pinta en su formulario y los que hacen legible la tabla con
/// `sqlite3`. `deny_unknown_fields` es lo que convierte una errata
/// del formulario —un `keep_dailly` de más— en un `400`: sin él, serde ignoraría el campo que no
/// conoce y la retención se quedaría en su valor por defecto **sin que la respuesta dijera una
/// palabra**, que es el peor de los desenlaces posibles: una tarea que guarda otra cosa que la que
/// el operador cree haber programado.
///
/// `name` es `Option<String>` **en el cable** y obligatorio en el negocio (#87 y #89): un cuerpo que
/// no traiga la clave entra aquí con `None` en vez de ser rechazado por el extractor, que es lo que
/// permite contestar un `400` con la envoltura que **nombra `name`** —el extractor rechaza por su
/// cuenta un cuerpo que no case con el tipo, con un mensaje ajeno al §5 y sin nombrar el campo—. El
/// `None` y el blanco los traduce [`nombre_del_cuerpo`] a ese `400`. Lo que llega **sin recortar**:
/// el recorte lo aplica el mismo sitio, y el tope de 120 caracteres se rechaza ahí.
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
    /// Nombre legible de la tarea: `Option` en el cable, obligatorio ya recortado en la fila.
    pub name: Option<String>,
}

/// Una tarea tal y como viaja por el cable: lo que devuelve la API.
///
/// Las claves son **exactamente** las del §3 más la `name` que añadió la `0005` (issue #65)
/// —los dos identificadores, el cron, la retención, el `enabled`, la fecha del alta y el nombre—,
/// para que el formulario, el cuerpo de la petición y la columna de la tabla se llamen igual; una
/// vista que inventara un campo rompería esa correspondencia sin que nada lo notara. Y no hay
/// ninguna credencial que callar aquí: la tarea no guarda ninguna, porque las dos que hay en el
/// sistema viven en su target y en su repositorio, que son las filas a las que ésta apunta.
///
/// `name` viaja como **cadena no vacía**, nunca `null`: desde la `0007` la columna es `NOT NULL` con
/// `UNIQUE(name)`, y las tareas anteriores a la migración llevan el nombre derivado que ella
/// escribió, así que no hay ninguna fila de la que no se pueda decir el nombre. La clave va
/// **siempre** —una vista que la omitiera dejaría al panel sin poder distinguir «esta tarea no tiene
/// nombre» de «el servidor no lo dice»— y ahora también el valor.
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
    /// Nombre legible de la tarea: obligatorio y único, nunca vacío.
    pub name: String,
}

impl TareaDelCable {
    /// Vuelca una fila de la capa de acceso en su forma del cable.
    ///
    /// Es privada porque los cinco manejadores viven en este módulo: no hay ningún llamador
    /// fuera, así que publicarla solo ensancharía la superficie sin que nada la necesite.
    ///
    /// El nombre sale **tal y como está en la fila** y sin recortar: el recorte lo aplicó el alta o
    /// la edición al guardarlo, y volver a recortarlo aquí haría que la vista y la fila pudieran
    /// decir cosas distintas.
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
            name: fila.name.clone(),
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

/// El `400` de una retención que el motor no sabría leer o que no conservaría nada.
///
/// Dos reglas, y las dos por el mismo motivo: que el valor que entra por el formulario sea uno que
/// el motor sepa leer.
///
/// # Las tres a cero
///
/// Con las **tres** reglas a cero no hay ninguna que conserve: la retención no conserva de menos,
/// **borra todo** el historial del repositorio en la copia siguiente, incluida la que se acaba de
/// hacer. El §3 pone un valor por defecto a las tres columnas pero **no una cota**, así que el valor
/// entra por esta puerta y entra a mano, y rechazarlo aquí es lo que evita que el error se cometa
/// sin querer. Un cero suelto acompañado de otra regla informada **sí** se acepta: es una decisión
/// del operador —«de esta regla no conserves ninguna»— y lo que no es una decisión es que las tres
/// sean cero.
///
/// # Un negativo que no sea `-1`, o algo que no cabe en el entero
///
/// El motor —`rustic::retencion::reglas`, descrito en su módulo— lee las tres columnas como un
/// entero de **32 bits** y le da un único significado especial al `-1`: «conservar todo». Cualquier
/// otro negativo, y cualquier valor que no quepa en `i32`, es un valor que el motor **rechaza** al
/// ir a copiar. Rechazarlo aquí es lo que lo para en el formulario, donde se puede corregir en la
/// misma petición, en vez de dejar que aparezca como un fallo de la primera copia; el `-1` se
/// conserva porque hoy es la forma legítima de pedir «conservar todo» y rechazarlo rompería
/// configuraciones válidas.
///
/// # Por qué esta comprobación no es la del motor
///
/// El motor tiene la suya —`rustic::retencion::reglas`, que devuelve
/// `rustic::Error::RetencionQueLoBorraTodo` para los tres ceros y `retencion::Error::RetencionFallida`
/// para el negativo imposible— y **ninguna de las dos sobra**: protegen de dos caminos distintos.
/// Ésta protege el formulario, que es por donde entra una fila de verdad y donde el operador puede
/// corregir en la misma petición; la del motor protege la copia de una fila escrita **directamente**
/// en la base, que aquí no pasa y que el motor no puede rechazar al leerla sin dejar la copia sin
/// hacer. La de aquí es la que puede contestar un `400` que nombre las columnas; la de allí es la
/// que impide que la retención de una fila ya escrita borre el historial.
fn retencion_que_conserva(cuerpo: &AltaDeTareaDelCable) -> Result<(), AppError> {
    if cuerpo.keep_daily == 0 && cuerpo.keep_weekly == 0 && cuerpo.keep_monthly == 0 {
        return Err(AppError::InvalidInput(
            "las tres reglas de retención (`keep_daily`, `keep_weekly` y `keep_monthly`) no pueden \
             ser cero a la vez: sin ninguna regla que conserve, la copia siguiente borraría todo el \
             historial del repositorio"
                .to_owned(),
        ));
    }

    // El mismo juego de valores que lee el motor: de cero en adelante, o el `-1` de «conservar
    // todo». El tope es el del entero de 32 bits que la biblioteca usa.
    for (columna, valor) in [
        ("keep_daily", cuerpo.keep_daily),
        ("keep_weekly", cuerpo.keep_weekly),
        ("keep_monthly", cuerpo.keep_monthly),
    ] {
        if valor < -1 || valor > i64::from(i32::MAX) {
            return Err(AppError::InvalidInput(format!(
                "el campo `{columna}` admite un valor de cero en adelante hasta {}, o el `-1`, que \
                 significa conservar todo",
                i32::MAX
            )));
        }
    }

    Ok(())
}

/// El `400` de una expresión cron que el reloj no sabría leer.
///
/// La gramática la valida el módulo del planificador —con el **mismo** analizador que después la
/// evaluará—, y no este recurso: si el borde validara por su cuenta, habría dos dueños de la
/// gramática y el día que uno cambiara la validación aceptaría lo que el reloj no sabe leer. Aquí
/// solo se traduce su rechazo a un `400` que **nombra el campo** del formulario que hay que
/// corregir, junto al motivo del rechazo.
///
/// Se comprueba **antes de escribir**, y la usan las dos rutas que escriben una tarea, el alta y la
/// edición: una petición rechazada no puede dejar fila.
fn cron_que_el_reloj_sepa_leer(cuerpo: &AltaDeTareaDelCable) -> Result<(), AppError> {
    crate::scheduler::validar(&cuerpo.cron_expression).map_err(|error| {
        AppError::InvalidInput(format!(
            "el campo `cron_expression` no es una expresión cron válida: {error}"
        ))
    })?;

    Ok(())
}

/// El nombre de la tarea, recortado y medido, o el `400` que lo nombra.
///
/// El nombre es **obligatorio**: un cuerpo que no traiga la clave, que la traiga a `null` o que la
/// traiga en blanco —vacía o de solo espacios— es un `400` que **nombra `name`**, porque lo que hay
/// que hacer con él es escribirlo y el panel tiene que saber en qué campo. El `400` nace **aquí** y
/// no en el extractor de JSON: `name` sigue siendo `Option<String>` en el struct del cuerpo, así que
/// `None` y el blanco llegan hasta este punto —el extractor solo rechazaría un cuerpo que no case
/// con el tipo, con un error ajeno a la envoltura del §5 y sin nombrar `name`—, y de aquí sale un
/// rechazo **distinguible** del `409` del duplicado, que nace al violar la unicidad de la base.
///
/// El recorte, la obligatoriedad y la cota de [`texto::COTA_NOMBRE`] caracteres los aplica
/// [`texto::obligatorio`], que es la **misma** regla que la de los demás campos de texto del borde:
/// era una copia local y pasó a ser una llamada para que el tope y el recorte no pudieran separarse
/// de los de `/targets` y `/repositories`. La comprobación va **antes de escribir** y la comparten
/// el alta y la edición —las dos pasan por [`alta_desde`]—, así que una petición rechazada no puede
/// dejar fila y no hay una tercera puerta de escritura que se olvide de ella.
fn nombre_del_cuerpo(cuerpo: &AltaDeTareaDelCable) -> Result<String, AppError> {
    texto::obligatorio("name", cuerpo.name.as_deref(), texto::COTA_NOMBRE)
}

/// Valida el cuerpo y lo convierte en el alta de la capa de acceso.
///
/// La retención se comprueba **antes de tocar la base**: un cuerpo que no puede ser válido por sí
/// mismo no necesita leer ninguna fila para rechazarse, y así el `400` de las tres reglas a cero no
/// depende de que las referencias existan. El nombre se normaliza a su lado y por el mismo motivo.
///
/// Las dos referencias se resuelven después, y en este orden: el target primero y el repositorio
/// después. No es un detalle de estilo —cuando las dos fallan, el `400` nombra **el target**, y eso
/// lo fija una prueba— además de ser el orden del formulario y el del §3.
///
/// La usan **las dos** rutas que escriben una tarea, el alta y la edición: el requisito de la
/// retención es del par, y con la comprobación aquí no hay forma de añadir una tercera puerta de
/// escritura que se olvide de ella. La expresión cron y el nombre se comprueban a su lado, también
/// antes de tocar la base: la gramática del cron la dicta el planificador —el mismo analizador que la
/// evaluará— y el nombre ya sale de aquí recortado, para que la fila guarde lo que se lee y no lo
/// que se envió.
async fn alta_desde(
    estado: &AppState,
    cuerpo: &AltaDeTareaDelCable,
) -> Result<AltaDeTarea, AppError> {
    retencion_que_conserva(cuerpo)?;
    cron_que_el_reloj_sepa_leer(cuerpo)?;

    let name = nombre_del_cuerpo(cuerpo)?;

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
        name,
    })
}

/// `GET /api/v1/jobs` — el listado.
///
/// Una tabla sin filas es un listado de cero elementos, **no** un recurso ausente: contesta `200`
/// con un array vacío, no `404` y no un `data` nulo. Es el estado normal de un Kesto recién
/// instalado, y el panel no tiene por qué tratarlo como un error.
pub async fn listado(
    State(state): State<AppState>,
    Query(consulta): Query<ConsultaDelCable>,
) -> Result<ApiResponse, AppError> {
    let (limite, desplazamiento) = consulta.paginacion()?;

    let filas = state.db.tareas.listar().await?;

    let vistas: Vec<TareaDelCable> = filas
        .iter()
        .map(TareaDelCable::desde)
        .collect::<Result<_, _>>()?;

    let total = vistas.len() as i64;
    let pagina: Vec<TareaDelCable> = vistas
        .into_iter()
        .skip(desplazamiento as usize)
        .take(limite as usize)
        .collect();

    Ok(ApiResponse::new(
        StatusCode::OK,
        "tareas listadas",
        Data::Paginated(PaginatedResponse::new(
            pagina
                .into_iter()
                .map(|v| {
                    serde_json::to_value(v)
                        .map_err(|e| AppError::Other(format!("error serializando: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?,
            total,
            limite,
            desplazamiento,
        )),
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
        name: alta.name,
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
    use std::sync::atomic::{AtomicU64, Ordering};

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
        directorio_de_prueba, estado_de_prueba, estado_desarrollo, get, nombre_de_tarea_de_prueba,
        post, post_json, put_json,
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

    /// Un nombre de tarea **distinto en cada llamada**, para sembrar por la API.
    ///
    /// Desde la `0007` el nombre es `UNIQUE`, así que un cuerpo de siembra con el nombre fijo haría
    /// chocar la segunda tarea contra un `409` que ninguna de estas pruebas ha pedido. El contador
    /// se lleva con un `AtomicU64` y no con una variable por prueba porque las pruebas corren en
    /// paralelo y lo comparten: un contador por prueba repetiría el `0` en todas y volvería el
    /// problema por la puerta de atrás.
    fn nombre_de_siembra() -> String {
        static CONTADOR: AtomicU64 = AtomicU64::new(0);

        let numero = CONTADOR.fetch_add(1, Ordering::Relaxed);

        format!("tarea sembrada {numero}")
    }

    /// El cuerpo de un alta de tarea, con las dos referencias que recibe.
    ///
    /// La tarea lleva un nombre de siembra —distinto en cada llamada, ver [`nombre_de_siembra`]—,
    /// porque desde la `0007` el nombre es obligatorio y único y un cuerpo sin él sería un `400`:
    /// lo que parametriza este cuerpo son sus dos referencias —el target del que se extrae y el
    /// repositorio al que se sube—, y el nombre es lo que hay que **no** tener que pensar en cada
    /// prueba. Los tres campos de retención llevan valores distintos a propósito, para que una vista
    /// que cruzara dos columnas no pasara inadvertida.
    fn cuerpo_de_tarea(target_id: &str, repository_id: &str) -> serde_json::Value {
        let mut cuerpo = serde_json::json!({
            "target_id": target_id,
            "repository_id": repository_id,
            "cron_expression": "0 3 * * *",
            "keep_daily": 7,
            "keep_weekly": 4,
            "keep_monthly": 6,
            "enabled": true,
        });

        cuerpo["name"] = serde_json::json!(nombre_de_siembra());

        cuerpo
    }

    /// El mismo cuerpo del alta **sin** el campo `name`: la falta, no el blanco.
    ///
    /// Es el caso del formulario que no llegó a informar el campo —la clave no viaja—, que es
    /// distinto del que la manda vacía y que tiene su propia prueba. Se construye quitando la clave
    /// en vez de armando un cuerpo aparte para que lo único que cambie respecto de
    /// [`cuerpo_de_tarea`] sea eso.
    fn cuerpo_de_tarea_sin_nombre(target_id: &str, repository_id: &str) -> serde_json::Value {
        let mut cuerpo = cuerpo_de_tarea(target_id, repository_id);
        cuerpo
            .as_object_mut()
            .expect("el cuerpo de la siembra es un objeto")
            .remove("name");

        cuerpo
    }

    /// El mismo cuerpo del alta con el campo `name`, **tal cual** llegó por el cable.
    ///
    /// El valor no se recorta aquí a propósito: lo que este change pide es que el recorte, el rechazo
    /// del blanco y el tope de 120 caracteres los aplique **el servidor**, así que quien manda el
    /// cuerpo tiene que poder mandar los espacios alrededor, una cadena en blanco y una larga sin que
    /// la prueba los arregle antes.
    fn cuerpo_de_tarea_con_nombre(
        target_id: &str,
        repository_id: &str,
        nombre: &str,
    ) -> serde_json::Value {
        let mut cuerpo = cuerpo_de_tarea(target_id, repository_id);
        cuerpo["name"] = serde_json::json!(nombre);

        cuerpo
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
    /// las columnas del §3 —más la `name` que añade la `0005` (issue #65)—. Los nombres son los del
    /// esquema para que el formulario, el cuerpo y la columna se llamen igual, y una vista que
    /// inventara un campo rompería esa correspondencia sin que nada lo notara: por eso la prueba
    /// afirma el **conjunto exacto** de claves y no solo la presencia de las que espera. Las dos
    /// cosas que **no** pueden estar: ninguna credencial —la tarea no guarda ninguna, porque las dos
    /// del sistema viven en su target y en su repositorio— y ningún nombre inventado que el panel no
    /// sepa pintar.
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
            // El `name` de la `0005` (issue #65), obligatorio y único desde la `0007` (#87 y #89): la
            // **clave** tiene que estar siempre —una vista que la omitiera dejaría al panel sin
            // poder distinguir «esta tarea no tiene nombre» de «el servidor no lo dice»— y su valor
            // es una cadena, nunca `null`.
            "name",
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
                    db_path: None,
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
                    name: nombre_de_tarea_de_prueba(),
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

    //
    // Cubre `api-resources`, requisito «El alta y la edición de una tarea validan su expresión
    // cron». El porqué del reparto no es de arquitectura: la validación es **la misma** que evalúa
    // el planificador —un analizador, una gramática, un solo sitio—, porque si el borde HTTP
    // tuviera su propia idea de qué expresión vale, lo que la API aceptara y lo que el reloj
    // entendiera podrían discrepar, y discreparían justo en los bordes (los segundos, los días de
    // la semana), que es donde aparecen las expresiones raras. Y valida **antes de escribir**,
    // porque una fila inválida que se escribe y luego se rechaza es peor que un rechazo: queda en
    // la base, el panel la lista como programada y la reconciliación del planificador la salta en
    // cada pasada. Las dos rutas que escriben —el alta y la edición— comparten el camino, así que
    // las dos quedan cubiertas por el mismo sitio.

    /// Una expresión que el analizador no entiende, para las tres pruebas del rechazo.
    ///
    /// No es una errata de seis campos —eso sería otro caso—: es texto que no es una expresión cron
    /// en absoluto, así que su rechazo no depende de la forma del calendario que se acepte.
    const EXPRESION_QUE_NO_PARSEA: &str = "cada lunes a las tres";

    /// 1.6 — El alta con una expresión de cinco campos se escribe.
    ///
    /// Cubre `api-resources`, escenario «El alta con una expresión de cinco campos se escribe». Es
    /// el guardarraíl de la validación nueva: la forma que la base ya guarda —`0 3 * * *`— tiene que
    /// seguir entrando, y entra **de verdad**, con la fila escrita y no solo con el `201`. Una
    /// validación que rechazara lo que ya era válido rompería las tareas existentes al editarlas, y
    /// es exactamente lo que esta prueba impediría pasar inadvertido.
    #[tokio::test]
    async fn el_alta_con_una_expresion_de_cinco_campos_se_escribe() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-del-cron-valido").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-cron-valido").await;

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
            "una expresión de cinco campos es válida y tiene que darse de alta: contestó {status} \
             con {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let guardada: String =
            sqlx::query_scalar("SELECT cron_expression FROM backup_jobs WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila de la tarea aceptada tiene que estar en la base");

        assert_eq!(
            guardada, "0 3 * * *",
            "la fila no guardó la expresión que informó la petición: {guardada:?}"
        );
    }

    /// 1.6 — El alta con una expresión inválida contesta `400` y no deja fila.
    ///
    /// Cubre `api-resources`, escenario «El alta con una expresión inválida contesta 400 y no deja
    /// fila». El rechazo llega **antes de escribir**: lo que se afirma no es solo el `400`, es que
    /// la tabla no tiene ninguna fila nueva —una petición rechazada que escribiera primero dejaría
    /// una tarea que nadie programó—. El motivo tiene que decir por qué la expresión no vale: un
    /// `400` mudo no le sirve de nada a quien tiene que corregir la casilla.
    #[tokio::test]
    async fn el_alta_con_una_expresion_invalida_contesta_400_y_no_deja_fila() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-del-cron-invalido").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-cron-invalido").await;

        let mut cuerpo_de_la_peticion = cuerpo_de_tarea(&target, &repositorio);
        cuerpo_de_la_peticion["cron_expression"] = serde_json::json!(EXPRESION_QUE_NO_PARSEA);

        let respuesta = post_json(router, "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "una expresión que el analizador no entiende es un formulario mal rellenado, no un \
             servidor roto: contestó {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            !mensaje.trim().is_empty(),
            "el motivo del rechazo tiene que decir por qué la expresión no vale, y llegó vacío"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect("la tabla de tareas tiene que poder contarse");

        assert_eq!(
            filas, 0,
            "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
        );
    }

    /// 1.6 — La edición con una expresión inválida deja la fila como estaba.
    ///
    /// Cubre `api-resources`, escenario «La edición con una expresión inválida deja la fila como
    /// estaba». La edición es cómo se arregla una expresión mala y también cómo se podría empeorar
    /// sin límite, así que la validación es del par de rutas y no solo del alta. Lo que se comprueba
    /// es la fila **en la base** y no la respuesta: un `400` que hubiera escrito antes de rechazar
    /// contestaría lo mismo y dejaría la tarea con la expresión que el analizador no entiende.
    #[tokio::test]
    async fn la_edicion_con_una_expresion_invalida_deja_la_fila_como_estaba() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-del-cron-editado").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-cron-editado").await;

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
        edicion["cron_expression"] = serde_json::json!(EXPRESION_QUE_NO_PARSEA);

        let respuesta = put_json(router, &format!("/api/v1/jobs/{id}"), &edicion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "la edición que deja una expresión que el analizador no entiende tiene que rechazarse: \
             contestó {status} con {cuerpo}"
        );

        let guardada: String =
            sqlx::query_scalar("SELECT cron_expression FROM backup_jobs WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila de la tarea tiene que seguir en la base");

        assert_eq!(
            guardada, "0 3 * * *",
            "el rechazo de la edición dejó la fila con la expresión nueva: {guardada:?}"
        );
    }

    /// 1.6 — El motivo del `400` nombra el campo `cron_expression`.
    ///
    /// Cubre `api-resources`, escenario «El motivo nombra el campo». Es la regla de la casa para
    /// todos los `400` de la superficie, y aquí es lo que hace falta: la regla de no repetir el
    /// valor existe por las credenciales, y quien envió la expresión la tiene delante, así que lo
    /// que le sirve es saber qué casilla del formulario corregir. Sin el nombre del campo, el motivo
    /// del analizador podría hablar de «la expresión» sin decir a quién se refiere en un cuerpo de
    /// siete campos.
    #[tokio::test]
    async fn el_motivo_del_rechazo_nombra_el_campo_cron_expression() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-del-nombre-del-campo").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-nombre-del-campo").await;

        let mut cuerpo_de_la_peticion = cuerpo_de_tarea(&target, &repositorio);
        cuerpo_de_la_peticion["cron_expression"] = serde_json::json!(EXPRESION_QUE_NO_PARSEA);

        let respuesta = post_json(router, "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "una expresión que no parsea tiene que rechazarse antes de comprobar el nombre del \
             campo: contestó {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("cron_expression"),
            "el motivo tiene que nombrar el campo del formulario que hay que corregir, y el \
             mensaje fue {mensaje:?}"
        );
    }

    // ── El nombre de la tarea (#87 y #89, antes #65) ─────────────────────────
    //
    // Cubre `api-resources`, requisito «El nombre de la tarea es obligatorio y único»: el alta y la
    // edición **exigen** un `name` que se recorta, que contesta `400` nombrando el campo si falta o
    // si queda en blanco tras recortar y `409` si ya lo tiene otra tarea. El nombre es la identidad
    // legible de la tarea y lo garantiza la base —`NOT NULL` y `UNIQUE(name)`—, así que el borde no
    // comprueba la unicidad: la deja reventar contra la restricción y traduce el choque.

    /// 1.2 — El alta guarda y publica el nombre **recortado**, y la lectura lo devuelve igual.
    ///
    /// Cubre el escenario «El alta guarda el nombre recortado». Se manda `copia nocturna` con
    /// espacios a los dos lados y se comprueba en las **tres** superficies que publican el nombre
    /// —la respuesta del alta, la lectura de la fila y el listado—, porque el recorte tiene que estar
    /// en la fila guardada y no solo en la vista de la respuesta que lo escribió: un `trim` aplicado
    /// al serializar dejaría la fila con los espacios y el nombre cambiaría de una lectura a otra.
    #[tokio::test]
    async fn el_alta_guarda_y_publica_el_nombre_recortado() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-del-nombre-recortado").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-nombre-recortado").await;

        let respuesta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "  copia nocturna  "),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "un nombre con espacios alrededor tiene que aceptarse y recortarse, no rechazarse: \
             contestó {status} con {cuerpo}"
        );

        assert_eq!(
            cuerpo["data"]["name"], "copia nocturna",
            "el alta tiene que publicar el nombre **recortado**: {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador de la tarea")
            .to_owned();

        let lectura = body_json(get(router.clone(), &format!("/api/v1/jobs/{id}")).await).await;

        assert_eq!(
            lectura["data"]["name"], "copia nocturna",
            "la fila guardada tiene que traer el nombre recortado, no el que llegó: {lectura}"
        );

        let listado = body_json(get(router, "/api/v1/jobs").await).await;

        assert_eq!(
            listado["data"]["items"][0]["name"], "copia nocturna",
            "el listado tiene que publicar el mismo nombre que la lectura: {listado}"
        );
    }

    /// 1.1 — Un alta **sin** `name` contesta `400` con la envoltura y un mensaje que nombra `name`.
    ///
    /// Cubre el escenario «Un alta sin name contesta 400 nombrando name». La clave **no viaja**, que
    /// es el caso del formulario que no llegó a informar el campo, y el rechazo tiene que nombrarlo
    /// para que se pueda corregir sin adivinar cuál de los ocho campos falta. La tabla se cuenta al
    /// final: un rechazo que escribiera primero sería peor que no rechazar.
    #[tokio::test]
    async fn un_alta_sin_name_contesta_400_nombrando_name() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-del-alta-sin-nombre").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-alta-sin-nombre").await;

        let respuesta = post_json(
            router,
            "/api/v1/jobs",
            &cuerpo_de_tarea_sin_nombre(&target, &repositorio),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un alta sin `name` tiene que rechazarse con la envoltura: contestó {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("name"),
            "el rechazo tiene que nombrar el campo `name` para que se pueda corregir desde el \
             formulario, y el mensaje fue {mensaje:?}"
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

    /// 1.2 — Un `name` en blanco contesta `400` nombrando `name`, y **no** se guarda.
    ///
    /// Cubre el escenario «Un nombre en blanco contesta 400 nombrando name», que sustituye al del
    /// contrato derogado —«un nombre en blanco se guarda como `null`»—: el blanco ya no es una tarea
    /// sin nombre sino una petición que hay que reescribir. Se manda una cadena de **solo espacios**
    /// para que el rechazo dependa del recorte y no de comparar con la cadena vacía: una validación
    /// que mirara `is_empty()` a secas dejaría pasar el blanco y esta prueba lo delata.
    #[tokio::test]
    async fn un_nombre_en_blanco_contesta_400_nombrando_name() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-del-nombre-en-blanco").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-nombre-en-blanco").await;

        let respuesta = post_json(
            router,
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "   "),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un nombre de solo espacios tiene que rechazarse, no guardarse vacío: contestó {status} \
             con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("name"),
            "el rechazo tiene que nombrar el campo `name`, y el mensaje fue {mensaje:?}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect(
                "la tabla tiene que ser contable sobre el fichero con el que se abrió el estado",
            );

        assert_eq!(
            filas, 0,
            "un nombre en blanco no puede dejar una fila con el nombre vacío: quedaron {filas}"
        );
    }

    /// 1.3 — Un `name` que ya tiene otra tarea contesta `409`, y el mensaje no repite el valor.
    ///
    /// Cubre el escenario «Un nombre duplicado en el alta contesta 409». El `409` sale de **violar
    /// la restricción de la base** y no de una lectura previa: comprobar y luego escribir deja la
    /// ventana abierta a que dos altas simultáneas pasen las dos la comprobación. Por eso la prueba
    /// no espía el camino —no puede— y se limita a fijar el desenlace del contrato: la segunda alta
    /// no entra y contesta `409` nombrando el campo, sin repetir el valor que traía.
    #[tokio::test]
    async fn un_nombre_duplicado_en_el_alta_contesta_409() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-del-nombre-duplicado").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-nombre-duplicado").await;

        let primera = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna"),
        )
        .await;

        let status = primera.status();
        let primera = body_json(primera).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la primera tarea tiene que darse de alta: contestó {status} con {primera}"
        );

        let respuesta = post_json(
            router,
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna"),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "un nombre que ya está en uso tiene que chocar con el estado actual: contestó {status} \
             con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("name"),
            "el rechazo tiene que nombrar el campo `name`, y el mensaje fue {mensaje:?}"
        );
        assert!(
            !mensaje.contains("copia nocturna"),
            "el rechazo nombra el campo, nunca el valor que traía, y el mensaje fue {mensaje:?}"
        );
    }

    /// 1.3b — Un `name` que ya tiene otra tarea contesta `409` también al **editar**.
    ///
    /// Cubre el escenario «Un nombre duplicado en la edición contesta 409», la mitad del `SHALL` que
    /// el alta no mide: las dos rutas escriben por la misma sentencia, así que la restricción las
    /// cubre a las dos, pero una edición que se saltara la escritura —o que guardara el nombre por
    /// otro camino— no lo delataría. Se afirma además que la segunda tarea **conserva** su nombre
    /// anterior: un rechazo que hubiera escrito antes de chocar sería peor que no rechazar.
    #[tokio::test]
    async fn un_nombre_duplicado_en_la_edicion_contesta_409() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-del-nombre-duplicado-al-editar").await;
        let repositorio =
            repositorio_de_prueba(&router, "repositorio-del-nombre-duplicado-al-editar").await;

        let primera = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna"),
        )
        .await;

        let status = primera.status();
        let primera = body_json(primera).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la primera tarea tiene que darse de alta: contestó {status} con {primera}"
        );

        let segunda = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia diaria"),
        )
        .await;

        let status = segunda.status();
        let segunda = body_json(segunda).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la segunda tarea tiene que darse de alta con su propio nombre: contestó {status} con \
             {segunda}"
        );

        let id = segunda["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador de la tarea")
            .to_owned();

        let respuesta = put_json(
            router.clone(),
            &format!("/api/v1/jobs/{id}"),
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna"),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "editar una tarea con el nombre de otra tiene que chocar: contestó {status} con {cuerpo}"
        );

        let lectura = body_json(get(router, &format!("/api/v1/jobs/{id}")).await).await;

        assert_eq!(
            lectura["data"]["name"], "copia diaria",
            "una edición rechazada no puede haber escrito el nombre nuevo: {lectura}"
        );
    }

    /// 1.4 — Editar una tarea **conservando su propio nombre** no choca: contesta `200`.
    ///
    /// Cubre el escenario «Editar una tarea conservando su propio nombre no choca». Es la prueba que
    /// impide que el `UNIQUE` convierta el formulario en intocable: una tarea no choca consigo misma,
    /// así que cambiar el cron —o cualquier otro campo— enviando el nombre que ya tenía tiene que
    /// seguir siendo una edición válida. Sin ella, un `409` que comparara «existe una fila con este
    /// nombre» sin excluir la propia pasaría por bueno y dejaría el nombre imposible de conservar.
    #[tokio::test]
    async fn editar_conservando_el_propio_nombre_no_choca() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-del-nombre-propio").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-nombre-propio").await;

        let alta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna"),
        )
        .await;

        let status = alta.status();
        let alta = body_json(alta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la tarea tiene que darse de alta antes de editarla: contestó {status} con {alta}"
        );

        let id = alta["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador de la tarea")
            .to_owned();

        let mut edicion = cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna");
        edicion["cron_expression"] = serde_json::json!("0 5 * * *");

        let respuesta = put_json(router.clone(), &format!("/api/v1/jobs/{id}"), &edicion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "una tarea no choca consigo misma: conservar el propio nombre tiene que aceptarse, y \
             contestó {status} con {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["name"], "copia nocturna",
            "su nombre tiene que seguir siendo el mismo: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["cron_expression"], "0 5 * * *",
            "y el campo que sí se cambió tiene que haber entrado: {cuerpo}"
        );
    }

    /// 1.5 — Los dos rechazos del nombre —el `400` de la falta y el `409` del duplicado— se
    /// distinguen entre sí.
    ///
    /// Cubre el escenario «Los dos rechazos del nombre se distinguen entre sí». Los dos hablan del
    /// mismo campo, pero mandan a hacer cosas distintas —uno a escribir un nombre, el otro a
    /// cambiarlo— y un panel que no los distinga obliga al operador a adivinar cuál de las dos
    /// arreglar. La aserción compara los dos textos, así que un mensaje copiado de un sitio al otro
    /// la hace fallar aunque los dos nombren `name`.
    #[tokio::test]
    async fn los_dos_rechazos_del_nombre_se_distinguen() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-de-los-dos-rechazos").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-de-los-dos-rechazos").await;

        let falta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_sin_nombre(&target, &repositorio),
        )
        .await;

        let status_falta = falta.status();
        let falta = body_json(falta).await;

        assert_eq!(
            status_falta,
            StatusCode::BAD_REQUEST,
            "el alta sin nombre tiene que contestar 400: contestó {status_falta} con {falta}"
        );

        let alta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna"),
        )
        .await;

        let status = alta.status();
        let alta = body_json(alta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la tarea que ocupa el nombre tiene que darse de alta: contestó {status} con {alta}"
        );

        let duplicado = post_json(
            router,
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna"),
        )
        .await;

        let status_duplicado = duplicado.status();
        let duplicado = body_json(duplicado).await;

        assert_eq!(
            status_duplicado,
            StatusCode::CONFLICT,
            "el nombre ya en uso tiene que contestar 409: contestó {status_duplicado} con {duplicado}"
        );

        let mensaje_falta = falta["message"]
            .as_str()
            .expect("el 400 tiene que traer un `message` de texto");
        let mensaje_duplicado = duplicado["message"]
            .as_str()
            .expect("el 409 tiene que traer un `message` de texto");

        assert_ne!(
            mensaje_falta, mensaje_duplicado,
            "los dos rechazos dicen cosas distintas del mismo campo y no pueden compartir mensaje: \
             {mensaje_falta:?} y {mensaje_duplicado:?}"
        );
    }

    /// 1.2c — La edición acepta `name` y lo recorta.
    ///
    /// Cubre la mitad del `SHALL` que la prueba del alta no mide: `PUT /api/v1/jobs/:id` acepta el
    /// campo con las **mismas** reglas —recorte y tope— y lo publica en su respuesta y en la lectura.
    /// Sin esta prueba, una edición que ignorara el campo dejaría el nombre imposible de **corregir**
    /// desde el panel: solo se podría fijar al darse de alta, y una errata no tendría arreglo.
    #[tokio::test]
    async fn la_edicion_acepta_y_recorta_el_nombre() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-de-la-edicion-del-nombre").await;
        let repositorio =
            repositorio_de_prueba(&router, "repositorio-de-la-edicion-del-nombre").await;

        let alta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea(&target, &repositorio),
        )
        .await;

        let status = alta.status();
        let alta = body_json(alta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la tarea tiene que darse de alta antes de editarla: contestó {status} con {alta}"
        );

        let id = alta["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador de la tarea")
            .to_owned();

        // Se le cambia el nombre, con espacios alrededor: la edición tiene que recortarlo.
        let mut edicion = cuerpo_de_tarea_con_nombre(&target, &repositorio, "  copia nocturna  ");
        edicion["keep_daily"] = serde_json::json!(3);

        let respuesta = put_json(router.clone(), &format!("/api/v1/jobs/{id}"), &edicion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "la edición con un nombre —recortable— tiene que aceptarse: contestó {status} con \
             {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["name"], "copia nocturna",
            "la edición tiene que publicar el nombre recortado: {cuerpo}"
        );

        let lectura = body_json(get(router, &format!("/api/v1/jobs/{id}")).await).await;

        assert_eq!(
            lectura["data"]["name"], "copia nocturna",
            "la fila editada tiene que guardar el nombre recortado, no el que llegó: {lectura}"
        );
        assert_eq!(
            lectura["data"]["keep_daily"],
            serde_json::json!(3),
            "y el resto de campos que traía la edición tiene que haber entrado: {lectura}"
        );
    }

    /// 1.2d — La edición con un nombre en blanco contesta `400` y **no** toca la fila.
    ///
    /// Cubre la mitad «o una edición» del escenario «Un nombre en blanco contesta 400 nombrando
    /// name». El nombre en blanco ya no es un modo de dejar la tarea sin nombre —eso no existe desde
    /// la `0007`— sino una petición que hay que reescribir, y la fila tiene que quedar **intacta**:
    /// el cuerpo trae además un `keep_daily` distinto, así que una edición que escribiera antes de
    /// validar lo delataría por el nombre y por la retención.
    #[tokio::test]
    async fn la_edicion_con_un_nombre_en_blanco_contesta_400_y_no_toca_la_fila() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-del-blanco-en-la-edicion").await;
        let repositorio =
            repositorio_de_prueba(&router, "repositorio-del-blanco-en-la-edicion").await;

        let alta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna"),
        )
        .await;

        let status = alta.status();
        let alta = body_json(alta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la tarea tiene que darse de alta antes de editarla: contestó {status} con {alta}"
        );

        let id = alta["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador de la tarea")
            .to_owned();

        let mut blanco = cuerpo_de_tarea_con_nombre(&target, &repositorio, "   ");
        blanco["keep_daily"] = serde_json::json!(3);

        let respuesta = put_json(router.clone(), &format!("/api/v1/jobs/{id}"), &blanco).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un nombre en blanco en la edición tiene que rechazarse: contestó {status} con {cuerpo}"
        );

        let lectura = body_json(get(router, &format!("/api/v1/jobs/{id}")).await).await;

        assert_eq!(
            lectura["data"]["name"], "copia nocturna",
            "una edición rechazada no puede haber escrito el nombre en blanco: {lectura}"
        );
        assert_eq!(
            lectura["data"]["keep_daily"],
            serde_json::json!(7),
            "ni ninguno de los otros campos que traía el cuerpo: {lectura}"
        );
    }

    /// 1.4 — Un nombre de más de **120** caracteres contesta `400` nombrando `name`.
    ///
    /// Cubre el escenario «Un nombre de más de 120 caracteres contesta 400 nombrando name». Es el
    /// techo del campo, y el rechazo tiene que **nombrar el campo** para que se pueda corregir desde
    /// el formulario. El mensaje **no** repite el valor —la regla de la casa— y por eso la cadena es
    /// distinguible: un `contains` sobre el texto crudo no puede dar un falso positivo por
    /// casualidad. La tabla se cuenta al final para afirmar que el rechazo no escribió nada.
    #[tokio::test]
    async fn un_nombre_de_mas_de_120_caracteres_contesta_400_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-del-nombre-largo").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-nombre-largo").await;

        let largo = "n".repeat(121);

        let respuesta = post_json(
            router,
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, &largo),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un nombre de 121 caracteres supera el tope de 120 y tiene que rechazarse: contestó \
             {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto");

        assert!(
            mensaje.contains("name"),
            "el rechazo tiene que nombrar el campo `name` para que se pueda corregir desde el \
             formulario, y el mensaje fue {mensaje:?}"
        );
        assert!(
            !mensaje.contains(&largo),
            "el rechazo nombra el campo, nunca el valor que traía, y el mensaje fue {mensaje:?}"
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

    /// 1.6 — El listado publica el `name` de **cada** tarea, y nunca lo publica a `null`.
    ///
    /// Cubre el escenario «El listado de tareas publica el nombre de cada tarea»: dos tareas dadas de
    /// alta por la API, una con nombre propio y otra con el de siembra. Se afirma la **clave** y el
    /// **valor** en las dos, y que ninguna llega a `null`: desde la `0007` la columna es `NOT NULL`,
    /// así que un `null` en el listado solo podría venir de una vista que lo anulara por su cuenta, y
    /// eso es exactamente lo que este change retira.
    #[tokio::test]
    async fn el_listado_publica_el_nombre_de_cada_tarea() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-del-listado-con-nombre").await;
        let repositorio =
            repositorio_de_prueba(&router, "repositorio-del-listado-con-nombre").await;

        let con_nombre = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna"),
        )
        .await;

        let status = con_nombre.status();
        let con_nombre = body_json(con_nombre).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la tarea con nombre propio tiene que darse de alta: contestó {status} con {con_nombre}"
        );

        let id_con_nombre = con_nombre["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador de la tarea")
            .to_owned();

        // La segunda lleva el nombre de siembra que arma [`cuerpo_de_tarea`], distinto del de la
        // primera: es con el que se afirma que el valor publicado es el de **cada** fila.
        let sembrada = cuerpo_de_tarea(&target, &repositorio);
        let nombre_sembrado = sembrada["name"]
            .as_str()
            .expect("el cuerpo de siembra trae un nombre de texto")
            .to_owned();

        let otra = post_json(router.clone(), "/api/v1/jobs", &sembrada).await;

        let status = otra.status();
        let otra = body_json(otra).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la segunda tarea tiene que darse de alta: contestó {status} con {otra}"
        );

        let id_de_la_otra = otra["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador de la tarea")
            .to_owned();

        let respuesta = get(router, "/api/v1/jobs").await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado tiene que contestar 200 con un array en `data`: contestó {status} con \
             {cuerpo}"
        );

        let filas = cuerpo["data"]
            .as_object()
            .expect("data debe ser objeto")
            .get("items")
            .expect("data debe tener items")
            .as_array()
            .expect("items debe ser array");

        assert_eq!(filas.len(), 2, "tienen que salir las dos tareas: {cuerpo}");

        let con = filas
            .iter()
            .find(|fila| fila["id"].as_str() == Some(id_con_nombre.as_str()))
            .unwrap_or_else(|| panic!("la tarea con nombre no está en el listado: {cuerpo}"));

        assert_eq!(
            con["name"], "copia nocturna",
            "la fila de la tarea con nombre tiene que publicar su `name`: {cuerpo}"
        );

        let otra = filas
            .iter()
            .find(|fila| fila["id"].as_str() == Some(id_de_la_otra.as_str()))
            .unwrap_or_else(|| panic!("la segunda tarea no está en el listado: {cuerpo}"));

        assert_eq!(
            otra["name"], nombre_sembrado,
            "cada fila tiene que publicar **su** nombre, no el de otra: {cuerpo}"
        );

        // Y ninguna llega a `null`: la columna es `NOT NULL` desde la `0007`, así que un `null` solo
        // podría venir de una vista que lo anulara por su cuenta.
        for fila in filas {
            assert!(
                fila["name"].is_string(),
                "ninguna fila del listado puede publicar `name` a `null`: {cuerpo}"
            );
        }
    }

    /// El listado de jobs responde con paginación (`limit`/`offset` y `items`/`total`).
    ///
    /// El endpoint actual devuelve un array plano en `data`; esta prueba fija el contrato de
    /// paginación y debe fallar hasta que se implemente. No toca la implementación.
    #[tokio::test]
    async fn el_listado_de_jobs_debe_responder_con_paginacion() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-paginacion").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-paginacion").await;

        // Sembramos varias tareas para que `total` supere la página.
        for _ in 0..3 {
            post_json(
                router.clone(),
                "/api/v1/jobs",
                &cuerpo_de_tarea(&target, &repositorio),
            )
            .await;
        }

        let respuesta = get(router, "/api/v1/jobs?limit=1&offset=0").await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado paginado tiene que contestar 200, y contestó {status} con {cuerpo}"
        );

        let items = cuerpo["data"]["items"]
            .as_array()
            .expect("el listado paginado tiene que traer un array en `data.items`: {cuerpo}");
        let total = cuerpo["data"]["total"]
            .as_u64()
            .expect("el listado paginado tiene que traer un entero en `data.total`: {cuerpo}");

        assert_eq!(
            items.len(),
            1,
            "con limit=1 el listado devuelve un solo ítem: {cuerpo}"
        );
        assert_eq!(
            total, 3,
            "total tiene que contar todas las tareas, no solo la página: {cuerpo}"
        );
    }

    /// 1.4c — El tope del nombre es **inclusivo**: 120 caracteres entran.
    ///
    /// Cubre la frontera exacta del escenario «Un nombre de más de 120 caracteres contesta 400
    /// nombrando name»: la prueba de 121 fija dónde **empieza** el rechazo y esta fija dónde **acaba**
    /// lo que sí entra. Un tope escrito con `>=` en vez de `>` rechazaría un nombre de exactamente
    /// 120 —justo el que el `maxLength` del formulario deja escribir— y solo una prueba sobre la
    /// frontera lo ve. Se comprueba la respuesta del alta **y** la lectura de la fila: un nombre que
    /// cupiera en la vista pero se guardara truncado cambiaría de una lectura a otra.
    #[tokio::test]
    async fn el_tope_de_120_caracteres_se_acepta_en_el_alta() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-del-nombre-justo").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-nombre-justo").await;

        let justo = "n".repeat(120);

        assert_eq!(
            justo.chars().count(),
            120,
            "la cadena de la prueba tiene que tener exactamente 120 caracteres"
        );

        let respuesta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, &justo),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "un nombre de exactamente 120 caracteres está en el tope y tiene que aceptarse: contestó \
             {status} con {cuerpo}"
        );

        let devuelto = cuerpo["data"]["name"]
            .as_str()
            .expect("el alta tiene que devolver el nombre como texto");

        assert_eq!(
            devuelto.chars().count(),
            120,
            "el alta tiene que devolver el nombre con sus 120 caracteres, sin truncarlo: {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador de la tarea")
            .to_owned();

        let lectura = body_json(get(router, &format!("/api/v1/jobs/{id}")).await).await;

        assert_eq!(
            lectura["data"]["name"].as_str(),
            Some(justo.as_str()),
            "la fila guardada tiene que conservar el nombre entero, con sus 120 caracteres: {lectura}"
        );
    }

    /// 1.4d — El tope se mide en **caracteres**, no en bytes.
    ///
    /// Cubre la segunda mitad de la frontera: 120 caracteres acentuados ocupan 240 bytes en UTF-8, así
    /// que un tope medido con `len()` —bytes— rechazaría un nombre que el formulario, con su
    /// `maxLength` en caracteres, deja escribir. La prueba afirma las dos cuentas sobre la **misma**
    /// cadena: 120 caracteres, que es lo que limita el contrato, y 240 bytes, que es lo que un tope
    /// equivocado habría contado; sin la segunda, la prueba pasaría igual con una cadena ASCII y no
    /// distinguiría las dos medidas.
    #[tokio::test]
    async fn el_tope_del_nombre_se_mide_en_caracteres_y_no_en_bytes() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-del-nombre-acentuado").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-nombre-acentuado").await;

        let acentuado = "á".repeat(120);

        assert_eq!(
            acentuado.chars().count(),
            120,
            "la cadena de la prueba tiene que tener 120 caracteres"
        );
        assert_eq!(
            acentuado.len(),
            240,
            "y 240 bytes en UTF-8, que es lo que un tope medido con `len()` habría contado"
        );

        let respuesta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, &acentuado),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "un nombre de 120 caracteres acentuados —240 bytes— cabe en el tope en caracteres y \
             tiene que aceptarse: contestó {status} con {cuerpo}"
        );

        let devuelto = cuerpo["data"]["name"]
            .as_str()
            .expect("el alta tiene que devolver el nombre como texto");

        assert_eq!(
            devuelto.chars().count(),
            120,
            "el alta tiene que devolver los 120 caracteres, no 240: {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador de la tarea")
            .to_owned();

        let lectura = body_json(get(router, &format!("/api/v1/jobs/{id}")).await).await;

        assert_eq!(
            lectura["data"]["name"].as_str(),
            Some(acentuado.as_str()),
            "la fila guardada tiene que conservar los 120 caracteres acentuados: {lectura}"
        );
    }

    /// 1.4e — El tope se aplica igual en la **edición**: 121 caracteres por `PUT` contestan `400`.
    ///
    /// Cubre la mitad «o una edición» del escenario «Un nombre de más de 120 caracteres contesta 400
    /// nombrando name», que hoy solo se ejerce por el alta. Que las dos rutas compartan la
    /// normalización es una propiedad de la implementación y no del contrato: una edición que validara
    /// por su cuenta podría dejarse el tope. Se comprueba además que el rechazo **no toca la fila**:
    /// el cuerpo lleva un `keep_daily` distinto y la lectura tiene que traer el nombre y la retención
    /// anteriores, así que una edición que escribiera antes de verificar lo delataría por los dos
    /// lados.
    #[tokio::test]
    async fn un_nombre_de_121_caracteres_contesta_400_en_la_edicion() {
        let router = api_router(estado_desarrollo().await);

        let target = target_de_prueba(&router, "target-del-nombre-largo-en-la-edicion").await;
        let repositorio =
            repositorio_de_prueba(&router, "repositorio-del-nombre-largo-en-la-edicion").await;

        let alta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &cuerpo_de_tarea_con_nombre(&target, &repositorio, "copia nocturna"),
        )
        .await;

        let status = alta.status();
        let alta = body_json(alta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la tarea tiene que darse de alta antes de editarla: contestó {status} con {alta}"
        );

        let id = alta["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador de la tarea")
            .to_owned();

        let largo = "n".repeat(121);

        let mut edicion = cuerpo_de_tarea_con_nombre(&target, &repositorio, &largo);
        edicion["keep_daily"] = serde_json::json!(3);

        let respuesta = put_json(router.clone(), &format!("/api/v1/jobs/{id}"), &edicion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un nombre de 121 caracteres tiene que rechazarse también al editar: contestó {status} \
             con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto");

        assert!(
            mensaje.contains("name"),
            "el rechazo de la edición tiene que nombrar el campo `name`, y el mensaje fue \
             {mensaje:?}"
        );
        assert!(
            !mensaje.contains(&largo),
            "el rechazo nombra el campo, nunca el valor que traía, y el mensaje fue {mensaje:?}"
        );

        let lectura = body_json(get(router, &format!("/api/v1/jobs/{id}")).await).await;

        assert_eq!(
            lectura["data"]["name"], "copia nocturna",
            "una edición rechazada no puede haber escrito el nombre nuevo: {lectura}"
        );
        assert_eq!(
            lectura["data"]["keep_daily"],
            serde_json::json!(7),
            "ni ninguno de los otros campos que traía el cuerpo: {lectura}"
        );
    }

    // ── La retención negativa alineada con el motor (SEC-14) ─────────────────
    //
    // Cubre `api-resources`, requisito «El alta y la edición de una tarea rechazan una retención
    // negativa que no sea -1». El motor (`rustic::retencion`) lee `-1` como «conservar todo» y
    // rechaza cualquier otro negativo o lo que no quepa en un entero de 32 bits, así que el borde
    // tiene que rechazar **lo mismo**: un valor que el motor no sabría leer se para en el formulario
    // en vez de aparecer como un fallo de la primera copia. Las dos capas existen porque protegen de
    // dos caminos distintos —el formulario y una fila escrita por debajo de la API—.

    /// 5.1 — Una regla negativa distinta de `-1` se rechaza nombrando su columna.
    #[tokio::test]
    async fn una_retencion_negativa_distinta_de_menos_uno_se_rechaza() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-de-la-retencion-negativa").await;
        let repositorio =
            repositorio_de_prueba(&router, "repositorio-de-la-retencion-negativa").await;

        let mut cuerpo_de_la_peticion = cuerpo_de_tarea(&target, &repositorio);
        cuerpo_de_la_peticion["keep_daily"] = serde_json::json!(-5);

        let respuesta = post_json(router, "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `-5` no lo lee el motor, así que el borde no lo puede aceptar: contestó {status} \
             con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto");

        assert!(
            mensaje.contains("keep_daily"),
            "el rechazo tiene que nombrar la columna, y el mensaje fue {mensaje:?}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect("la tabla de tareas tiene que poder contarse");

        assert_eq!(
            filas, 0,
            "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
        );
    }

    /// 5.2 — El `-1` se acepta como «conservar todo» y la fila lo guarda.
    #[tokio::test]
    async fn menos_uno_se_acepta_como_conservar_todo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-del-conservar-todo").await;
        let repositorio = repositorio_de_prueba(&router, "repositorio-del-conservar-todo").await;

        let mut cuerpo_de_la_peticion = cuerpo_de_tarea(&target, &repositorio);
        cuerpo_de_la_peticion["keep_daily"] = serde_json::json!(-1);

        let respuesta = post_json(router, "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el `-1` es la forma legítima de pedir «conservar todo»: contestó {status} con {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let guardado: i64 = sqlx::query_scalar("SELECT keep_daily FROM backup_jobs WHERE id = ?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .expect("la fila tiene que estar en el fichero de la base");

        assert_eq!(
            guardado, -1,
            "la fila tiene que guardar el `-1` tal cual, que es lo que el motor lee"
        );
    }

    /// 5.3 — Una regla por encima del entero de la biblioteca se rechaza.
    #[tokio::test]
    async fn una_regla_por_encima_del_entero_de_la_biblioteca_se_rechaza() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let target = target_de_prueba(&router, "target-de-la-retencion-enorme").await;
        let repositorio =
            repositorio_de_prueba(&router, "repositorio-de-la-retencion-enorme").await;

        let mut cuerpo_de_la_peticion = cuerpo_de_tarea(&target, &repositorio);
        cuerpo_de_la_peticion["keep_weekly"] = serde_json::json!(i64::from(i32::MAX) + 1);

        let respuesta = post_json(router, "/api/v1/jobs", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un valor que no cabe en el entero de 32 bits que usa el motor tiene que rechazarse: \
             contestó {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto");

        assert!(
            mensaje.contains("keep_weekly"),
            "el rechazo tiene que nombrar la columna, y el mensaje fue {mensaje:?}"
        );
        assert!(
            mensaje.contains(&i32::MAX.to_string()),
            "el rechazo de un valor por encima del tope tiene que decir cuál es el máximo, y el \
             mensaje fue {mensaje:?}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect("la tabla de tareas tiene que poder contarse");

        assert_eq!(
            filas, 0,
            "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
        );
    }
}
