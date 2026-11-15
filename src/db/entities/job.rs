//! La tabla `backup_jobs` del §3: la entidad, el alta y la capa de acceso.
//!
//! Es la tabla que une un target con un repositorio, y la que lleva la retención
//! —`keep_daily`, `keep_weekly`, `keep_monthly`— y el interruptor `enabled`. Las dos
//! claves foráneas son `NOT NULL` y con `ON DELETE CASCADE`: una tarea huérfana no es
//! un estado posible, y borrar su target o su repositorio se la lleva por delante.
//!
//! **No tiene ninguna columna de secreto**, así que su capa de acceso **no** recibe la
//! clave de cifrado: pasarle una que no se usa sugeriría que aquí se cifra algo.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

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
    /// Nombre legible de la tarea; `None` si no tiene.
    ///
    /// Es la columna que añade la `0005` (issue #65) y la única anulable de la tabla: el nombre es
    /// opcional por contrato —una tarea sin él sigue siendo válida, y el panel cae al
    /// identificador—, así que `None` es un estado del contrato y no un dato que falte. Nunca es
    /// `Some("")`: un nombre en blanco lo guarda el alta como `NULL`, y `''` y `NULL` no se pintan
    /// igual en el panel.
    pub name: Option<String>,
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
    /// Nombre legible de la tarea; `None` si se da de alta sin nombre.
    ///
    /// Llega **ya recortado y en blanco convertido a `None`**: el recorte y el paso del blanco a
    /// `None` son del borde —el alta y la edición comparten ese validador—, porque una cadena vacía
    /// en la columna fingiría un nombre que nadie escribió. Aquí no se vuelve a mirar.
    pub name: Option<String>,
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
///
/// `name` va la **última** porque es donde la deja el `ALTER TABLE ... ADD COLUMN` de la `0005`, y
/// el orden es el que publica `PRAGMA table_info`: ponerla en otro sitio aquí haría que la consulta
/// y el esquema se leyeran en dos órdenes distintos sin que nada lo delatara.
const SQL_LEER: &str = "SELECT id, target_id, repository_id, cron_expression, keep_daily, keep_weekly, \
     keep_monthly, enabled, created_at, name FROM backup_jobs WHERE id = ?1";

/// Listado de todas las tareas.
///
/// `ORDER BY created_at, id` y **no** `created_at` a secas: las filas dadas de alta en
/// el mismo segundo empatan —`CURRENT_TIMESTAMP` no tiene más resolución— y el orden
/// entre dos llamadas seguidas dejaría de ser estable. El `id` es el desempate.
const SQL_LISTAR: &str = "SELECT id, target_id, repository_id, cron_expression, keep_daily, keep_weekly, \
     keep_monthly, enabled, created_at, name FROM backup_jobs ORDER BY created_at, id";

/// Alta de una tarea, devolviendo la fila tal y como quedó.
///
/// Si el `target_id` o el `repository_id` no existen, la clave foránea rechaza la fila:
/// una tarea huérfana no llega a existir.
const SQL_ALTA: &str = "INSERT INTO backup_jobs \
     (id, target_id, repository_id, cron_expression, keep_daily, keep_weekly, keep_monthly, enabled, name) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
     RETURNING id, target_id, repository_id, cron_expression, keep_daily, keep_weekly, keep_monthly, \
     enabled, created_at, name";

/// Actualización de la fila entera.
///
/// `created_at` **no** entra: es la fecha de nacimiento de la fila, no un campo que se
/// edite —reescribirla cambiaría la historia y el orden del listado—. `name` sí: es un campo del
/// formulario que se puede corregir o borrar, y editar una tarea sin poder quitar su nombre
/// dejaría una errata sin arreglo.
const SQL_ACTUALIZAR: &str = "UPDATE backup_jobs SET target_id = ?1, repository_id = ?2, \
     cron_expression = ?3, keep_daily = ?4, keep_weekly = ?5, keep_monthly = ?6, enabled = ?7, \
     name = ?8 WHERE id = ?9";

/// Los nombres de **varias** tareas, resueltos en **una sola** lectura.
///
/// La usan los otros dos listados que publican el nombre de la tarea de cada fila —el del histórico
/// y el de instantáneas—, y por eso es por lotes: la página del histórico llega a 200 filas, y
/// resolverlas de una en una serían 200 consultas para pintar una pantalla.
///
/// La lista de identificadores **es un valor enlazado**: viaja como un array JSON en `?1` y
/// `json_each` lo abre en filas dentro del propio SQLite, así que la sentencia es una constante y
/// ningún identificador entra en su texto. Es la misma forma que la hermana por lotes de los
/// objetivos de las instantáneas —`objetivos_de_las_instantaneas`, en `execution.rs`—, y por el
/// mismo motivo: un `IN` con un marcador por identificador obligaría a **armar** la sentencia en
/// piezas, y aquí la sentencia que se ejecuta y la que viaja en el error tienen que ser la misma
/// constante.
///
/// No lleva `ORDER BY`: el resultado se recoge en un mapa por identificador, y el orden de un mapa
/// no depende del de las filas.
const SQL_NOMBRES: &str = "SELECT id, name FROM backup_jobs \
     WHERE id IN (SELECT value FROM json_each(?1))";

