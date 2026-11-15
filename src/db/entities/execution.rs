//! La tabla `job_executions` del §3: la entidad, el alta y la capa de acceso.
//!
//! Es el histórico: una fila por intento de copia, con su estado, los bytes que se
//! procesaron y subieron, la marca del `snapshot` y las dos fechas. `started_at` es
//! `NOT NULL` y **sin** valor por defecto —lo informa quien crea la ejecución—, y
//! `finished_at` es anulable: una ejecución en curso no tiene final, y `NULL` **no**
//! significa «ahora».
//!
//! **No tiene ninguna columna de secreto**, así que su capa de acceso **no** recibe la
//! clave de cifrado: pasarle una que no se usa sugeriría que aquí se cifra algo.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteRow;
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;

use crate::db::DbError;
use crate::db::entities::target::TipoDeTarget;

/// Estado de una ejecución, tal y como lo declara el `CHECK` del §3.
///
/// El texto almacenado es el nombre de la variante en minúsculas —`pending`,
/// `running`, `success`, `failed`—, que es lo que el `CHECK` acepta y lo que hace que
/// la columna sea legible con `sqlite3`; guardarlo como número rompería el §3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "lowercase")]
pub enum Estado {
    /// Encolada, todavía sin empezar.
    Pending,
    /// En curso.
    Running,
    /// Terminada sin error.
    Success,
    /// Terminada con error.
    Failed,
}

impl Estado {
    /// El texto que el `CHECK` del §3 admite y que la columna guarda.
    ///
    /// Es la **única** otra boca que escribe estos cuatro literales, además del `CHECK`: la
    /// capa de acceso los guarda con `sqlx::Type`, el cable los publica por aquí y el filtro
    /// de la consulta los lee por [`Estado::desde_texto`]. Con la pareja en un solo sitio,
    /// añadir un estado es tocar el `CHECK` y este `match`, y el compilador avisa de la rama
    /// que falta.
    pub fn texto(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }

