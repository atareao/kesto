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

use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteRow;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::db::DbError;

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

/// Alta de una ejecución, devolviendo la fila tal y como quedó.
///
/// Si el `job_id` no existe, la clave foránea rechaza la fila: una ejecución huérfana no
/// llega a existir.
const SQL_ALTA: &str = "INSERT INTO job_executions \
     (id, job_id, status, snapshot_id, bytes_processed, bytes_uploaded, error_message, started_at, \
     finished_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
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
            .bind(alta.started_at)
            .bind(alta.finished_at)
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

    /// Actualiza la fila **entera** y devuelve si existía.
    pub async fn actualizar(&self, ejecucion: &Ejecucion) -> Result<bool, DbError> {
        let resultado = sqlx::query(SQL_ACTUALIZAR)
            .bind(ejecucion.status)
            .bind(&ejecucion.snapshot_id)
            .bind(ejecucion.bytes_processed)
            .bind(ejecucion.bytes_uploaded)
            .bind(&ejecucion.error_message)
            .bind(ejecucion.finished_at)
            .bind(ejecucion.id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ACTUALIZAR))?;

        Ok(resultado.rows_affected() == 1)
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
fn error_de_consulta(consulta: &str) -> impl FnOnce(sqlx::Error) -> DbError + '_ {
    move |causa| DbError::Consulta {
        consulta: consulta.to_owned(),
        causa,
    }
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
            consulta: CONSULTA_MAPEO.to_owned(),
            causa,
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
        consulta: CONSULTA_MAPEO.to_owned(),
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    let job_id = columna::<String>(fila, "job_id")?;
    let job_id = Uuid::parse_str(&job_id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO.to_owned(),
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
    use sqlx::SqlitePool;
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    use super::{AltaDeEjecucion, Ejecuciones, Estado};
    use crate::db::entities::job::{AltaDeTarea, Tareas};
    use crate::db::entities::repository::{AltaDeRepositorio, Repositorios};
    use crate::db::entities::target::{AltaDeTarget, Targets, TipoDeTarget};
    use crate::secret::SecretString;
    use crate::test_support::{DirectorioDePrueba, base_con_clave};

    /// Identificador del `snapshot` de las pruebas: distinguible y largo.
    const SNAPSHOT: &str = "snapshot-distinguible-000000000000000000000001";

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
}