/// Borrado por identificador.
const SQL_BORRAR: &str = "DELETE FROM backup_jobs WHERE id = ?1";

/// Texto del error cuando falla el **mapeo** de una fila ya traída.
///
/// No es una sentencia ejecutada —la fila ya llegó—, así que no se confunde con las
/// `SQL_*`: dice dónde falló, que es leer una columna. Es lo que el registro necesita
/// saber cuando el fallo no viene del motor.
const CONSULTA_MAPEO: &str = "mapeo de la fila de backup_jobs";

/// Texto del error cuando lo que falla es **armar la lista** de identificadores que viaja enlazada.
///
/// La lista se arma antes de consultar, así que no es una sentencia ejecutada y no se confunde con
/// las `SQL_*`: nombra el paso que falló, que es codificar los identificadores para que viajen.
const CONSULTA_LISTA_DE_TAREAS: &str = "lista de identificadores de tareas";

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
            .bind(&alta.name)
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
            .bind(&tarea.name)
            .bind(tarea.id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ACTUALIZAR))?;

        Ok(resultado.rows_affected() == 1)
    }

    /// Los nombres de varias tareas, resueltos en **una sola** lectura.
    ///
    /// Es la mitad que los listados del histórico y de instantáneas necesitan para publicar el
    /// nombre de la tarea de cada fila sin pagar una consulta por fila: quien llama reúne los
    /// `job_id` de su página y recibe aquí el nombre de cada una de golpe.
    ///
    /// **Una tarea sin nombre sale en el mapa con `None`**, y una tarea que no exista —o una fila sin
    /// vínculo, que no llega a aportar identificador— **no sale**: el mapa devuelve `None` para las
    /// dos cuando lo consulta quien llama, y eso es lo correcto, porque el contrato publica `null`
    /// tanto si la tarea no tiene nombre como si la ejecución no casa con ninguna. Lo que el mapa
    /// **no** hace es inventar entradas de relleno para identificadores que no pidió nadie.
    ///
    /// Las claves son los `Uuid`, no sus formas en texto: quien consulta tiene el identificador ya
    /// convertido —lo sacó de una fila de la capa de acceso— y pasarle una segunda forma que tuviera
    /// que normalizar sería devolverle el problema que esta capa ya resolvió.
    ///
    /// # Errors
    ///
    /// Un [`DbError::Consulta`] si la lista no se puede codificar o la sentencia falla.
    pub async fn nombres(&self, ids: &[Uuid]) -> Result<HashMap<Uuid, Option<String>>, DbError> {
        // Sin identificadores no hay nada que resolver: la consulta sobra, y un listado de cero
        // filas no tiene por qué costar una ida y vuelta a la base.
        if ids.is_empty() {
            return Ok(HashMap::new());
        }

        // El array lo arma `serde_json`, que es quien sabe escapar: ningún identificador se pega al
        // texto de la consulta. El fallo viaja como `sqlx::Error::Encode` —y no estrena una
        // variante— porque es exactamente lo que significa: un valor que no se pudo codificar para
        // viajar a la base.
        let identificadores: Vec<String> =
            ids.iter().map(|id| id.hyphenated().to_string()).collect();

        let lista = serde_json::to_string(&identificadores).map_err(|causa| DbError::Consulta {
            consulta: CONSULTA_LISTA_DE_TAREAS,
            causa: sqlx::Error::Encode(Box::new(causa)),
        })?;

        let filas = sqlx::query(SQL_NOMBRES)
            .bind(lista)
            .fetch_all(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_NOMBRES))?;

        let mut nombres = HashMap::with_capacity(ids.len());

        for fila in &filas {
            // El `IN` no casa nunca con `NULL`, así que una fila que ha llegado hasta aquí tiene su
            // `id` escrito: leerlo no puede degenerar en una clave vacía.
            let id = columna::<String>(fila, "id")?;
            let id = Uuid::parse_str(&id).map_err(|causa| DbError::Consulta {
                consulta: CONSULTA_MAPEO,
                causa: sqlx::Error::Decode(Box::new(causa)),
            })?;

            if let Entry::Vacant(hueco) = nombres.entry(id) {
                hueco.insert(columna::<Option<String>>(fila, "name")?);
            }
        }

        Ok(nombres)
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
fn error_de_consulta(consulta: &'static str) -> impl FnOnce(sqlx::Error) -> DbError {
    move |causa| DbError::Consulta { consulta, causa }
}