    /// El estado que corresponde a un texto, o `None` si no es ninguno de los cuatro.
    ///
    /// `None` y no un estado por defecto: quien pregunta por un `status` que no existe ha
    /// escrito mal la petición, y devolverle el listado entero sin decirlo sería el silencio
    /// que esta casa no admite.
    pub fn desde_texto(texto: &str) -> Option<Self> {
        match texto {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "success" => Some(Self::Success),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Fila de `job_executions`.
#[derive(Debug, Clone)]
pub struct Ejecucion {
    /// UUIDv4 en forma canónica con guiones.
    pub id: Uuid,
    /// Tarea a la que pertenece.
    pub job_id: Uuid,
    /// Estado de la ejecución.
    pub status: Estado,
    /// Identificador del `snapshot` devuelto por `rustic_core`; `None` mientras no hay.
    pub snapshot_id: Option<String>,
    /// Bytes procesados.
    pub bytes_processed: i64,
    /// Bytes subidos.
    pub bytes_uploaded: i64,
    /// Mensaje de error; `None` si no falló.
    pub error_message: Option<String>,
    /// Cuándo empezó, en UTC.
    pub started_at: OffsetDateTime,
    /// Cuándo terminó, en UTC; `None` si sigue en curso.
    pub finished_at: Option<OffsetDateTime>,
}

/// Los datos de un alta. **Sin `id`**: lo genera la capa de acceso.
#[derive(Debug, Clone)]
pub struct AltaDeEjecucion {
    /// Tarea a la que pertenece.
    pub job_id: Uuid,
    /// Estado de la ejecución.
    pub status: Estado,
    /// Identificador del `snapshot` devuelto por `rustic_core`.
    pub snapshot_id: Option<String>,
    /// Bytes procesados.
    pub bytes_processed: i64,
    /// Bytes subidos.
    pub bytes_uploaded: i64,
    /// Mensaje de error.
    pub error_message: Option<String>,
    /// Cuándo empezó, en UTC.
    pub started_at: OffsetDateTime,
    /// Cuándo terminó, en UTC.
    pub finished_at: Option<OffsetDateTime>,
}

/// La capa de acceso de `job_executions`.
pub struct Ejecuciones {
    pool: SqlitePool,
}

/// Los filtros y la ventana del listado de `job_executions`.
///
/// Los dos filtros son **opcionales y ausentes por defecto**, y la ventana llega siempre
/// informada: el valor por defecto y el tope son del contrato HTTP, así que quien los decide es
/// el manejador —que es quien conoce `limit`— y la capa de acceso no tiene que saber que
/// existen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FiltroDeEjecuciones {
    /// Tarea por la que filtrar; `None` no filtra.
    pub job_id: Option<Uuid>,
    /// Estado por el que filtrar; `None` no filtra.
    pub status: Option<Estado>,
    /// Cuántas filas devolver como mucho.
    pub limite: i64,
    /// Cuántas filas saltar desde el principio del orden.
    pub desplazamiento: i64,
}

/// El target sobre el que se restaura una instantánea, resuelto por la ejecución que la anotó.
///
/// Es lo que la restauración necesita saber y lo que el §3 **sí** permite resolver: una
/// instantánea no guarda el nombre de su tarea —lo nombra la biblioteca al guardarla—, así que el
/// único camino de una instantánea a su target es el vínculo que la ejecución dejó escrito:
/// `job_executions.snapshot_id` → la tarea → su `target_id`. Inventar otro —deducirlo del nombre
/// del target, o del resumen de la instantánea— sería adivinar sobre datos que no lo dicen.
///
/// Vive en el módulo de `job_executions` y no en el de `backup_targets` porque es la ejecución
/// quien tiene el `snapshot_id`: la pregunta empieza en esta tabla y las otras dos entran a
/// resolver las claves foráneas.
#[derive(Debug, Clone)]
pub struct ObjetivoDeRestauracion {
    /// La tarea de la ejecución que anotó la instantánea.
    pub job_id: Uuid,
    /// El target de esa tarea.
    pub target_id: Uuid,
    /// El tipo de esa fuente, tal y como lo declara el `CHECK` del §3.
    pub target_type: TipoDeTarget,
    /// La ruta del host; `None` si la fuente no es un directorio.
    ///
    /// Es un `Option` porque **ningún `CHECK` de la base** exige la ruta a un target de
    /// directorio: el §3 solo la pide en un comentario de columna —«Requerido para tipo
    /// `directory`»— y quien la exige de verdad es el alta de targets, en el borde HTTP. Una
    /// fila que llegue de otra vía —una importación, un `UPDATE` a mano— puede tener un
    /// `directory` sin ruta, así que quien lo consuma no puede dar por hecho que está: la
    /// restauración lo comprueba y contesta `422` si falta.
    pub host_path: Option<String>,
    /// El nombre del target, para que la respuesta pueda nombrarlo.
    pub target_name: String,
}

// Las sentencias, como constantes: `DbError::Consulta` lleva el texto de la consulta,
// así que la que se ejecuta y la que viaja en el error tienen que ser **la misma
// cadena**. Con el literal repetido en los dos sitios, lo que se ejecuta y lo que se
// registra pueden separarse sin que nada lo delate.
//
// Los valores van **enlazados** (`?1`, `?2`…): ninguna se compone concatenando ni se
// interpola un valor, ni siquiera para armarla en piezas.

/// Columnas del `SELECT`, en el orden del §3 —el mismo que recorre [`desde_fila`]—.
const SQL_LEER: &str = "SELECT id, job_id, status, snapshot_id, bytes_processed, bytes_uploaded, \
     error_message, started_at, finished_at FROM job_executions WHERE id = ?1";

/// Listado de todas las ejecuciones.
///
/// `ORDER BY started_at, id` y no `started_at` a secas: `job_executions` no tiene
/// `created_at`, así que su fecha de nacimiento es la de inicio, y dos ejecuciones que
/// empiezan en el mismo instante empatan. El `id` es el desempate, para que el orden
/// entre dos llamadas seguidas sea estable.
const SQL_LISTAR: &str = "SELECT id, job_id, status, snapshot_id, bytes_processed, bytes_uploaded, \
     error_message, started_at, finished_at FROM job_executions ORDER BY started_at, id";

/// Listado filtrado y paginado, con el orden del contrato.
///
/// El `WHERE` se escribe **una sola vez** y con los dos filtros dentro de la misma sentencia:
/// `?1 IS NULL OR job_id = ?1` no filtra cuando el filtro no llega, así que la consulta es una
/// cadena **estática** y los cuatro valores van enlazados. Componerla en piezas según los
/// filtros presentes sería construir una consulta concatenando, que es lo que la auditoría de
/// este change prohíbe.
///
/// `ORDER BY started_at DESC, id` y no `started_at` a secas: las ejecuciones que arrancan en el
/// mismo segundo empatan —`CURRENT_TIMESTAMP` no tiene más resolución— y sin desempate dos
/// peticiones seguidas con el mismo `offset` pueden repetir una fila y saltarse otra. El
/// descendente es el del contrato: lo último primero, que es lo que el panel pinta.
const SQL_LISTAR_FILTRADO: &str = "SELECT id, job_id, status, snapshot_id, bytes_processed, \
     bytes_uploaded, error_message, started_at, finished_at FROM job_executions \
     WHERE (?1 IS NULL OR job_id = ?1) AND (?2 IS NULL OR status = ?2) \
     ORDER BY started_at DESC, id LIMIT ?3 OFFSET ?4";

/// Alta de una ejecución, devolviendo la fila tal y como quedó.
///
/// Si el `job_id` no existe, la clave foránea rechaza la fila: una ejecución huérfana no
/// llega a existir.
const SQL_ALTA: &str = "INSERT INTO job_executions \
     (id, job_id, status, snapshot_id, bytes_processed, bytes_uploaded, error_message, started_at, \
     finished_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
     RETURNING id, job_id, status, snapshot_id, bytes_processed, bytes_uploaded, error_message, \
     started_at, finished_at";

/// Alta de una ejecución **solo si la tarea no tiene ya una en curso**, devolviendo la fila
/// tal y como quedó.
///
/// Es una **sola sentencia** y no un «leer y luego insertar», y ahí está todo el asunto:
/// entre la lectura y la escritura de un `alta` caben las peticiones de otro, así que dos
/// disparos simultáneos leerían los dos «no hay ninguna en curso» e insertarían los dos, que
/// es exactamente lo que el contrato prohíbe. Aquí, o la fila entra y se devuelve, o no
/// entra y no se devuelve nada: sin transacción explícita, una sola sentencia es atómica por
/// construcción, y las conexiones que coinciden serializan en la escritura de SQLite, así
/// que la que llega después reevalúa su `WHERE NOT EXISTS` **dentro de su propia escritura**
/// y ve la fila de la anterior.
///
/// Los dos estados que bloquean van **enlazados** (`?10`, `?11`), no escritos en el texto:
/// la única boca que escribe esos literales es [`Estado::texto`] —y el `CHECK` del §3—, y
/// una copia aquí sería una segunda boca que se quedaría atrás el día que se añada un
/// estado sin que nada lo delatara.
///
/// `job_id` se enlaza **una vez** y se nombra dos veces (`?2`), en la fila y en la guarda:
/// es el mismo valor, y enlazarlo dos veces dejaría dos copias que podrían separarse.
///
/// Si el `job_id` no existe, la clave foránea rechaza la fila igual que en [`SQL_ALTA`]: una
/// ejecución huérfana no llega a existir, y eso **no** es «la tarea ya estaba en curso».
const SQL_ALTA_SI_LIBRE: &str = "INSERT INTO job_executions \
     (id, job_id, status, snapshot_id, bytes_processed, bytes_uploaded, error_message, started_at, \
     finished_at) \
     SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9 \
     WHERE NOT EXISTS (SELECT 1 FROM job_executions WHERE job_id = ?2 AND status IN (?10, ?11)) \
     RETURNING id, job_id, status, snapshot_id, bytes_processed, bytes_uploaded, error_message, \
     started_at, finished_at";

/// Actualización de la fila entera.
///
/// `started_at` **no** entra: es la fecha de nacimiento de la fila —el equivalente del
/// `created_at` de las otras tablas—, no un campo que se edite. Lo que cambia mientras
/// una ejecución vive es su estado, sus contadores, su error y su final.
const SQL_ACTUALIZAR: &str = "UPDATE job_executions SET status = ?1, snapshot_id = ?2, \
     bytes_processed = ?3, bytes_uploaded = ?4, error_message = ?5, finished_at = ?6 WHERE id = ?7";

/// Borrado por identificador.
const SQL_BORRAR: &str = "DELETE FROM job_executions WHERE id = ?1";

/// El target al que pertenece una instantánea, por el vínculo que dejó su ejecución.
///
/// Las tres tablas van unidas **por sus claves foráneas** —el plan del listado filtrado ya usa
/// esos índices— y no por ningún campo de texto: el `repository_id` entra como filtro porque dos
/// repositorios pueden guardar la misma instantánea —el identificador es el resumen de su
/// contenido, no un nombre único—, y sin él una restauración pedida sobre un repositorio podría
/// resolverse contra la copia de otro.
///
/// `ORDER BY ... LIMIT 1` y no un `SELECT` a secas: una instantánea puede haberse copiado varias
/// veces y la pregunta es por **una** —la más reciente es la que el panel pinta primero—, así que
/// el orden decide cuál y lo decide de forma estable, con el identificador de la fila como
/// desempate. Ordenar `e.started_at DESC` es lo mismo que hace el histórico: lo último primero.
const SQL_OBJETIVO_DE_LA_INSTANTANEA: &str = "SELECT e.job_id, t.id, t.target_type, t.host_path, \
     t.name FROM job_executions e \
     JOIN backup_jobs j ON j.id = e.job_id \
     JOIN backup_targets t ON t.id = j.target_id \
     WHERE e.snapshot_id = ?1 AND j.repository_id = ?2 \
     ORDER BY e.started_at DESC, e.id DESC LIMIT 1";

/// Los objetivos de **varias** instantáneas, resueltos en **una sola** lectura.
///
/// Es la hermana por lotes de [`SQL_OBJETIVO_DE_LA_INSTANTANEA`] y conserva su semántica entera: las
/// mismas tres tablas unidas por sus claves foráneas, el `repository_id` como filtro y el mismo
/// orden —`e.started_at DESC, e.id DESC`—, del que depende que la **primera** fila de cada
/// instantánea sea la suya más reciente, que es lo que allí decide el `LIMIT 1`. Lo único que cambia
/// es que esta trae las filas de **todas** las instantáneas del lote a la vez: el explorador del
/// panel resuelve así el target de las 200 de su ventana en una lectura y no en 200.
///
/// # Por qué la lista de instantáneas viaja enlazada y no como marcadores del `IN`
///
/// Un `IN` ortodoxo —un marcador por identificador— obligaría a **armar la sentencia en piezas**,
/// porque el número de instantáneas lo pone quien llama, y esta casa tiene escrito lo contrario: la
/// sentencia que se ejecuta y la que viaja al registro tienen que ser **la misma constante** —ver el
/// comentario de las `SQL_*`—, y eso es lo que hace estructuralmente imposible que un valor acabe
/// dentro de una consulta. Aquí la lista **es un valor enlazado**: viaja como un array JSON en `?1`
/// y `json_each` lo abre en filas dentro del propio SQLite, así que la consulta es una constante,
/// ningún identificador entra en su texto y no hay piezas que armar.
///
/// `json_each` es parte de SQLite (JSON1) y no una extensión: el binario la trae compilada. Y de ahí
/// sale una consecuencia que conviene tener dicha: el límite de 999 parámetros enlazados de SQLite
/// **no se roza**, porque esta consulta enlaza dos —el array y el repositorio— y no uno por
/// instantánea, así que no hay lote que trocear mientras las instantáneas quepan en un solo array.
const SQL_OBJETIVOS_DE_LAS_INSTANTANEAS: &str = "SELECT e.snapshot_id, e.job_id, t.id, \
     t.target_type, t.host_path, t.name FROM job_executions e \
     JOIN backup_jobs j ON j.id = e.job_id \
     JOIN backup_targets t ON t.id = j.target_id \
     WHERE e.snapshot_id IN (SELECT value FROM json_each(?1)) AND j.repository_id = ?2 \
     ORDER BY e.started_at DESC, e.id DESC";

/// Texto del error cuando lo que falla es **armar la lista** de instantáneas que viaja enlazada.
///
/// No es una sentencia ejecutada —la lista se arma antes de consultar— y por eso no se confunde con
/// las `SQL_*`: dice dónde falló, que es codificar el array que `json_each` abre en filas.
const CONSULTA_LISTA_DE_INSTANTANEAS: &str = "lista JSON de instantáneas del lote";

/// Texto del error cuando falla el **mapeo** de una fila ya traída.
///
/// No es una sentencia ejecutada —la fila ya llegó—, así que no se confunde con las
/// `SQL_*`: dice dónde falló, que es leer una columna. Es lo que el registro necesita
/// saber cuando el fallo no viene del motor.
const CONSULTA_MAPEO: &str = "mapeo de la fila de job_executions";

impl Ejecuciones {
    /// Construye la capa de acceso sobre un pool.
    ///
    /// **Sin clave de cifrado**: `job_executions` no tiene ninguna columna de secreto, y
    /// pedir una que no se usa sugeriría que aquí se cifra algo.
    pub fn nuevo(pool: SqlitePool) -> Self {
        Ejecuciones { pool }
    }

    /// Da de alta una ejecución y devuelve la fila tal y como quedó.
    ///
    /// Una ejecución que apunta a una tarea que no existe se rechaza con error: la clave
    /// foránea del §3 no admite huérfanas.
    pub async fn alta(&self, alta: AltaDeEjecucion) -> Result<Ejecucion, DbError> {
        // El identificador se genera **aquí**, no en la base: SQLite no tiene `uuid()`
        // y un valor por defecto en el DDL obligaría a interpolarlo en la sentencia.
        let id = Uuid::new_v4();

        let fila = sqlx::query(SQL_ALTA)
            // El `id` y la clave foránea van como texto con guiones: un `bind` de `Uuid`
            // los escribiría como un BLOB de 16 bytes y el §3 dice `TEXT`.
            .bind(id.hyphenated().to_string())
            .bind(alta.job_id.hyphenated().to_string())
            .bind(alta.status)
            .bind(&alta.snapshot_id)
            .bind(alta.bytes_processed)
            .bind(alta.bytes_uploaded)
            .bind(&alta.error_message)
            // Las fechas se normalizan a UTC **al enlazarlas**: el `Encode` de `time`
            // conserva el desplazamiento del valor, y el §3 dice que las fechas son UTC,
            // así que un llamante con `+02:00` no puede dejar la columna fuera de UTC.
            // Es el mismo instante, en otra representación.
            .bind(alta.started_at.to_offset(UtcOffset::UTC))
            .bind(
                alta.finished_at
                    .map(|fecha| fecha.to_offset(UtcOffset::UTC)),
            )
            .fetch_one(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ALTA))?;

        desde_fila(&fila)
    }

    /// Lee una ejecución por su identificador: `Ok(None)` si no está.
    ///
    /// «No está» y «no se pudo leer» son cosas distintas: lo primero es `Ok(None)` y lo
    /// segundo, un error. Confundirlos convertiría una base caída en «no existe», que
    /// es lo que hace que un fallo se persiga en el sitio equivocado.
    pub async fn leer(&self, id: &Uuid) -> Result<Option<Ejecucion>, DbError> {
        let fila = sqlx::query(SQL_LEER)
            .bind(id.hyphenated().to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_LEER))?;

        match fila {
            Some(fila) => desde_fila(&fila).map(Some),
            None => Ok(None),
        }
    }

