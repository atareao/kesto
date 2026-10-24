//! La tabla `backup_jobs` del §3: la entidad, el alta y la capa de acceso.
//!
//! Es la tabla que une un target con un repositorio, y la que lleva la retención
//! —`keep_daily`, `keep_weekly`, `keep_monthly`— y el interruptor `enabled`. Las dos
//! claves foráneas son `NOT NULL` y con `ON DELETE CASCADE`: una tarea huérfana no es
//! un estado posible, y borrar su target o su repositorio se la lleva por delante.
//!
//! **No tiene ninguna columna de secreto**, así que su capa de acceso **no** recibe la
//! clave de cifrado: pasarle una que no se usa sugeriría que aquí se cifra algo.

use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteRow;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::db::DbError;

/// Fila de `backup_jobs`.
#[derive(Debug, Clone)]
pub struct Tarea {
    /// UUIDv4 en forma canónica con guiones.
    pub id: Uuid,
    /// Target al que pertenece.
    pub target_id: Uuid,
    /// Repositorio donde se guardan sus copias.
    pub repository_id: Uuid,
    /// Expresión cron con la que se agenda.
    pub cron_expression: String,
    /// Copias diarias que se retienen.
    pub keep_daily: i64,
    /// Copias semanales que se retienen.
    pub keep_weekly: i64,
    /// Copias mensuales que se retienen.
    pub keep_monthly: i64,
    /// Si la tarea está activa.
    pub enabled: bool,
    /// Fecha de alta, en UTC.
    pub created_at: OffsetDateTime,
}

/// Los datos de un alta. **Sin `id`**: lo genera la capa de acceso.
#[derive(Debug, Clone)]
pub struct AltaDeTarea {
    /// Target al que pertenece.
    pub target_id: Uuid,
    /// Repositorio donde se guardan sus copias.
    pub repository_id: Uuid,
    /// Expresión cron con la que se agenda.
    pub cron_expression: String,
    /// Copias diarias que se retienen.
    pub keep_daily: i64,
    /// Copias semanales que se retienen.
    pub keep_weekly: i64,
    /// Copias mensuales que se retienen.
    pub keep_monthly: i64,
    /// Si la tarea está activa.
    pub enabled: bool,
}

/// La capa de acceso de `backup_jobs`.
pub struct Tareas {
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
const SQL_LEER: &str = "SELECT id, target_id, repository_id, cron_expression, keep_daily, keep_weekly, \
     keep_monthly, enabled, created_at FROM backup_jobs WHERE id = ?1";

/// Listado de todas las tareas.
///
/// `ORDER BY created_at, id` y **no** `created_at` a secas: las filas dadas de alta en
/// el mismo segundo empatan —`CURRENT_TIMESTAMP` no tiene más resolución— y el orden
/// entre dos llamadas seguidas dejaría de ser estable. El `id` es el desempate.
const SQL_LISTAR: &str = "SELECT id, target_id, repository_id, cron_expression, keep_daily, keep_weekly, \
     keep_monthly, enabled, created_at FROM backup_jobs ORDER BY created_at, id";

/// Alta de una tarea, devolviendo la fila tal y como quedó.
///
/// Si el `target_id` o el `repository_id` no existen, la clave foránea rechaza la fila:
/// una tarea huérfana no llega a existir.
const SQL_ALTA: &str = "INSERT INTO backup_jobs \
     (id, target_id, repository_id, cron_expression, keep_daily, keep_weekly, keep_monthly, enabled) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
     RETURNING id, target_id, repository_id, cron_expression, keep_daily, keep_weekly, keep_monthly, \
     enabled, created_at";

/// Actualización de la fila entera.
///
/// `created_at` **no** entra: es la fecha de nacimiento de la fila, no un campo que se
/// edite —reescribirla cambiaría la historia y el orden del listado—.
const SQL_ACTUALIZAR: &str = "UPDATE backup_jobs SET target_id = ?1, repository_id = ?2, \
     cron_expression = ?3, keep_daily = ?4, keep_weekly = ?5, keep_monthly = ?6, enabled = ?7 \
     WHERE id = ?8";

/// Borrado por identificador.
const SQL_BORRAR: &str = "DELETE FROM backup_jobs WHERE id = ?1";

/// Texto del error cuando falla el **mapeo** de una fila ya traída.
///
/// No es una sentencia ejecutada —la fila ya llegó—, así que no se confunde con las
/// `SQL_*`: dice dónde falló, que es leer una columna. Es lo que el registro necesita
/// saber cuando el fallo no viene del motor.
const CONSULTA_MAPEO: &str = "mapeo de la fila de backup_jobs";

impl Tareas {
    /// Construye la capa de acceso sobre un pool.
    ///
    /// **Sin clave de cifrado**: `backup_jobs` no tiene ninguna columna de secreto, y
    /// pedir una que no se usa sugeriría que aquí se cifra algo.
    pub fn nuevo(pool: SqlitePool) -> Self {
        Tareas { pool }
    }