/// Lee una columna de la fila, conservando la causa si falta o no tiene el tipo.
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
        consulta: CONSULTA_MAPEO,
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    let target_id = columna::<String>(fila, "target_id")?;
    let target_id = Uuid::parse_str(&target_id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO,
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    let repository_id = columna::<String>(fila, "repository_id")?;
    let repository_id = Uuid::parse_str(&repository_id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO,
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
        name: columna(fila, "name")?,
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
    use crate::test_support::{DirectorioDePrueba, base_con_clave, clave_de_prueba};

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
            db_path: None,
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
            name: None,
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
    /// `backup_jobs` tiene **dos** claves foráneas —`target_id` y `repository_id`—, las dos
    /// `NOT NULL` y sin excepción, así que se cubren las dos: ni una tarea sin target, ni una
    /// tarea sin repositorio, ni una ejecución sin tarea llegan a existir. La comprobación
    /// con SQL directo es la que demuestra que no ha quedado ni una fila «a medias».
    #[tokio::test]
    async fn una_insercion_huerfana_se_rechaza_y_no_deja_fila() {
        let (_directorio, pool, tareas, target_id, repository_id) = base_con_tarea_posible().await;

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

        // Y la **otra** clave foránea: una tarea que apunta a un repositorio que no existe.
        let error = tareas
            .alta(alta_tarea(&target_id, &inexistente, "0 4 * * *"))
            .await
            .expect_err("un repositorio inexistente no puede dar una tarea");

        assert!(
            matches!(error, DbError::Consulta { .. }),
            "el fallo debe llegar como error tipado: {error}"
        );

        // Y una ejecución que apunta a una tarea que no existe. La capa pide la clave de
        // cifrado —resuelve el `db_password` del target del vínculo—, y aquí no se descifra
        // nada: se le da la de las pruebas, que es la de la base que se acaba de abrir.
        let ejecuciones = Ejecuciones::nuevo(pool.clone(), clave_de_prueba());

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

    /// 1.1 (entidad) — Un `INSERT` directo que omita `name` se acepta y deja la columna a `NULL`.
    ///
    /// Es la mitad que el pragma del esquema no mide por sí sola: la columna de la `0005` es
    /// **anulable**, así que una fila escrita con SQL a mano —sin pasar por esta capa, que es como
    /// la escribiría `sqlite3`— tiene que entrar sin el nombre y quedar a `NULL`. Una columna
    /// declarada `NOT NULL` sin valor por defecto pasaría una prueba que solo mirara que existe y
    /// rechazaría esta inserción.
    #[tokio::test]
    async fn un_alta_directa_sin_name_deja_la_columna_a_null() {
        let (_directorio, pool, _tareas, target_id, repository_id) = base_con_tarea_posible().await;

        let id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression) \
             VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(id.hyphenated().to_string())
        .bind(target_id.hyphenated().to_string())
        .bind(repository_id.hyphenated().to_string())
        .bind("0 3 * * *")
        .execute(&pool)
        .await
        .expect(
            "una fila sin `name` tiene que poder insertarse: si la columna fuera `NOT NULL` sin \
             valor por defecto, esto fallaría",
        );

        let nombre =
            sqlx::query_scalar::<_, Option<String>>("SELECT name FROM backup_jobs WHERE id = ?1")
                .bind(id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("la columna `name` tiene que poder leerse en la fila insertada sin ella");

        assert!(
            nombre.is_none(),
            "una fila insertada sin `name` tiene que quedar a NULL, y quedó {nombre:?}"
        );
    }

    /// 1.2 (entidad) — `actualizar` con un nombre nuevo lo persiste en la columna.
    ///
    /// Hoy el nombre solo se prueba por el `PUT` de la API; esta prueba lo fija donde de verdad se
    /// escribe, que es la sentencia de la capa de acceso. Un `UPDATE` que se dejara `name` fuera de
    /// la lista de columnas —o que lo bindeara en otro hueco— pasaría el ciclo de la 1.28, que da de
    /// alta sin nombre y nunca toca el campo, y entonces una edición posterior no guardaría nada.
    #[tokio::test]
    async fn la_actualizacion_persiste_el_nombre_nuevo() {
        let (_directorio, _pool, tareas, target_id, repository_id) = base_con_tarea_posible().await;

        let mut tarea = tareas
            .alta(alta_tarea(&target_id, &repository_id, "0 3 * * *"))
            .await
            .expect("el alta no puede fallar");

        assert!(
            tarea.name.is_none(),
            "el alta de la prueba entra sin nombre, así que lo que se mide es la edición"
        );

        tarea.name = Some("copia nocturna".to_owned());

        assert!(
            tareas
                .actualizar(&tarea)
                .await
                .expect("la actualización no puede fallar"),
            "la fila existía, así que la actualización debe decir que sí"
        );

        let releida = tareas
            .leer(&tarea.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe seguir existiendo");

        assert_eq!(
            releida.name.as_deref(),
            Some("copia nocturna"),
            "el nombre pasado a `actualizar` tiene que quedar en la columna"
        );
    }
}