    /// Lista las ejecuciones con un orden **estable**.
    pub async fn listar(&self) -> Result<Vec<Ejecucion>, DbError> {
        let filas = sqlx::query(SQL_LISTAR)
            .fetch_all(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_LISTAR))?;

        filas.iter().map(desde_fila).collect()
    }

    /// Lista las ejecuciones que cumplen el filtro, en el orden del contrato.
    ///
    /// La ventana es obligatoria —llega ya resuelta— porque el histórico es la tabla que más
    /// crece: sin tope, un listado materializaría la tabla entera en memoria y ocuparía una de
    /// las conexiones del pool mientras el motor quiere escribir.
    pub async fn listar_filtrado(
        &self,
        filtro: &FiltroDeEjecuciones,
    ) -> Result<Vec<Ejecucion>, DbError> {
        let filas = sqlx::query(SQL_LISTAR_FILTRADO)
            .bind(filtro.job_id.map(|id| id.hyphenated().to_string()))
            .bind(filtro.status.map(Estado::texto))
            .bind(filtro.limite)
            .bind(filtro.desplazamiento)
            .fetch_all(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_LISTAR_FILTRADO))?;

        filas.iter().map(desde_fila).collect()
    }

    /// Resuelve el target sobre el que restaurar una instantánea, o `None` si no hay vínculo.
    ///
    /// «No hay vínculo» y «no se pudo leer» son cosas distintas, como en [`Ejecuciones::leer`]: lo
    /// primero es `Ok(None)` —una instantánea que está en el repositorio y no en el histórico, que
    /// es el caso de una copia hecha por otra herramienta— y lo segundo un error. Confundirlos
    /// convertiría una base caída en «esa instantánea no tiene target», que es un rechazo con el
    /// código equivocado y un diagnóstico que manda a buscar el fallo donde no está.
    ///
    /// El `repository_id` acota el vínculo al repositorio por el que se pregunta: la misma
    /// instantánea puede estar en dos, y la restauración se pide contra uno.
    pub async fn objetivo_de_la_instantanea(
        &self,
        snapshot_id: &str,
        repository_id: &Uuid,
    ) -> Result<Option<ObjetivoDeRestauracion>, DbError> {
        let fila = sqlx::query(SQL_OBJETIVO_DE_LA_INSTANTANEA)
            .bind(snapshot_id)
            .bind(repository_id.hyphenated().to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_OBJETIVO_DE_LA_INSTANTANEA))?;

        fila.as_ref().map(objetivo_desde_fila).transpose()
    }

    /// Resuelve el objetivo de **varias** instantáneas de una vez: las que tienen vínculo, con él.
    ///
    /// Es la hermana por lotes de [`Ejecuciones::objetivo_de_la_instantanea`] y comparte su semántica
    /// entera —el mismo camino `job_executions.snapshot_id` → la tarea → su `target_id`, el mismo
    /// «gana la ejecución más reciente» y el mismo acotado por `repository_id`, que hace falta porque
    /// la misma instantánea puede estar en dos repositorios—, pero resuelve el lote completo en una
    /// lectura en vez de una por instantánea.
    ///
    /// **Una instantánea sin vínculo no sale en el mapa**, y eso es contrato y no olvido: una copia
    /// que hizo otra herramienta —o cuya fila de `job_executions` se borró en cascada con su tarea—
    /// está en el repositorio y no tiene target. Quien pregunta tiene que poder distinguir eso de un
    /// fallo de base, y la distinción es la misma que el `Ok(None)` de la hermana: la ausencia de
    /// entrada es lo primero y el `Err` es lo segundo. Por eso el mapa no se rellena con
    /// `None`s —«sin vínculo» y «vínculo sin datos» serían la misma cosa— ni con entradas de
    /// relleno.
    ///
    /// Las **claves** son los identificadores tal y como llegaron —el de la columna `snapshot_id`,
    /// que es el que publica el listado—, para que quien pregunte pueda casarlos sin normalizar nada.
    ///
    /// # Errors
    ///
    /// Un [`DbError::Consulta`] si la lista no se puede codificar o la sentencia falla.
    pub async fn objetivos_de_las_instantaneas(
        &self,
        snapshot_ids: &[String],
        repository_id: &Uuid,
    ) -> Result<HashMap<String, ObjetivoDeRestauracion>, DbError> {
        // Sin identificadores no hay nada que resolver: la consulta sobra, y un listado vacío no
        // tiene por qué costar una ida y vuelta a la base.
        if snapshot_ids.is_empty() {
            return Ok(HashMap::new());
        }

        // El array lo arma `serde_json`, que es quien sabe escapar: ningún identificador se pega al
        // texto de la consulta. El fallo viaja como `sqlx::Error::Encode` —y no estrena una
        // variante— porque es exactamente lo que significa: un valor que no se pudo codificar para
        // viajar a la base.
        let lista = serde_json::to_string(snapshot_ids).map_err(|causa| DbError::Consulta {
            consulta: CONSULTA_LISTA_DE_INSTANTANEAS,
            causa: sqlx::Error::Encode(Box::new(causa)),
        })?;

        let filas = sqlx::query(SQL_OBJETIVOS_DE_LAS_INSTANTANEAS)
            .bind(lista)
            .bind(repository_id.hyphenated().to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_OBJETIVOS_DE_LAS_INSTANTANEAS))?;

        let mut objetivos = HashMap::with_capacity(snapshot_ids.len());

        for fila in &filas {
            // El `IN` no casa nunca con `NULL`, así que una fila que ha llegado hasta aquí tiene su
            // `snapshot_id` escrito: leerlo no puede degenerar en una clave vacía.
            let instantanea = columna::<String>(fila, "snapshot_id")?;

            // La primera fila de cada instantánea es la más reciente —el `ORDER BY` las trae así—,
            // así que la instantánea que ya ocupa un hueco no se toca: es el mismo desenlace que
            // resuelve el `LIMIT 1` de la hermana, y aquí hace falta porque el lote trae las filas de
            // todas.
            if let Entry::Vacant(hueco) = objetivos.entry(instantanea) {
                hueco.insert(objetivo_desde_fila(fila)?);
            }
        }

        Ok(objetivos)
    }

    /// Actualiza la fila **entera** y devuelve si existía.
    pub async fn actualizar(&self, ejecucion: &Ejecucion) -> Result<bool, DbError> {
        let resultado = sqlx::query(SQL_ACTUALIZAR)
            .bind(ejecucion.status)
            .bind(&ejecucion.snapshot_id)
            .bind(ejecucion.bytes_processed)
            .bind(ejecucion.bytes_uploaded)
            .bind(&ejecucion.error_message)
            // Misma normalización que en el alta: el §3 dice UTC.
            .bind(
                ejecucion
                    .finished_at
                    .map(|fecha| fecha.to_offset(UtcOffset::UTC)),
            )
            .bind(ejecucion.id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ACTUALIZAR))?;

        Ok(resultado.rows_affected() == 1)
    }

    /// Da de alta una ejecución **solo si la tarea no tiene ya una en curso**.
    ///
    /// Es la guarda del disparo manual —`POST /api/v1/jobs/:id/run` encola una ejecución y
    /// contesta `202`, y `409` si esa tarea ya tiene una `pending` o una `running`—: el
    /// contrato dice que **no** se cree una segunda ejecución, y la comprobación tiene que
    /// ser atómica porque dos peticiones simultáneas no pueden leer las dos «no hay
    /// ninguna» e insertar las dos. Por eso esto es **una sola sentencia** —la constante
    /// `SQL_ALTA_SI_LIBRE`— y no un `leer` seguido de un `alta`: sin transacción explícita,
    /// la sentencia es atómica por construcción.
    ///
    /// Devuelve `Ok(Some(fila))` cuando el alta entra, y `Ok(None)` cuando la guarda la
    /// rechaza. `None` significa **«la tarea ya tenía una en curso»**, y nunca «no se pudo»:
    /// un fallo de base sigue siendo `Err`, porque confundir los dos convertiría una base
    /// caída en un `409` que manda al operador a mirar una ejecución que no existe.
    ///
    /// La guarda mira **las filas que ya hay** para ese `job_id`, con independencia del
    /// estado que traiga el alta nueva: quien llama pasa [`Estado::Pending`] —que es el
    /// estado con el que se encola—, pero la decisión no depende de eso.
    ///
    /// Si el `job_id` no existe, la clave foránea rechaza la fila y sale un `DbError`, igual
    /// que en [`Ejecuciones::alta`]: una ejecución huérfana no llega a existir. Eso **no** se
    /// convierte en `None`, que significaría «ya está corriendo» y mandaría al operador a
    /// mirar donde no es.
    ///
    /// `started_at` lo informa quien llama, como en el alta normal.
    pub async fn alta_si_no_hay_ninguna_en_curso(
        &self,
        alta: AltaDeEjecucion,
    ) -> Result<Option<Ejecucion>, DbError> {
        // El identificador se genera **aquí**, no en la base: SQLite no tiene `uuid()`
        // y un valor por defecto en el DDL obligaría a interpolarlo en la sentencia.
        let id = Uuid::new_v4();

        let fila = sqlx::query(SQL_ALTA_SI_LIBRE)
            // El `id` y la clave foránea van como texto con guiones: un `bind` de `Uuid`
            // los escribiría como un BLOB de 16 bytes y el §3 dice `TEXT`.
            .bind(id.hyphenated().to_string())
            .bind(alta.job_id.hyphenated().to_string())
            .bind(alta.status)
            .bind(&alta.snapshot_id)
            .bind(alta.bytes_processed)
            .bind(alta.bytes_uploaded)
            .bind(&alta.error_message)
            // Las fechas se normalizan a UTC **al enlazarlas**, por lo mismo que en el
            // alta normal: el `Encode` de `time` conserva el desplazamiento del valor y
            // el §3 dice que las fechas son UTC.
            .bind(alta.started_at.to_offset(UtcOffset::UTC))
            .bind(
                alta.finished_at
                    .map(|fecha| fecha.to_offset(UtcOffset::UTC)),
            )
            // Los dos estados que bloquean, por la boca que los escribe: la guarda no
            // escribe literales de estado en el SQL.
            .bind(Estado::Pending.texto())
            .bind(Estado::Running.texto())
            .fetch_optional(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ALTA_SI_LIBRE))?;

        // Sin fila devuelta no hay alta: el `WHERE` de la sentencia no encontró hueco, que
        // es el `None` del contrato. No es un fallo y por eso no es un `Err`.
        match fila {
            Some(fila) => desde_fila(&fila).map(Some),
            None => Ok(None),
        }
    }

    /// Borra la fila y devuelve si existía.
    pub async fn borrar(&self, id: &Uuid) -> Result<bool, DbError> {
        let resultado = sqlx::query(SQL_BORRAR)
            .bind(id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_BORRAR))?;

        Ok(resultado.rows_affected() == 1)
    }
}