    /// Da de alta una tarea y devuelve la fila tal y como quedó.
    ///
    /// Una tarea que apunta a un target o a un repositorio que no existen se rechaza
    /// con error: la clave foránea del §3 no admite huérfanas.
    pub async fn alta(&self, alta: AltaDeTarea) -> Result<Tarea, DbError> {
        // El identificador se genera **aquí**, no en la base: SQLite no tiene `uuid()`
        // y un valor por defecto en el DDL obligaría a interpolarlo en la sentencia.
        let id = Uuid::new_v4();

        let fila = sqlx::query(SQL_ALTA)
            // El `id` y las dos claves foráneas van como texto con guiones: un `bind` de
            // `Uuid` los escribiría como un BLOB de 16 bytes y el §3 dice `TEXT`.
            .bind(id.hyphenated().to_string())
            .bind(alta.target_id.hyphenated().to_string())
            .bind(alta.repository_id.hyphenated().to_string())
            .bind(&alta.cron_expression)
            .bind(alta.keep_daily)
            .bind(alta.keep_weekly)
            .bind(alta.keep_monthly)
            .bind(alta.enabled)
            .fetch_one(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ALTA))?;

        desde_fila(&fila)
    }

    /// Lee una tarea por su identificador: `Ok(None)` si no está.
    ///
    /// «No está» y «no se pudo leer» son cosas distintas: lo primero es `Ok(None)` y lo
    /// segundo, un error. Confundirlos convertiría una base caída en «no existe», que
    /// es lo que hace que un fallo se persiga en el sitio equivocado.
    pub async fn leer(&self, id: &Uuid) -> Result<Option<Tarea>, DbError> {
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

    /// Lista las tareas con un orden **estable**.
    pub async fn listar(&self) -> Result<Vec<Tarea>, DbError> {
        let filas = sqlx::query(SQL_LISTAR)
            .fetch_all(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_LISTAR))?;

        filas.iter().map(desde_fila).collect()
    }

    /// Actualiza la fila **entera** y devuelve si existía.
    pub async fn actualizar(&self, tarea: &Tarea) -> Result<bool, DbError> {
        let resultado = sqlx::query(SQL_ACTUALIZAR)
            .bind(tarea.target_id.hyphenated().to_string())
            .bind(tarea.repository_id.hyphenated().to_string())
            .bind(&tarea.cron_expression)
            .bind(tarea.keep_daily)
            .bind(tarea.keep_weekly)
            .bind(tarea.keep_monthly)
            .bind(tarea.enabled)
            .bind(tarea.id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ACTUALIZAR))?;

        Ok(resultado.rows_affected() == 1)
    }

    /// Borra la fila y devuelve si existía.
    ///
    /// El `ON DELETE CASCADE` del §3 arrastra sus `job_executions`: el borrado en
    /// cascada lo hace la base, no esta capa.
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