/// Envuelve un fallo de `sqlx` con **la misma** sentencia que se ejecutó.
///
/// La sentencia entra por parámetro en vez de repetirse en cada `map_err`: así la que
/// se ejecuta y la que viaja en el error son una sola cadena —la constante—, y no dos
/// literales que pueden separarse.
fn error_de_consulta(consulta: &'static str) -> impl FnOnce(sqlx::Error) -> DbError {
    move |causa| DbError::Consulta { consulta, causa }
}

/// Lee una columna de la fila, conservando la causa si falta o no tiene el tipo.
///
/// El parámetro es **`Option`** cuando la columna lo es: así `NULL` llega como `None` y
/// nunca se confunde con la cadena vacía ni con una fecha inventada.
fn columna<'fila, T>(fila: &'fila SqliteRow, nombre: &str) -> Result<T, DbError>
where
    T: sqlx::Decode<'fila, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
{
    fila.try_get::<T, _>(nombre)
        .map_err(|causa| DbError::Consulta {
            consulta: CONSULTA_MAPEO,
            causa,
        })
}

/// Mapea la fila del vínculo a [`ObjetivoDeRestauracion`].
///
/// `host_path` es la única columna anulable: un target de base de datos no tiene ruta de host y
/// sale como `None`, nunca como cadena vacía —el §3 distingue `NULL` de `''` y el mapeo no lo
/// confunde—.
fn objetivo_desde_fila(fila: &SqliteRow) -> Result<ObjetivoDeRestauracion, DbError> {
    let job_id = columna::<String>(fila, "job_id")?;
    let job_id = Uuid::parse_str(&job_id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO,
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    let target_id = columna::<String>(fila, "id")?;
    let target_id = Uuid::parse_str(&target_id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO,
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    Ok(ObjetivoDeRestauracion {
        job_id,
        target_id,
        target_type: columna(fila, "target_type")?,
        host_path: columna(fila, "host_path")?,
        target_name: columna(fila, "name")?,
    })
}

/// Mapea una fila de `job_executions` a la entidad.
///
/// No descifra nada: esta tabla no tiene secretos. `finished_at` es anulable y sale como
/// `None` cuando la columna es `NULL`, sin inventarse ninguna fecha.
fn desde_fila(fila: &SqliteRow) -> Result<Ejecucion, DbError> {
    // El `id` viaja como `TEXT` con guiones, así que la conversión a `Uuid` es del
    // mapeo y no de `sqlx`. Su error **no** es un `sqlx::Error`, y envolverlo en
    // `sqlx::Error::Decode` es lo que lo hace viajar por `DbError::Consulta` con la
    // causa intacta, sin estrenar una variante para un fallo que ya tiene sitio.
    let id = columna::<String>(fila, "id")?;
    let id = Uuid::parse_str(&id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO,
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    let job_id = columna::<String>(fila, "job_id")?;
    let job_id = Uuid::parse_str(&job_id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO,
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    Ok(Ejecucion {
        id,
        job_id,
        status: columna(fila, "status")?,
        snapshot_id: columna(fila, "snapshot_id")?,
        bytes_processed: columna(fila, "bytes_processed")?,
        bytes_uploaded: columna(fila, "bytes_uploaded")?,
        error_message: columna(fila, "error_message")?,
        started_at: columna(fila, "started_at")?,
        finished_at: columna(fila, "finished_at")?,
    })
}

#[cfg(test)]
mod tests {
    use futures_util::future::join_all;
    use sqlx::SqlitePool;
    use time::macros::datetime;
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    use super::{AltaDeEjecucion, Ejecuciones, Estado, FiltroDeEjecuciones};
    use crate::db::DbError;
    use crate::db::entities::job::{AltaDeTarea, Tareas};
    use crate::db::entities::repository::{AltaDeRepositorio, Repositorios};
    use crate::db::entities::target::{AltaDeTarget, Targets, TipoDeTarget};
    use crate::secret::SecretString;
    use crate::test_support::{DirectorioDePrueba, base_con_clave};

    /// Identificador del `snapshot` de las pruebas: distinguible y largo.
    const SNAPSHOT: &str = "snapshot-distinguible-000000000000000000000001";

    /// Un segundo `snapshot`, distinguible del primero: el lote resuelve **varias** instantáneas y
    /// con una sola no se vería si cada una trae la suya.
    const OTRA_INSTANTANEA: &str = "snapshot-distinguible-000000000000000000000002";

    /// Un `SecretString` con el valor dado.
    fn secreto(valor: &str) -> SecretString {
        SecretString::new(valor.to_owned())
    }

    /// Un alta de repositorio mínima, para satisfacer la clave foránea.
    fn alta_repositorio(nombre: &str) -> AltaDeRepositorio {
        AltaDeRepositorio {
            name: nombre.to_owned(),
            endpoint: format!("https://s3.example.test/{nombre}"),
            bucket: format!("cubo-{nombre}"),
            region: "eu-west-1".to_owned(),
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_owned(),
            secret_access_key: secreto("AKIAIOSFODNN7EXAMPLE-de-ejecucion"),
            passphrase: secreto("passphrase-distinguible-de-ejecucion"),
        }
    }

    /// Un alta de target de directorio mínima, para satisfacer la clave foránea.
    fn alta_target(nombre: &str) -> AltaDeTarget {
        AltaDeTarget {
            name: nombre.to_owned(),
            target_type: TipoDeTarget::Directory,
            container_name: None,
            db_user: None,
            db_password: None,
            db_name: None,
            host_path: Some(format!("/srv/{nombre}")),
            db_path: None,
        }
    }

    /// Una ejecución terminada con éxito, con valores distinguibles.
    fn alta_ejecucion(job_id: &Uuid) -> AltaDeEjecucion {
        let started_at = OffsetDateTime::now_utc();

        AltaDeEjecucion {
            job_id: *job_id,
            status: Estado::Success,
            snapshot_id: Some(SNAPSHOT.to_owned()),
            bytes_processed: 1024,
            bytes_uploaded: 512,
            error_message: None,
            started_at,
            finished_at: Some(started_at + Duration::seconds(30)),
        }
    }

    /// Un alta de ejecución con la instantánea, la tarea y el instante dados.
    ///
    /// El desempate del contrato es la fecha —lo último primero—, así que las pruebas del lote
    /// necesitan fechas **elegidas** y no las de `now_utc` que trae [`alta_ejecucion`]: dos altas
    /// seguidas podrían caer en el mismo instante y el orden quedaría en manos del desempate por
    /// `id`, que es lo que aquí no se quiere medir.
    fn alta_con(instantanea: &str, job_id: &Uuid, started_at: OffsetDateTime) -> AltaDeEjecucion {
        AltaDeEjecucion {
            job_id: *job_id,
            status: Estado::Success,
            snapshot_id: Some(instantanea.to_owned()),
            bytes_processed: 1024,
            bytes_uploaded: 512,
            error_message: None,
            started_at,
            finished_at: Some(started_at + Duration::seconds(30)),
        }
    }

    /// Un alta **encolada**: en `pending`, sin `snapshot`, sin error y sin final.
    ///
    /// Es la forma que tiene el alta del disparo manual —la ejecución se encola y se
    /// contesta `202`—, así que es la que ejercitan las pruebas de la guarda. Los tres
    /// campos anulables se dejan a `None` y no a lo que traiga [`alta_ejecucion`]: una
    /// ejecución encolada no ha visto ningún `snapshot` ni ha fallado, y una fecha de
    /// final aquí sería una ejecución terminada disfrazada de encolada.
    fn alta_encolada(job_id: &Uuid) -> AltaDeEjecucion {
        AltaDeEjecucion {
            status: Estado::Pending,
            snapshot_id: None,
            error_message: None,
            finished_at: None,
            ..alta_ejecucion(job_id)
        }
    }

    /// Un pool migrado con un repositorio, un target y una tarea ya dados de alta: lo
    /// que `job_executions` necesita para no ser huérfana.
    async fn base_con_ejecucion_posible() -> (DirectorioDePrueba, SqlitePool, Ejecuciones, Uuid) {
        let (directorio, pool, clave) = base_con_clave().await;

        let repositorio = Repositorios::nuevo(pool.clone(), clave.clone())
            .alta(alta_repositorio("repo-de-ejecucion"))
            .await
            .expect("el alta del repositorio no puede fallar");

        let target = Targets::nuevo(pool.clone(), clave)
            .alta(alta_target("target-de-ejecucion"))
            .await
            .expect("el alta del target no puede fallar");

        let tarea = Tareas::nuevo(pool.clone())
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
            .expect("el alta de la tarea no puede fallar");

        let ejecuciones = Ejecuciones::nuevo(pool.clone());

        (directorio, pool, ejecuciones, tarea.id)
    }

    /// Cuántas filas de `job_executions` tiene la tarea dada, **con SQL directo**.
    ///
    /// Se cuenta contra la tabla y no con la capa de acceso a propósito: lo que estas
    /// pruebas afirman es que la guarda **no dejó fila**, y una comprobación que pasara
    /// por la misma capa que se está probando podría salir bien por la razón equivocada
    /// —una `listar_filtrado` que se olvidara de un filtro también diría «no hay»—. El
    /// recuento de la tabla es lo único que no comparte código con lo que se mide.
    async fn filas_de_la_tarea(pool: &SqlitePool, job_id: &Uuid) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM job_executions WHERE job_id = ?1")
            .bind(job_id.hyphenated().to_string())
            .fetch_one(pool)
            .await
            .expect("el recuento de filas debe poder leerse")
    }

    /// Una **segunda** tarea sobre el mismo target y el mismo repositorio que la de la
    /// base de prueba.
    ///
    /// Existe para medir que la guarda es **por tarea**: sin una segunda tarea, «no
    /// bloquea a la de al lado» no se puede afirmar. Se construye leyendo la primera y
    /// reutilizando sus dos claves foráneas en vez de dar de alta otro repositorio y otro
    /// target: el §3 exige que las dos existan, y volver a montarlos aquí abriría una
    /// segunda forma de armar el aparato que podría separarse de la primera.
    async fn tarea_hermana(pool: &SqlitePool, job_id: &Uuid) -> Uuid {
        let tareas = Tareas::nuevo(pool.clone());
        let original = tareas
            .leer(job_id)
            .await
            .expect("la lectura de la tarea no puede fallar")
            .expect("la tarea de la base de prueba tiene que estar");

        tareas
            .alta(AltaDeTarea {
                target_id: original.target_id,
                repository_id: original.repository_id,
                cron_expression: "0 4 * * *".to_owned(),
                keep_daily: 7,
                keep_weekly: 4,
                keep_monthly: 12,
                enabled: true,
            })
            .await
            .expect("el alta de la tarea hermana no puede fallar")
            .id
    }

    /// 1.30 — El ciclo completo de una ejecución, con su estado y su `status` guardado
    /// **como texto**.
    ///
    /// La comprobación con SQL directo es la que fija que el enumerado no se guarde como
    /// número: `success` tiene que estar en la columna tal cual.
    #[tokio::test]
    async fn el_ciclo_de_una_ejecucion_y_su_estado_guardado_como_texto() {
        let (_directorio, pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;

        let dado_de_alta = ejecuciones
            .alta(alta_ejecucion(&job_id))
            .await
            .expect("el alta no puede fallar");

        let leido = ejecuciones
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila recién dada de alta debe estar");

        assert_eq!(leido.id, dado_de_alta.id, "el id leído no es el del alta");
        assert_eq!(leido.job_id, job_id);
        assert_eq!(leido.status, Estado::Success);
        assert_eq!(leido.snapshot_id.as_deref(), Some(SNAPSHOT));
        assert_eq!(leido.bytes_processed, 1024);
        assert_eq!(leido.bytes_uploaded, 512);
        assert_eq!(leido.error_message, None);
        assert_eq!(leido.started_at, dado_de_alta.started_at);
        assert_eq!(leido.finished_at, dado_de_alta.finished_at);
        assert!(
            leido.finished_at.expect("terminó") > leido.started_at,
            "el final debe ser posterior al inicio"
        );

        let estado_almacenado: String =
            sqlx::query_scalar("SELECT status FROM job_executions WHERE id = ?1")
                .bind(dado_de_alta.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el estado debe poder leerse");

        assert_eq!(
            estado_almacenado, "success",
            "el enumerado no se guardó como texto, sino como {estado_almacenado}"
        );

        let listado = ejecuciones
            .listar()
            .await
            .expect("el listado no puede fallar");
        assert_eq!(listado.len(), 1, "el listado debe traer la ejecución");

        let mut cambiada = leido.clone();
        cambiada.status = Estado::Failed;
        cambiada.error_message = Some("fallo distinguible de la prueba 1-30".to_owned());

        assert!(
            ejecuciones
                .actualizar(&cambiada)
                .await
                .expect("la actualización no puede fallar"),
            "la fila existía, así que la actualización debe decir que sí"
        );

        let releida = ejecuciones
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe seguir existiendo");

        assert_eq!(releida.status, Estado::Failed, "el estado tocado no cambió");
        assert_eq!(
            releida.error_message.as_deref(),
            Some("fallo distinguible de la prueba 1-30"),
            "el mensaje de error no se escribió"
        );
        assert_eq!(
            releida.bytes_processed, leido.bytes_processed,
            "los contadores no debían cambiar"
        );
        assert_eq!(
            releida.started_at, dado_de_alta.started_at,
            "el started_at no debía cambiar"
        );

        assert!(
            ejecuciones
                .borrar(&dado_de_alta.id)
                .await
                .expect("el borrado no puede fallar"),
            "la fila existía, así que el borrado debe decir que sí"
        );

        let tras_borrar = ejecuciones
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar");

        assert!(tras_borrar.is_none(), "la fila debía desaparecer");
    }

    /// 1.31 — Una ejecución con `finished_at` a `NULL` se lee como `None` y **no** como
    /// una fecha inventada.
    #[tokio::test]
    async fn el_finished_at_nulo_se_lee_como_none() {
        let (_directorio, pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;

        // Una ejecución en curso: empezó y todavía no ha terminado.
        let en_curso = ejecuciones
            .alta(AltaDeEjecucion {
                status: Estado::Running,
                snapshot_id: None,
                finished_at: None,
                ..alta_ejecucion(&job_id)
            })
            .await
            .expect("el alta no puede fallar");

        let leida = ejecuciones
            .leer(&en_curso.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert_eq!(
            leida.finished_at, None,
            "un finished_at NULL debe leerse como `None`, no como una fecha inventada"
        );
        assert_eq!(leida.status, Estado::Running);
        assert_eq!(
            leida.snapshot_id, None,
            "una ejecución sin snapshot debe leerse sin él"
        );

        // Y en la columna es `NULL` de verdad, no una fecha cualquiera.
        let es_nulo: i64 =
            sqlx::query_scalar("SELECT finished_at IS NULL FROM job_executions WHERE id = ?1")
                .bind(en_curso.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el `IS NULL` debe poder leerse");

        assert_eq!(es_nulo, 1, "la columna no es NULL, sino otra cosa");
    }

    /// Una fecha escrita por la aplicación **vuelve como UTC**.
    ///
    /// Sin esta prueba el escenario no estaba cubierto de verdad: lo que había comparaba
    /// dos fechas **leídas** —las dos del camino de decodificación—, así que una corrupción
    /// sistemática del encode/decode habría movido a las dos por igual y el test habría
    /// seguido verde.
    ///
    /// Aquí se enlaza una fecha **conocida** y con desplazamiento **distinto de cero**, y se
    /// afirma **contra la entrada**: mismo instante y desplazamiento cero al volver. El
    /// `Encode` de `time` conserva el desplazamiento del valor, así que sin la
    /// normalización del alta la columna no estaría en UTC y el §3 quedaría incumplido.
    #[tokio::test]
    async fn una_fecha_con_desplazamiento_vuelve_en_utc() {
        let (_directorio, _pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;

        let inicio = datetime!(2026-03-14 09:26:53 +2:30);
        let fin = inicio + Duration::minutes(7);

        assert!(
            !inicio.offset().is_utc(),
            "el aparato de la prueba no usó un desplazamiento distinto de cero"
        );

        let dado_de_alta = ejecuciones
            .alta(AltaDeEjecucion {
                started_at: inicio,
                finished_at: Some(fin),
                ..alta_ejecucion(&job_id)
            })
            .await
            .expect("el alta no puede fallar");

        let leido = ejecuciones
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert!(
            leido.started_at.offset().is_utc(),
            "el started_at volvió con desplazamiento, no en UTC: {}",
            leido.started_at
        );
        assert_eq!(
            leido.started_at, inicio,
            "el instante guardado no es el de entrada"
        );

        let fin_leido = leido
            .finished_at
            .expect("la ejecución se dio de alta terminada");

        assert!(
            fin_leido.offset().is_utc(),
            "el finished_at volvió con desplazamiento, no en UTC: {fin_leido}"
        );
        assert_eq!(fin_leido, fin, "el instante final no es el de entrada");
    }

    /// El listado filtrado acota por los dos filtros, ordena descendente por la fecha y
    /// desempata por el identificador.
    ///
    /// Fija la **sentencia** y no el manejador: lo que se afirma es que un filtro ausente no
    /// filtra, que el orden es `started_at DESC, id` y que la ventana se aplica. Una prueba que
    /// pasara por la API no distinguiría una sentencia mal escrita de un manejador que
    /// reordena.
    #[tokio::test]
    async fn el_listado_filtrado_acota_y_ordena_por_la_fecha_descendente() {
        let (directorio, _pool, ejecuciones, tarea) = base_con_ejecucion_posible().await;

        // Tres filas: dos con la **misma** fecha —para el desempate— y una anterior.
        let empate = datetime!(2026-10-25 03:00:00 UTC);
        let anterior = empate - Duration::hours(1);

        let mut identificadores = Vec::new();

        for (estado, fecha, procesados) in [
            (Estado::Success, anterior, 100_i64),
            (Estado::Failed, empate, 200),
            (Estado::Running, empate, 300),
        ] {
            let fila = ejecuciones
                .alta(AltaDeEjecucion {
                    job_id: tarea,
                    status: estado,
                    snapshot_id: None,
                    bytes_processed: procesados,
                    bytes_uploaded: procesados / 2,
                    error_message: None,
                    started_at: fecha,
                    finished_at: None,
                })
                .await
                .expect("el alta de la ejecución no puede fallar");

            identificadores.push(fila.id);
        }

        let sin_filtros = FiltroDeEjecuciones {
            job_id: None,
            status: None,
            limite: 50,
            desplazamiento: 0,
        };

        // Sin filtros: las tres, la más reciente primero.
        let todas = ejecuciones
            .listar_filtrado(&sin_filtros)
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(
            todas.len(),
            3,
            "sin filtros tienen que salir las tres filas"
        );
        assert_eq!(
            todas[2].bytes_processed, 100,
            "la más antigua tiene que quedar la última, y ahí no hay empate que valga"
        );

        // Las dos que empatan en la fecha son las **dos primeras**, y entre ellas el orden lo
        // decide el identificador, que es aleatorio en cada ejecución. Afirmar que la de 300 va
        // primero mediría el azar del UUID y no el contrato —y así se midió: la prueba fallaba
        // siete de cada diez veces—. Lo que se afirma es el **conjunto**, que es lo que el orden
        // por fecha decide de verdad: las dos empatadas van delante de la antigua, y el orden
        // entre ellas es el que fijan las dos aserciones por identificador que vienen después.
        let mut de_las_empatadas = [todas[0].bytes_processed, todas[1].bytes_processed];
        de_las_empatadas.sort();

        assert_eq!(
            de_las_empatadas,
            [200, 300],
            "las dos filas que empatan tienen que ir delante de la más antigua"
        );

        // Las dos que empatan salen en orden de identificador, que es el desempate.
        let mut esperadas = [
            identificadores[1].hyphenated().to_string(),
            identificadores[2].hyphenated().to_string(),
        ];
        esperadas.sort();

        assert_eq!(
            todas[0].id.hyphenated().to_string(),
            esperadas[0],
            "con la misma fecha, el desempate es el identificador"
        );
        assert_eq!(
            todas[1].id.hyphenated().to_string(),
            esperadas[1],
            "y el de la segunda también"
        );

        // El filtro por estado acota.
        let solo_fallidas = ejecuciones
            .listar_filtrado(&FiltroDeEjecuciones {
                job_id: None,
                status: Some(Estado::Failed),
                limite: 50,
                desplazamiento: 0,
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(
            solo_fallidas.len(),
            1,
            "el filtro por estado tiene que acotar"
        );

        // Un `job_id` que no es el de las filas no devuelve ninguna: el filtro no se ignora.
        let de_otra_tarea = ejecuciones
            .listar_filtrado(&FiltroDeEjecuciones {
                job_id: Some(Uuid::new_v4()),
                status: None,
                limite: 50,
                desplazamiento: 0,
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert!(
            de_otra_tarea.is_empty(),
            "un filtro por una tarea sin ejecuciones no puede devolver filas"
        );

        // La ventana se aplica: saltar una devuelve la segunda del orden.
        let segunda = ejecuciones
            .listar_filtrado(&FiltroDeEjecuciones {
                job_id: None,
                status: None,
                limite: 1,
                desplazamiento: 1,
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(
            segunda.len(),
            1,
            "la ventana tiene que devolver una sola fila"
        );
        assert_eq!(
            segunda[0].id, todas[1].id,
            "saltar una tiene que dar la segunda del orden"
        );

        // La guarda del directorio temporal sigue viva hasta aquí: si se soltara antes, la base
        // desaparecería bajo las consultas.
        drop(directorio);
    }

    /// Con la tarea libre, la guarda **deja pasar** el alta y devuelve la fila.
    ///
    /// Es el caso que el disparo manual recorre cuando nadie está copiando: `POST
    /// /api/v1/jobs/:id/run` encola y contesta `202`, así que el alta tiene que ocurrir de
    /// verdad —con su `id` y su `job_id`— y no quedarse en un «podría».
    #[tokio::test]
    async fn con_la_tarea_libre_la_guarda_inserta_y_devuelve_la_fila() {
        let (_directorio, pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;

        let creada = ejecuciones
            .alta_si_no_hay_ninguna_en_curso(alta_encolada(&job_id))
            .await
            .expect("el alta no puede fallar")
            .expect("con la tarea libre la guarda tiene que dejar pasar el alta");

        assert_eq!(
            creada.job_id, job_id,
            "la fila devuelta no es de la tarea que se pidió"
        );
        assert_eq!(
            creada.status,
            Estado::Pending,
            "el estado devuelto no es el del alta"
        );

        assert_eq!(
            filas_de_la_tarea(&pool, &job_id).await,
            1,
            "el alta tiene que dejar exactamente una fila"
        );

        // Y la fila devuelta es la que quedó: misma `id` leída por el camino normal.
        let leida = ejecuciones
            .leer(&creada.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila que devolvió la guarda tiene que estar en la tabla");

        assert_eq!(
            leida.id, creada.id,
            "la fila devuelta por la guarda no es la que quedó en la tabla"
        );
    }

    /// Con una ejecución `pending` ya en la tabla, la guarda **rechaza** el alta: devuelve
    /// `None` y no crea fila.
    ///
    /// Las dos mitades se afirman por separado y no basta con una: un `None` que además
    /// hubiera insertado sería peor que un `Some`, porque el operador leería «ya está
    /// corriendo» y en la tabla habría una segunda ejecución encolada que nadie mira.
    #[tokio::test]
    async fn una_ejecucion_pending_en_curso_rechaza_el_alta_sin_crear_fila() {
        let (_directorio, pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;

        let primera = ejecuciones
            .alta(alta_encolada(&job_id))
            .await
            .expect("el alta previa no puede fallar");

        let segunda = ejecuciones
            .alta_si_no_hay_ninguna_en_curso(alta_encolada(&job_id))
            .await
            .expect("la guarda rechaza el alta **sin fallar**: `None` no es un error");

        assert!(
            segunda.is_none(),
            "con una `pending` en curso la guarda tiene que devolver `None`"
        );
        assert_eq!(
            filas_de_la_tarea(&pool, &job_id).await,
            1,
            "la guarda dijo que no, pero dejó una segunda fila en la tabla"
        );

        // La fila que queda es la de antes, no una nueva que se hubiera colado.
        let restante = ejecuciones
            .listar()
            .await
            .expect("el listado no puede fallar");

        assert_eq!(
            restante.len(),
            1,
            "tiene que quedar solo la ejecución previa"
        );
        assert_eq!(
            restante[0].id, primera.id,
            "la fila que queda no es la primera: la guarda insertó de todos modos"
        );
    }

    /// Con una ejecución `running` ya en la tabla, la guarda **rechaza** igual.
    ///
    /// Va aparte de la `pending` aunque las dos bloqueen: son los dos estados que el
    /// contrato nombra, y una guarda que sólo mirara `pending` —el estado con el que
    /// siempre entra el alta nueva— dejaría pasar una segunda copia sobre la que está
    /// corriendo, que es el caso que la guarda existe para impedir.
    #[tokio::test]
    async fn una_ejecucion_running_en_curso_rechaza_el_alta_sin_crear_fila() {
        let (_directorio, pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;

        let en_curso = ejecuciones
            .alta(AltaDeEjecucion {
                status: Estado::Running,
                ..alta_encolada(&job_id)
            })
            .await
            .expect("el alta previa no puede fallar");

        let segunda = ejecuciones
            .alta_si_no_hay_ninguna_en_curso(alta_encolada(&job_id))
            .await
            .expect("la guarda rechaza el alta **sin fallar**: `None` no es un error");

        assert!(
            segunda.is_none(),
            "con una `running` en curso la guarda tiene que devolver `None`"
        );
        assert_eq!(
            filas_de_la_tarea(&pool, &job_id).await,
            1,
            "la guarda dijo que no, pero dejó una segunda fila en la tabla"
        );
        assert_eq!(
            en_curso.status,
            Estado::Running,
            "el aparato de la prueba no dejó la ejecución en curso"
        );
    }

    /// El histórico **terminado** no bloquea: con un `success` o un `failed` de ayer, la
    /// guarda deja pasar la copia de hoy.
    ///
    /// Es la prueba que impide que un `success` de ayer deje la tarea clavada para
    /// siempre. Se recorren los **dos** estados terminales —y con dos tareas distintas,
    /// porque la primera copia que pasa deja una `pending` que bloquearía la segunda
    /// vuelta—: una guarda que sólo reconociera `success` como «terminado» dejaría un
    /// `failed` bloqueando la tarea para siempre, que es el fallo peor de los dos porque
    /// el operador no lo ve venir.
    #[tokio::test]
    async fn el_historico_terminado_no_bloquea_una_copia_nueva() {
        let (_directorio, pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;
        let otra_tarea = tarea_hermana(&pool, &job_id).await;

        for (tarea, estado) in [(&job_id, Estado::Success), (&otra_tarea, Estado::Failed)] {
            ejecuciones
                .alta(AltaDeEjecucion {
                    status: estado,
                    ..alta_ejecucion(tarea)
                })
                .await
                .expect("el alta del histórico no puede fallar");

            let creada = ejecuciones
                .alta_si_no_hay_ninguna_en_curso(alta_encolada(tarea))
                .await
                .expect("el alta no puede fallar");

            assert!(
                creada.is_some(),
                "una ejecución `{estado:?}` de ayer no puede dejar la tarea clavada para siempre"
            );
            assert_eq!(
                filas_de_la_tarea(&pool, tarea).await,
                2,
                "tienen que quedar la del histórico y la nueva"
            );
        }
    }

    /// ⚠️ La prueba que muerde: **seis altas simultáneas sobre la misma tarea dejan una
    /// sola fila**.
    ///
    /// Es lo único que mide que la guarda sea **una sola sentencia** y no un «leer y luego
    /// insertar». Con la versión de dos pasos, las seis lecturas se lanzan antes de que
    /// ninguna termine —las seis quedan en el aire a la vez y todas ven «no hay ninguna en
    /// curso»—, así que las seis insertan y la tarea acaba con seis copias encoladas: el
    /// contrato del disparo manual dice que **no** se cree una segunda ejecución.
    ///
    /// Con la sentencia única, cada conexión reevalúa el `WHERE NOT EXISTS` **dentro de su
    /// propia escritura**, y como SQLite serializa las escrituras, la que llega después ve
    /// la fila de la anterior.
    ///
    /// Seis y no dos: el pool tiene [`crate::db::MAX_CONEXIONES`] conexiones, así que seis
    /// altas garantizan que hay más peticiones que conexiones y al menos una tiene que
    /// esperar el turno —que es justo el caso que un «leer y luego insertar» no sabe
    /// resolver, porque su lectura ya ocurrió antes de la espera—.
    #[tokio::test]
    async fn seis_altas_simultaneas_dejan_una_sola_fila() {
        /// Cuántas altas simultáneas se lanzan sobre la **misma** tarea.
        const SIMULTANEAS: usize = 6;

        let (_directorio, pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;

        // Los seis futuros se construyen **antes** de esperar ninguno: así las seis
        // sentencias están vivas a la vez y la prueba mide concurrencia de verdad, no una
        // cola de altas en serie.
        let altas = (0..SIMULTANEAS)
            .map(|_| ejecuciones.alta_si_no_hay_ninguna_en_curso(alta_encolada(&job_id)));

        let mut creadas = Vec::new();

        for resultado in join_all(altas).await {
            let resultado = resultado.expect(
                "ninguna de las altas simultáneas puede fallar: un fallo de base no es «la tarea \
                 ya estaba en curso»",
            );

            if let Some(fila) = resultado {
                creadas.push(fila);
            }
        }

        assert_eq!(
            creadas.len(),
            1,
            "de {SIMULTANEAS} altas simultáneas sobre la misma tarea salieron {}: \
             la guarda no es atómica",
            creadas.len()
        );
        assert_eq!(
            filas_de_la_tarea(&pool, &job_id).await,
            1,
            "la tabla tiene que quedar con **exactamente una** fila de la tarea"
        );
    }

    /// La guarda es **por tarea**: la ejecución en curso de la de al lado no bloquea.
    ///
    /// Sin esto, una guarda que mirara la tabla entera —sin filtrar por `job_id`— pasaría
    /// las cinco pruebas anteriores y dejaría el panel con una sola copia posible a la vez
    /// en todo Kesto.
    #[tokio::test]
    async fn la_ejecucion_en_curso_de_otra_tarea_no_bloquea_el_alta() {
        let (_directorio, pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;
        let otra_tarea = tarea_hermana(&pool, &job_id).await;

        ejecuciones
            .alta(alta_encolada(&job_id))
            .await
            .expect("el alta previa no puede fallar");

        let creada = ejecuciones
            .alta_si_no_hay_ninguna_en_curso(alta_encolada(&otra_tarea))
            .await
            .expect("el alta no puede fallar");

        assert!(
            creada.is_some(),
            "la ejecución en curso de otra tarea no puede bloquear el alta de ésta"
        );
        assert_eq!(
            filas_de_la_tarea(&pool, &job_id).await,
            1,
            "la tarea que ya estaba en curso no puede llevarse una segunda fila"
        );
        assert_eq!(
            filas_de_la_tarea(&pool, &otra_tarea).await,
            1,
            "la tarea de al lado tiene que quedarse con su fila"
        );
    }

    /// Una tarea que no existe **falla**, no devuelve `None`.
    ///
    /// Es el detalle del contrato que se rompe por comodidad: convertir el fallo de la clave
    /// foránea en `None` haría que el manejador contestara `409` —«esa tarea ya tiene una
    /// ejecución en curso»— sobre una tarea que no existe, y mandaría al operador a buscar
    /// una ejecución que no hay. Aquí se fija que la huérfana sale como error de consulta,
    /// igual que en [`Ejecuciones::alta`], y que **no llega a existir**.
    #[tokio::test]
    async fn una_tarea_que_no_existe_falla_en_vez_de_devolver_none() {
        let (_directorio, pool, ejecuciones, _job_id) = base_con_ejecucion_posible().await;
        let inexistente = Uuid::new_v4();

        let error = ejecuciones
            .alta_si_no_hay_ninguna_en_curso(alta_encolada(&inexistente))
            .await
            .expect_err("una tarea inexistente tiene que fallar, no decir que ya estaba en curso");

        assert!(
            matches!(error, DbError::Consulta { .. }),
            "el fallo tiene que llegar como error de consulta, no como otro: {error}"
        );
        assert_eq!(
            filas_de_la_tarea(&pool, &inexistente).await,
            0,
            "una ejecución huérfana no puede llegar a existir"
        );
    }

    /// **El objetivo de una instantánea sale del vínculo de su ejecución, y sin él no hay nada.**
    ///
    /// Es la consulta que sostiene la restauración entera: el único camino de una instantánea a su
    /// target es la ejecución que la anotó. Se afirma el objetivo **entero** —tarea, target, tipo,
    /// ruta y nombre—, porque cada campo viaja a la respuesta o decide si se puede restaurar, y uno
    /// que faltara dejaría la restauración sin destino.
    ///
    /// Y se afirma el `None` en los dos casos en que **no** hay vínculo: sin ninguna ejecución que
    /// anote esa instantánea —una copia hecha con otra herramienta— y con la ejecución anotando
    /// otra. Y el tercero, que es el que la firma lleva por el `repository_id`: la misma
    /// instantánea anotada por una tarea de **otro** repositorio no resuelve, porque la pregunta es
    /// por un repositorio concreto. `None` en los tres y no un error: «no hay vínculo» es un
    /// rechazo con su código, y un error aquí lo convertiría en un fallo del servidor.
    #[tokio::test]
    async fn el_objetivo_de_una_instantanea_sale_del_vinculo_de_su_ejecucion() {
        let (_directorio, pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;
        let tarea = Tareas::nuevo(pool.clone())
            .leer(&job_id)
            .await
            .expect("la lectura de la tarea no puede fallar")
            .expect("la tarea de la base de prueba tiene que estar");

        // Sin ninguna ejecución todavía: la instantánea existe para quien la mire en el
        // repositorio, pero aquí no hay a qué target apuntarla.
        assert!(
            ejecuciones
                .objetivo_de_la_instantanea(SNAPSHOT, &tarea.repository_id)
                .await
                .expect("la consulta no puede fallar")
                .is_none(),
            "sin ninguna ejecución que anote la instantánea no hay vínculo que resolver"
        );

        ejecuciones
            .alta(alta_ejecucion(&job_id))
            .await
            .expect("el alta de la ejecución no puede fallar");

        let objetivo = ejecuciones
            .objetivo_de_la_instantanea(SNAPSHOT, &tarea.repository_id)
            .await
            .expect("la consulta no puede fallar")
            .expect("la ejecución anotó la instantánea, así que el vínculo existe");

        assert_eq!(objetivo.job_id, job_id, "la tarea no es la de la ejecución");
        assert_eq!(
            objetivo.target_id, tarea.target_id,
            "el target no es el de la tarea de la ejecución"
        );
        assert_eq!(objetivo.target_type, TipoDeTarget::Directory);
        assert_eq!(
            objetivo.host_path.as_deref(),
            Some("/srv/target-de-ejecucion")
        );
        assert_eq!(objetivo.target_name, "target-de-ejecucion");

        assert!(
            ejecuciones
                .objetivo_de_la_instantanea("otra-instantanea", &tarea.repository_id)
                .await
                .expect("la consulta no puede fallar")
                .is_none(),
            "una instantánea que ninguna ejecución anotó no tiene vínculo"
        );

        assert!(
            ejecuciones
                .objetivo_de_la_instantanea(SNAPSHOT, &Uuid::new_v4())
                .await
                .expect("la consulta no puede fallar")
                .is_none(),
            "el vínculo se acota al repositorio por el que se pregunta: la copia de otro \
             repositorio no resuelve aquí"
        );
    }

    /// **El lote resuelve el target de cada instantánea, y la que no tiene vínculo no es un fallo.**
    ///
    /// Es la hermana por lotes de la consulta anterior, y mide lo que aquélla no puede: que varias
    /// instantáneas se resuelvan de **una** vez y cada una con **su** target —un lote no puede
    /// devolver el de la primera para todas—, que lo que no tiene vínculo **falte** del mapa sin
    /// arrastrar al resto, y que el «gana la más reciente» siga decidiendo dentro del lote.
    ///
    /// El «más reciente» se discrimina por el `job_id` del objetivo y no por el target: las dos
    /// ejecuciones de la misma instantánea son de tareas **hermanas**, que apuntan al mismo target,
    /// así que el target no distingue nada; la tarea que anotó cada copia, sí.
    ///
    /// Y se afirman los dos casos extremos del contrato: un lote de un identificador que no casa con
    /// ninguna fila —el mapa sale vacío, igual que el `Ok(None)` de la hermana— y un lote **vacío**,
    /// que no consulta y devuelve un mapa vacío en vez de fallar.
    #[tokio::test]
    async fn los_objetivos_de_un_lote_salen_del_vinculo_de_cada_ejecucion() {
        /// La instantánea que **no** casa con ninguna ejecución.
        const SIN_VINCULO: &str = "instantanea-que-nadie-anoto";

        let (_directorio, pool, ejecuciones, job_id) = base_con_ejecucion_posible().await;
        let hermana = tarea_hermana(&pool, &job_id).await;
        let tarea = Tareas::nuevo(pool.clone())
            .leer(&job_id)
            .await
            .expect("la lectura de la tarea no puede fallar")
            .expect("la tarea de la base de prueba tiene que estar");

        // La misma instantánea anotada por **dos** ejecuciones de tareas distintas, y una segunda
        // instantánea anotada por una sola: el lote tiene que resolver las dos primeras y quedarse,
        // para la repetida, con la de la ejecución más reciente.
        ejecuciones
            .alta(alta_con(
                SNAPSHOT,
                &job_id,
                datetime!(2026-01-01 10:00:00 UTC),
            ))
            .await
            .expect("el alta de la ejecución no puede fallar");
        ejecuciones
            .alta(alta_con(
                SNAPSHOT,
                &hermana,
                datetime!(2026-01-02 10:00:00 UTC),
            ))
            .await
            .expect("el alta de la ejecución hermana no puede fallar");
        ejecuciones
            .alta(alta_con(
                OTRA_INSTANTANEA,
                &job_id,
                datetime!(2026-01-03 10:00:00 UTC),
            ))
            .await
            .expect("el alta de la ejecución de la otra instantánea no puede fallar");

        let objetivos = ejecuciones
            .objetivos_de_las_instantaneas(
                &[
                    SNAPSHOT.to_owned(),
                    OTRA_INSTANTANEA.to_owned(),
                    SIN_VINCULO.to_owned(),
                ],
                &tarea.repository_id,
            )
            .await
            .expect("la consulta del lote no puede fallar");

        assert_eq!(
            objetivos.len(),
            2,
            "el lote tiene que traer las dos instantáneas con vínculo y ninguna más: {objetivos:?}"
        );

        let con_vinculo = objetivos
            .get(SNAPSHOT)
            .expect("la instantánea anotada por dos ejecuciones tiene que salir en el lote");

        assert_eq!(
            con_vinculo.job_id, hermana,
            "de dos ejecuciones de la misma instantánea gana la más reciente, y ganó la primera"
        );
        assert_eq!(
            con_vinculo.target_id, tarea.target_id,
            "el target no es el de la tarea de la ejecución"
        );
        assert_eq!(con_vinculo.target_type, TipoDeTarget::Directory);
        assert_eq!(
            con_vinculo.host_path.as_deref(),
            Some("/srv/target-de-ejecucion")
        );
        assert_eq!(con_vinculo.target_name, "target-de-ejecucion");

        let la_otra = objetivos
            .get(OTRA_INSTANTANEA)
            .expect("la otra instantánea con vínculo tiene que salir con **su** objetivo");

        assert_eq!(
            la_otra.job_id, job_id,
            "el lote tiene que casar cada instantánea con su propia ejecución, y no devolver el \
             objetivo de la primera para todas"
        );

        assert!(
            !objetivos.contains_key(SIN_VINCULO),
            "una instantánea que ninguna ejecución anotó no tiene vínculo y no puede salir en el \
             mapa: {objetivos:?}"
        );

        let de_otro_repositorio = ejecuciones
            .objetivos_de_las_instantaneas(&[SNAPSHOT.to_owned()], &Uuid::new_v4())
            .await
            .expect("la consulta del lote no puede fallar");

        assert!(
            de_otro_repositorio.is_empty(),
            "el lote se acota al repositorio por el que se pregunta: la copia de otro repositorio \
             no resuelve aquí"
        );

        let vacio = ejecuciones
            .objetivos_de_las_instantaneas(&[], &tarea.repository_id)
            .await
            .expect("un lote vacío no puede fallar");

        assert!(
            vacio.is_empty(),
            "un lote sin identificadores no tiene nada que resolver y tiene que salir vacío"
        );
    }
}