/// Mapea una fila de `backup_jobs` a la entidad.
///
/// No descifra nada: esta tabla no tiene secretos. `enabled` se lee como `bool` —la
/// columna guarda `1` y `0`, que es lo que el §3 declara como `BOOLEAN`.
fn desde_fila(fila: &SqliteRow) -> Result<Tarea, DbError> {
    // El `id` viaja como `TEXT` con guiones, así que la conversión a `Uuid` es del
    // mapeo y no de `sqlx`. Su error **no** es un `sqlx::Error`, y envolverlo en
    // `sqlx::Error::Decode` es lo que lo hace viajar por `DbError::Consulta` con la
    // causa intacta, sin estrenar una variante para un fallo que ya tiene sitio.
    let id = columna::<String>(fila, "id")?;
    let id = Uuid::parse_str(&id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO.to_owned(),
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    let target_id = columna::<String>(fila, "target_id")?;
    let target_id = Uuid::parse_str(&target_id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO.to_owned(),
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    let repository_id = columna::<String>(fila, "repository_id")?;
    let repository_id = Uuid::parse_str(&repository_id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO.to_owned(),
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    Ok(Tarea {
        id,
        target_id,
        repository_id,
        cron_expression: columna(fila, "cron_expression")?,
        keep_daily: columna(fila, "keep_daily")?,
        keep_weekly: columna(fila, "keep_weekly")?,
        keep_monthly: columna(fila, "keep_monthly")?,
        enabled: columna(fila, "enabled")?,
        created_at: columna(fila, "created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::{AltaDeTarea, Tareas};
    use crate::db::DbError;
    use crate::db::entities::execution::{AltaDeEjecucion, Ejecuciones, Estado};
    use crate::db::entities::repository::{AltaDeRepositorio, Repositorios};
    use crate::db::entities::target::{AltaDeTarget, Targets, TipoDeTarget};
    use crate::secret::SecretString;
    use crate::test_support::{DirectorioDePrueba, base_con_clave};

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
            secret_access_key: secreto("AKIAIOSFODNN7EXAMPLE-de-tarea"),
            passphrase: secreto("passphrase-distinguible-de-tarea"),
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

    /// Un alta de tarea con los valores del §3 por defecto y `enabled` informado.
    fn alta_tarea(target_id: &Uuid, repository_id: &Uuid, cron: &str) -> AltaDeTarea {
        AltaDeTarea {
            target_id: *target_id,
            repository_id: *repository_id,
            cron_expression: cron.to_owned(),
            keep_daily: 7,
            keep_weekly: 4,
            keep_monthly: 12,
            enabled: true,
        }
    }

    /// Un pool migrado con un repositorio y un target ya dados de alta: lo que
    /// `backup_jobs` necesita para no ser huérfana.
    async fn base_con_tarea_posible() -> (DirectorioDePrueba, SqlitePool, Tareas, Uuid, Uuid) {
        let (directorio, pool, clave) = base_con_clave().await;

        let repositorio = Repositorios::nuevo(pool.clone(), clave.clone())
            .alta(alta_repositorio("repo-de-tarea"))
            .await
            .expect("el alta del repositorio no puede fallar");

        let target = Targets::nuevo(pool.clone(), clave)
            .alta(alta_target("target-de-tarea"))
            .await
            .expect("el alta del target no puede fallar");

        let tareas = Tareas::nuevo(pool.clone());

        (directorio, pool, tareas, target.id, repositorio.id)
    }

    /// 1.28 — El ciclo completo de una tarea, con sus tres `keep_*` y su `enabled`.
    #[tokio::test]
    async fn el_ciclo_de_una_tarea_con_su_retencion_y_su_interruptor() {
        let (_directorio, _pool, tareas, target_id, repository_id) = base_con_tarea_posible().await;

        let dado_de_alta = tareas
            .alta(alta_tarea(&target_id, &repository_id, "0 3 * * *"))
            .await
            .expect("el alta no puede fallar");

        let leido = tareas
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila recién dada de alta debe estar");

        assert_eq!(leido.id, dado_de_alta.id, "el id leído no es el del alta");
        assert_eq!(leido.target_id, target_id);
        assert_eq!(leido.repository_id, repository_id);
        assert_eq!(leido.cron_expression, "0 3 * * *");
        assert_eq!(leido.keep_daily, 7);
        assert_eq!(leido.keep_weekly, 4);
        assert_eq!(leido.keep_monthly, 12);
        assert!(leido.enabled, "la tarea se dio de alta activa");
        assert_eq!(leido.created_at, dado_de_alta.created_at);

        let listado = tareas.listar().await.expect("el listado no puede fallar");
        assert_eq!(listado.len(), 1, "el listado debe traer la tarea");

        let mut cambiado = leido.clone();
        cambiado.cron_expression = "0 5 * * *".to_owned();
        cambiado.keep_daily = 14;
        cambiado.enabled = false;

        assert!(
            tareas
                .actualizar(&cambiado)
                .await
                .expect("la actualización no puede fallar"),
            "la fila existía, así que la actualización debe decir que sí"
        );

        let releido = tareas
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe seguir existiendo");

        assert_eq!(
            releido.cron_expression, "0 5 * * *",
            "el campo tocado no cambió"
        );
        assert_eq!(releido.keep_daily, 14, "la retención no cambió");
        assert!(!releido.enabled, "el interruptor no cambió");
        assert_eq!(
            releido.created_at, dado_de_alta.created_at,
            "el created_at no debía cambiar"
        );

        assert!(
            tareas
                .borrar(&dado_de_alta.id)
                .await
                .expect("el borrado no puede fallar"),
            "la fila existía, así que el borrado debe decir que sí"
        );

        let tras_borrar = tareas
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar");

        assert!(tras_borrar.is_none(), "la fila debía desaparecer");
    }

    /// 1.29 — `enabled` se almacena como el entero `1` cuando es verdadero y `0` cuando
    /// es falso, y se lee como `bool`.
    #[tokio::test]
    async fn el_enabled_se_guarda_como_uno_o_cero() {
        let (_directorio, pool, tareas, target_id, repository_id) = base_con_tarea_posible().await;

        let activa = tareas
            .alta(alta_tarea(&target_id, &repository_id, "0 3 * * *"))
            .await
            .expect("el alta no puede fallar");

        let inactiva = tareas
            .alta(AltaDeTarea {
                enabled: false,
                ..alta_tarea(&target_id, &repository_id, "0 4 * * *")
            })
            .await
            .expect("el alta no puede fallar");

        let activa_cruda: i64 = sqlx::query_scalar("SELECT enabled FROM backup_jobs WHERE id = ?1")
            .bind(activa.id.hyphenated().to_string())
            .fetch_one(&pool)
            .await
            .expect("el interruptor debe poder leerse en crudo");

        assert_eq!(activa_cruda, 1, "una tarea activa debe guardar un 1");

        let inactiva_cruda: i64 =
            sqlx::query_scalar("SELECT enabled FROM backup_jobs WHERE id = ?1")
                .bind(inactiva.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el interruptor debe poder leerse en crudo");

        assert_eq!(inactiva_cruda, 0, "una tarea inactiva debe guardar un 0");

        assert!(
            tareas
                .leer(&activa.id)
                .await
                .expect("la lectura no puede fallar")
                .expect("la fila debe estar")
                .enabled,
            "el 1 almacenado debe leerse como `true`"
        );

        assert!(
            !tareas
                .leer(&inactiva.id)
                .await
                .expect("la lectura no puede fallar")
                .expect("la fila debe estar")
                .enabled,
            "el 0 almacenado debe leerse como `false`"
        );
    }

    /// 1.34 — Una inserción **huérfana** se rechaza con error tipado y **no deja fila**.
    ///
    /// Las dos claves foráneas del §3 son `NOT NULL` y sin excepción: ni una tarea sin
    /// target, ni una ejecución sin tarea llegan a existir. La comprobación con SQL
    /// directo es la que demuestra que no ha quedado ni una fila «a medias».
    #[tokio::test]
    async fn una_insercion_huerfana_se_rechaza_y_no_deja_fila() {
        let (_directorio, pool, tareas, _target_id, repository_id) = base_con_tarea_posible().await;

        let inexistente = Uuid::new_v4();

        // Una tarea que apunta a un target que no existe.
        let error = tareas
            .alta(alta_tarea(&inexistente, &repository_id, "0 3 * * *"))
            .await
            .expect_err("un target inexistente no puede dar una tarea");

        assert!(
            matches!(error, DbError::Consulta { .. }),
            "el fallo debe llegar como error tipado: {error}"
        );

        // Y una ejecución que apunta a una tarea que no existe. La capa solo necesita
        // el pool, así que se construye aquí a partir del mismo.
        let ejecuciones = Ejecuciones::nuevo(pool.clone());

        let error = ejecuciones
            .alta(AltaDeEjecucion {
                job_id: inexistente,
                status: Estado::Pending,
                snapshot_id: None,
                bytes_processed: 0,
                bytes_uploaded: 0,
                error_message: None,
                started_at: OffsetDateTime::now_utc(),
                finished_at: None,
            })
            .await
            .expect_err("una tarea inexistente no puede dar una ejecución");

        assert!(
            matches!(error, DbError::Consulta { .. }),
            "el fallo debe llegar como error tipado: {error}"
        );

        let tareas_que_quedaron: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect("el recuento de tareas no puede fallar");

        assert_eq!(
            tareas_que_quedaron, 0,
            "la inserción huérfana dejó una tarea"
        );

        let ejecuciones_que_quedaron: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM job_executions")
                .fetch_one(&pool)
                .await
                .expect("el recuento de ejecuciones no puede fallar");

        assert_eq!(
            ejecuciones_que_quedaron, 0,
            "la inserción huérfana dejó una ejecución"
        );
    }
}
