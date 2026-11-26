//! La tabla `restore_executions`: la entidad, el alta y la capa de acceso.
//!
//! Es el histórico de **restauraciones**, el hermano de `job_executions` y un añadido al §3
//! —el SRS modela la copia, no la restauración—. Una fila por intento de restaurar una
//! instantánea, con su estado, el `snapshot` que se restauró, el target sobre el que se
//! escribió, el motivo del fallo y las dos fechas. `started_at` es `NOT NULL` y **sin** valor
//! por defecto —lo informa quien crea la fila—, y `finished_at` es anulable: una restauración
//! en curso no tiene final, y `NULL` **no** significa «ahora».
//!
//! `target_id` es anulable, así que la entidad lo lleva como `Option`. Una restauración que
//! entró sin target resuelto —el vínculo de la instantánea con su tarea puede no existir— se
//! lee como `None` y **no** como un identificador inventado.
//!
//! **No tiene ninguna columna de secreto**, así que su capa de acceso **no** recibe la clave
//! de cifrado: pasarle una que no se usa sugeriría que aquí se cifra algo.
//!
//! El estado se reutiliza de [`Estado`] —el de `job_executions`— y no se declara un enumerado
//! propio: el `CHECK` de la `0006` admite **los mismos cuatro literales**, y una segunda copia
//! del enumerado sería una segunda boca que escribiría esos textos, justo lo que
//! `Estado::texto` existe para evitar. El día que la restauración gane una fase, la gana el
//! `CHECK` y la gana su `match`, y ese día se decide si son dos enumerados o uno.

use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteRow;
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;

use crate::db::DbError;
use crate::db::entities::execution::Estado;
use crate::db::entities::target::TipoDeTarget;

/// Fila de `restore_executions`.
#[derive(Debug, Clone)]
pub struct Restauracion {
    /// UUIDv4 en forma canónica con guiones.
    pub id: Uuid,
    /// Identificador del `snapshot` que se restaura.
    pub snapshot_id: String,
    /// Target sobre el que se escribe; `None` si no se resolvió ninguno.
    pub target_id: Option<Uuid>,
    /// Estado de la restauración.
    pub status: Estado,
    /// Motivo del fallo; `None` si no falló.
    pub error_message: Option<String>,
    /// Cuándo empezó, en UTC.
    pub started_at: OffsetDateTime,
    /// Cuándo terminó, en UTC; `None` si sigue en curso.
    pub finished_at: Option<OffsetDateTime>,
}

/// Los datos de un alta. **Sin `id` y sin `status`**: el identificador lo genera la capa de
/// acceso y el estado con el que nace una restauración es siempre [`Estado::Pending`].
#[derive(Debug, Clone)]
pub struct AltaDeRestauracion {
    /// Identificador del `snapshot` que se restaura.
    pub snapshot_id: String,
    /// Target sobre el que se escribe; `None` si no se resolvió ninguno.
    pub target_id: Option<Uuid>,
    /// Cuándo empezó, en UTC.
    pub started_at: OffsetDateTime,
}

/// La capa de acceso de `restore_executions`.
pub struct Restauraciones {
    pool: SqlitePool,
}

/// Los filtros y la ventana del listado de `restore_executions`.
///
/// Los dos filtros son **opcionales y ausentes por defecto**, y la ventana llega siempre
/// informada: el valor por defecto y el tope son del contrato HTTP, así que quien los decide es
/// el manejador —que es quien conoce `limit`— y la capa de acceso no tiene que saber que
/// existen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FiltroDeRestauraciones {
    /// Target por el que filtrar; `None` no filtra.
    pub target_id: Option<Uuid>,
    /// Estado por el que filtrar; `None` no filtra.
    pub status: Option<Estado>,
    /// Cuántas filas devolver como mucho.
    pub limite: i64,
    /// Cuántas filas saltar desde el principio del orden.
    pub desplazamiento: i64,
}

/// Una fila del listado: la restauración y los datos de su target, que **no** viven en
/// `restore_executions`.
///
/// El nombre y el tipo del target salen del vínculo `restore_executions.target_id →
/// backup_targets.id` con un `LEFT JOIN`, no de la fila: la tabla de restauraciones guarda **cuál**
/// es el target, no cómo se llama ni de qué tipo es. Es el mismo reparto que el histórico de copias
/// hace con el nombre de su tarea, y por la misma razón: el listado tiene que poder anunciar el
/// target sin una consulta por fila.
///
/// `target_name` y `target_type` son `Option` porque la fila **puede** no tener target —el contrato
/// lo admite— y entonces el `LEFT JOIN` no casa con ninguna y los tres viajan como `None`. Cuando la
/// fila **sí** tiene target, la clave foránea garantiza que la fila referenciada existe, así que el
/// `JOIN` la encuentra y los dos salen informados: ni el nombre ni el tipo se publican como `null`
/// con un `target_id` que no lo es.
#[derive(Debug, Clone)]
pub struct RestauracionListada {
    /// La fila de `restore_executions`.
    pub restauracion: Restauracion,
    /// Nombre del target referenciado; `None` si la fila no tiene target.
    pub target_name: Option<String>,
    /// Tipo del target referenciado; `None` si la fila no tiene target.
    pub target_type: Option<TipoDeTarget>,
}

/// Una página del histórico de restauraciones: la ventana y **el total** de lo que cumple el
/// filtro.
///
/// El total va con la ventana y no se deja a una segunda llamada: el contrato del listado
/// publica «una página acotada con el total», y un total que el llamante tuviera que pedir
/// aparte es un total que puede olvidarse —o pedirse con otros filtros—. Aquí viajan juntos y
/// salen de los mismos dos filtros.
///
/// El total cuenta **todas** las filas que cumplen el filtro, no las de la ventana: es lo que
/// permite al panel saber cuántas páginas hay sin pedirlas todas.
#[derive(Debug, Clone)]
pub struct PaginaDeRestauraciones {
    /// Las filas de la ventana, en el orden del contrato, con los datos de su target.
    pub filas: Vec<RestauracionListada>,
    /// Cuántas filas cumplen el filtro en total, sin aplicar la ventana.
    pub total: i64,
}

// Las sentencias, como constantes: `DbError::Consulta` lleva el texto de la consulta,
// así que la que se ejecuta y la que viaja en el error tienen que ser **la misma
// cadena**. Con el literal repetido en los dos sitios, lo que se ejecuta y lo que se
// registra pueden separarse sin que nada lo delate.
//
// Los valores van **enlazados** (`?1`, `?2`…): ninguna se compone concatenando ni se
// interpola un valor, ni siquiera para armarla en piezas.

/// Columnas del `SELECT`, en el orden de la `0006` —el mismo que recorre [`desde_fila`]—.
const SQL_LEER: &str = "SELECT id, snapshot_id, target_id, status, error_message, started_at, \
     finished_at FROM restore_executions WHERE id = ?1";

/// Alta de una restauración, devolviendo la fila tal y como quedó.
///
/// Si el `target_id` no existe, la clave foránea rechaza la fila: una restauración que apunta
/// a un target inexistente no llega a existir. Y si llega a `NULL`, la clave foránea no
/// opina —`NULL` no referencia nada en SQL—, que es el caso de la restauración sin target
/// resuelto.
const SQL_ALTA: &str = "INSERT INTO restore_executions \
     (id, snapshot_id, target_id, status, error_message, started_at, finished_at) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
     RETURNING id, snapshot_id, target_id, status, error_message, started_at, finished_at";

/// Listado filtrado y paginado, con el orden del contrato y los datos del target.
///
/// El `WHERE` se escribe **una sola vez** y con los dos filtros dentro de la misma sentencia:
/// `?1 IS NULL OR target_id = ?1` no filtra cuando el filtro no llega, así que la consulta es
/// una cadena **estática** y los cuatro valores van enlazados. Componerla en piezas según los
/// filtros presentes sería construir una consulta concatenando, que es lo que la auditoría de
/// este change prohíbe.
///
/// `ORDER BY started_at DESC, id` y no `started_at` a secas: dos restauraciones que arrancan en
/// el mismo segundo empatan y sin desempate dos peticiones seguidas con el mismo `offset`
/// pueden repetir una fila y saltarse otra. El descendente es el del contrato: lo último
/// primero, que es lo que el panel pinta.
///
/// El `LEFT JOIN` a `backup_targets` trae el nombre y el tipo del target referenciado —las dos
/// columnas que la tabla de restauraciones **no** guarda— sin perder la fila cuando no hay target:
/// `LEFT` y no `INNER`, para que una restauración sin target siga listándose con sus huecos. El
/// `ON` va por el identificador y no por otra columna: `target_id` es la clave foránea.
const SQL_LISTAR_FILTRADO: &str = "SELECT r.id, r.snapshot_id, r.target_id, r.status, \
     r.error_message, r.started_at, r.finished_at, t.name AS target_name, \
     t.target_type AS target_type FROM restore_executions r \
     LEFT JOIN backup_targets t ON t.id = r.target_id \
     WHERE (?1 IS NULL OR r.target_id = ?1) AND (?2 IS NULL OR r.status = ?2) \
     ORDER BY r.started_at DESC, r.id LIMIT ?3 OFFSET ?4";

/// El total de filas que cumplen el mismo filtro, **sin** la ventana.
///
/// Es la hermana de [`SQL_LISTAR_FILTRADO`] y comparte su `WHERE` entero —los mismos dos
/// filtros, escritos una sola vez cada uno dentro de una cadena estática—, para que la página y
/// el total no puedan acotar por cosas distintas.
const SQL_CONTAR_FILTRADO: &str = "SELECT COUNT(*) FROM restore_executions \
     WHERE (?1 IS NULL OR target_id = ?1) AND (?2 IS NULL OR status = ?2)";

/// El texto con el que viaja un fallo al **abrir** la transacción del listado: ver
/// [`SQL_LISTAR_FILTRADO`], que es lo que va dentro.
const CONSULTA_ABRIR_TRANSACCION: &str =
    "BEGIN de la transacción del listado de restore_executions";

/// El texto con el que viaja un fallo al **cerrar** la transacción del listado: ver
/// [`CONSULTA_ABRIR_TRANSACCION`].
const CONSULTA_CERRAR_TRANSACCION: &str =
    "COMMIT de la transacción del listado de restore_executions";

/// Actualización del desenlace: estado, motivo y final.
///
/// `snapshot_id`, `target_id` y `started_at` **no** entran: son la identidad de la fila —qué se
/// restauró, dónde y desde cuándo—, no campos que se editen. Lo que cambia mientras una
/// restauración vive es su estado, su motivo de fallo y su final.
const SQL_ACTUALIZAR: &str =
    "UPDATE restore_executions SET status = ?1, error_message = ?2, finished_at = ?3 WHERE id = ?4";

/// Texto del error cuando falla el **mapeo** de una fila ya traída.
///
/// No es una sentencia ejecutada —la fila ya llegó—, así que no se confunde con las
/// `SQL_*`: dice dónde falló, que es leer una columna. Es lo que el registro necesita
/// saber cuando el fallo no viene del motor.
const CONSULTA_MAPEO: &str = "mapeo de la fila de restore_executions";

impl Restauraciones {
    /// Construye la capa de acceso sobre un pool.
    ///
    /// **Sin clave de cifrado**: `restore_executions` no tiene ninguna columna de secreto, y
    /// pedir una que no se usa sugeriría que aquí se cifra algo.
    pub fn nuevo(pool: SqlitePool) -> Self {
        Restauraciones { pool }
    }

    /// Da de alta una restauración **en `pending`** y devuelve la fila tal y como quedó.
    ///
    /// El estado se enlaza desde [`Estado::texto`] **en vez de** omitirlo y dejar que lo ponga el
    /// valor por defecto del `CHECK`: el defecto es la red para una fila que entre por otra vía, y
    /// la capa de acceso dice lo que escribe en vez de heredarlo. El `pending` es además el
    /// contrato —una restauración existe **antes** de contestar el `200`—, y quien la pasa a
    /// `running` es el motor.
    ///
    /// Una restauración que apunta a un target que no existe se rechaza con error: la clave
    /// foránea de la `0006` no admite huérfanas.
    pub async fn alta(&self, alta: AltaDeRestauracion) -> Result<Restauracion, DbError> {
        // El identificador se genera **aquí**, no en la base: SQLite no tiene `uuid()`
        // y un valor por defecto en el DDL obligaría a interpolarlo en la sentencia.
        let id = Uuid::new_v4();

        let fila = sqlx::query(SQL_ALTA)
            // El `id` y la clave foránea van como texto con guiones: un `bind` de `Uuid`
            // los escribiría como un BLOB de 16 bytes y la `0006` dice `TEXT`.
            .bind(id.hyphenated().to_string())
            .bind(&alta.snapshot_id)
            .bind(alta.target_id.map(|id| id.hyphenated().to_string()))
            .bind(Estado::Pending.texto())
            .bind(Option::<String>::None)
            // La fecha se normaliza a UTC **al enlazarla**: el `Encode` de `time` conserva el
            // desplazamiento del valor, y la columna es UTC, así que un llamante con `+02:00`
            // no puede dejar la fila fuera de UTC. Es el mismo instante, en otra representación.
            .bind(alta.started_at.to_offset(UtcOffset::UTC))
            .bind(Option::<OffsetDateTime>::None)
            .fetch_one(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ALTA))?;

        desde_fila(&fila)
    }

    /// Lee una restauración por su identificador: `Ok(None)` si no está.
    ///
    /// «No está» y «no se pudo leer» son cosas distintas: lo primero es `Ok(None)` y lo
    /// segundo, un error. Confundirlos convertiría una base caída en «no existe», que
    /// es lo que hace que un fallo se persiga en el sitio equivocado.
    pub async fn leer(&self, id: &Uuid) -> Result<Option<Restauracion>, DbError> {
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

    /// Lista las restauraciones que cumplen el filtro, en el orden del contrato, **con el total** y
    /// con los datos de su target.
    ///
    /// La ventana es obligatoria —llega ya resuelta— porque el histórico crece y, sin tope, un
    /// listado materializaría la tabla entera en memoria y ocuparía una de las conexiones del
    /// pool mientras el motor quiere escribir.
    ///
    /// Son **dos** lecturas y no una: la página la resuelve [`SQL_LISTAR_FILTRADO`] y el total
    /// [`SQL_CONTAR_FILTRADO`], que es la única forma de saber cuántas hay sin traerlas. Las dos van
    /// con los mismos filtros y en la **misma transacción**: sin ella el motor podría escribir entre
    /// la página y la cuenta y el total diría un número que la página no sostiene. Las dos son
    /// lecturas, así que la transacción no bloquea a nadie mientras dura.
    ///
    /// El total lo cuenta [`SQL_CONTAR_FILTRADO`] sobre la tabla **sola** y no sobre el `JOIN`:
    /// contar las filas de un `LEFT JOIN` que no multiplica —el `ON` casa con una clave primaria—
    /// daría lo mismo, pero la cuenta no necesita el nombre de nadie, así que no paga la unión.
    pub async fn listar_filtrado(
        &self,
        filtro: &FiltroDeRestauraciones,
    ) -> Result<PaginaDeRestauraciones, DbError> {
        let mut transaccion = self
            .pool
            .begin()
            .await
            .map_err(error_de_consulta(CONSULTA_ABRIR_TRANSACCION))?;

        let filas = sqlx::query(SQL_LISTAR_FILTRADO)
            .bind(filtro.target_id.map(|id| id.hyphenated().to_string()))
            .bind(filtro.status.map(Estado::texto))
            .bind(filtro.limite)
            .bind(filtro.desplazamiento)
            .fetch_all(&mut *transaccion)
            .await
            .map_err(error_de_consulta(SQL_LISTAR_FILTRADO))?;

        let total = sqlx::query_scalar(SQL_CONTAR_FILTRADO)
            .bind(filtro.target_id.map(|id| id.hyphenated().to_string()))
            .bind(filtro.status.map(Estado::texto))
            .fetch_one(&mut *transaccion)
            .await
            .map_err(error_de_consulta(SQL_CONTAR_FILTRADO))?;

        transaccion
            .commit()
            .await
            .map_err(error_de_consulta(CONSULTA_CERRAR_TRANSACCION))?;

        let filas = filas
            .iter()
            .map(desde_fila_listada)
            .collect::<Result<_, _>>()?;

        Ok(PaginaDeRestauraciones { filas, total })
    }

    /// Actualiza el desenlace de la fila y devuelve si existía.
    ///
    /// Solo se escriben el estado, el motivo y el final —ver [`SQL_ACTUALIZAR`]—, así que la
    /// fila que entra puede venir de [`Restauraciones::leer`] con esos tres campos tocados y el
    /// resto como estaban. Un `false` significa **«no había fila con ese `id`»**, no «no se
    /// pudo»: un fallo de base sigue siendo `Err`.
    pub async fn actualizar(&self, restauracion: &Restauracion) -> Result<bool, DbError> {
        let resultado = sqlx::query(SQL_ACTUALIZAR)
            .bind(restauracion.status.texto())
            .bind(&restauracion.error_message)
            // Misma normalización que en el alta: la columna es UTC.
            .bind(
                restauracion
                    .finished_at
                    .map(|fecha| fecha.to_offset(UtcOffset::UTC)),
            )
            .bind(restauracion.id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ACTUALIZAR))?;

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

/// El texto de la columna como `Uuid`, o el fallo del mapeo con su causa.
///
/// La conversión es del mapeo y no de `sqlx` —la columna es `TEXT` y la entidad es `Uuid`—, así
/// que su error **no** es un `sqlx::Error` y se envuelve en `sqlx::Error::Decode` para viajar
/// por `DbError::Consulta` con la causa intacta, sin estrenar una variante para un fallo que ya
/// tiene sitio.
fn uuid_de_la_columna(fila: &SqliteRow, nombre: &str) -> Result<Uuid, DbError> {
    let texto = columna::<String>(fila, nombre)?;

    Uuid::parse_str(&texto).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO,
        causa: sqlx::Error::Decode(Box::new(causa)),
    })
}

/// Mapea una fila de `restore_executions` a la entidad.
///
/// No descifra nada: esta tabla no tiene secretos. `target_id` y `finished_at` son anulables y
/// salen como `None` cuando la columna es `NULL`, sin inventarse ningún valor.
fn desde_fila(fila: &SqliteRow) -> Result<Restauracion, DbError> {
    let target_id = columna::<Option<String>>(fila, "target_id")?
        .map(|texto| {
            Uuid::parse_str(&texto).map_err(|causa| DbError::Consulta {
                consulta: CONSULTA_MAPEO,
                causa: sqlx::Error::Decode(Box::new(causa)),
            })
        })
        .transpose()?;

    Ok(Restauracion {
        id: uuid_de_la_columna(fila, "id")?,
        snapshot_id: columna(fila, "snapshot_id")?,
        target_id,
        status: columna(fila, "status")?,
        error_message: columna(fila, "error_message")?,
        started_at: columna(fila, "started_at")?,
        finished_at: columna(fila, "finished_at")?,
    })
}

/// Mapea una fila del **listado** a la fila con su target resuelto.
///
/// La mitad de la restauración la lee [`desde_fila`] —el `SELECT` del listado trae sus mismas
/// columnas con sus mismos nombres—, y la otra mitad son las dos del `LEFT JOIN`. Se leen como
/// `Option` porque el `LEFT JOIN` las deja `NULL` cuando la fila no tiene target, y ese `NULL` es
/// un hueco legítimo —el contrato lo publica como `null`—, no un fallo de mapeo: por eso van por
/// [`columna`] con `Option` y no por una lectura que exigiera el valor.
fn desde_fila_listada(fila: &SqliteRow) -> Result<RestauracionListada, DbError> {
    Ok(RestauracionListada {
        restauracion: desde_fila(fila)?,
        target_name: columna(fila, "target_name")?,
        target_type: columna(fila, "target_type")?,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sqlx::SqlitePool;
    use time::macros::datetime;
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    use super::{
        AltaDeRestauracion, Estado, FiltroDeRestauraciones, PaginaDeRestauraciones, Restauracion,
        Restauraciones,
    };
    use crate::db::crypto::Clave;
    use crate::db::entities::target::{AltaDeTarget, Targets, TipoDeTarget};
    use crate::test_support::{DirectorioDePrueba, base_con_clave};

    /// La instantánea de las pruebas: distinguible y larga.
    const INSTANTANEA: &str = "instantanea-distinguible-000000000000000000000001";

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

    /// Una restauración recién encolada, con la instantánea y el target dados.
    fn alta_restauracion(target_id: Option<Uuid>) -> AltaDeRestauracion {
        AltaDeRestauracion {
            snapshot_id: INSTANTANEA.to_owned(),
            target_id,
            started_at: OffsetDateTime::now_utc(),
        }
    }

    /// Un pool migrado con un target ya dado de alta: lo que `restore_executions` necesita
    /// para no ser huérfana.
    async fn base_con_restauracion_posible() -> (
        DirectorioDePrueba,
        SqlitePool,
        Restauraciones,
        Uuid,
        Arc<Clave>,
    ) {
        let (directorio, pool, clave) = base_con_clave().await;

        let target = Targets::nuevo(pool.clone(), clave.clone())
            .alta(alta_target("target-de-restauracion"))
            .await
            .expect("el alta del target no puede fallar");

        let restauraciones = Restauraciones::nuevo(pool.clone());

        (directorio, pool, restauraciones, target.id, clave)
    }

    /// Cuántas filas tiene `restore_executions`, con **SQL directo**.
    ///
    /// Se cuenta contra la tabla y no con la capa de acceso a propósito: lo que estas pruebas
    /// afirman es qué quedó **escrito**, y una comprobación que pasara por la misma capa que se
    /// está probando podría salir bien por la razón equivocada. El recuento de la tabla es lo
    /// único que no comparte código con lo que se mide.
    async fn filas(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM restore_executions")
            .fetch_one(pool)
            .await
            .expect("el recuento de filas debe poder leerse")
    }

    /// La definición de la tabla tal y como la dejó la migración.
    async fn definicion_de_la_tabla(pool: &SqlitePool) -> String {
        sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'restore_executions'",
        )
        .fetch_one(pool)
        .await
        .expect("la definición de restore_executions debe poder leerse")
    }

    /// Una fila de `PRAGMA table_info`, con lo que interesa de cada columna.
    struct Columna {
        nombre: String,
        notnull: i64,
        por_defecto: Option<String>,
        clave_primaria: i64,
    }

    /// Las columnas que la migración dejó en `restore_executions`, en su orden declarado.
    async fn columnas(pool: &SqlitePool) -> Vec<Columna> {
        sqlx::query("PRAGMA table_info(restore_executions)")
            .fetch_all(pool)
            .await
            .expect("el `PRAGMA table_info` debe poder leerse")
            .iter()
            .map(|fila| Columna {
                nombre: sqlx::Row::try_get(fila, "name").expect("la columna tiene nombre"),
                notnull: sqlx::Row::try_get(fila, "notnull")
                    .expect("la columna declara si es nula"),
                por_defecto: sqlx::Row::try_get(fila, "dflt_value")
                    .expect("la columna declara su valor por defecto"),
                clave_primaria: sqlx::Row::try_get(fila, "pk")
                    .expect("la columna declara si es clave primaria"),
            })
            .collect()
    }

    /// Las columnas del índice dado, en su orden.
    ///
    /// El parámetro es `&'static str`, y eso es lo que hace segura la interpolación: el
    /// nombre del índice solo puede ser una **constante de compilación**, así que el
    /// compilador **impide** que entre una cadena construida en ejecución. `AssertSqlSafe`
    /// sigue haciendo falta porque `sqlx 0.9` solo implementa `SqlSafeStr` para `&'static str`
    /// y aquí el texto se compone con `format!` —un `PRAGMA` no admite valor enlazado—. La
    /// consulta que llega a SQLite es la misma que antes del salto
    /// (`saneamiento-de-dependencias`).
    async fn columnas_del_indice(pool: &SqlitePool, indice: &'static str) -> Vec<String> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA index_info('{indice}')"
        )))
        .fetch_all(pool)
        .await
        .expect("el `PRAGMA index_info` debe poder leerse")
        .iter()
        .map(|fila| sqlx::Row::try_get(fila, "name").expect("la columna del índice tiene nombre"))
        .collect()
    }

    /// 0.2 — La migración crea la tabla, sus columnas, su `CHECK`, su clave foránea y su índice.
    ///
    /// Las tres comprobaciones van por caminos distintos porque miden cosas distintas: las
    /// columnas y la clave foránea salen de los `PRAGMA` —lo que SQLite entiende, no lo que el
    /// texto del fichero dice—, el `CHECK` sale de la definición guardada —el `PRAGMA` no lo
    /// publica— y el índice, de `index_info`, que además dice **sobre qué columnas** es.
    #[tokio::test]
    async fn la_migracion_crea_la_tabla_sus_columnas_su_check_su_clave_foranea_y_su_indice() {
        let (_directorio, pool, _clave) = base_con_clave().await;

        // Las columnas, en el orden declarado, con su nulabilidad, su defecto y su clave.
        let columnas = columnas(&pool).await;
        let esperadas: [(&str, i64, Option<&str>, i64); 7] = [
            ("id", 1, None, 1),
            ("snapshot_id", 1, None, 0),
            ("target_id", 0, None, 0),
            ("status", 1, Some("'pending'"), 0),
            ("error_message", 0, None, 0),
            ("started_at", 1, None, 0),
            ("finished_at", 0, None, 0),
        ];

        assert_eq!(
            columnas.len(),
            esperadas.len(),
            "la tabla tiene un número de columnas distinto del contrato: {:?}",
            columnas.iter().map(|c| &c.nombre).collect::<Vec<_>>()
        );

        for (columna, (nombre, notnull, por_defecto, clave_primaria)) in
            columnas.iter().zip(esperadas)
        {
            assert_eq!(
                columna.nombre, nombre,
                "el orden de las columnas no es el de la 0006"
            );
            assert_eq!(
                columna.notnull, notnull,
                "la columna `{nombre}` no declara la nulabilidad del contrato"
            );
            assert_eq!(
                columna.por_defecto.as_deref(),
                por_defecto,
                "la columna `{nombre}` no declara el valor por defecto del contrato"
            );
            assert_eq!(
                columna.clave_primaria, clave_primaria,
                "la columna `{nombre}` no declara la clave primaria del contrato"
            );
        }

        // El `CHECK`, que los `PRAGMA` no publican: se lee de la definición guardada.
        let definicion = definicion_de_la_tabla(&pool).await;

        assert!(
            definicion.contains("CHECK(status IN ('pending', 'running', 'success', 'failed'))"),
            "el `CHECK` de `status` no es el del contrato: {definicion}"
        );

        // La clave foránea, estructuralmente: a qué tabla, por qué columna, a qué columna y
        // qué hace al borrar.
        let claves = sqlx::query("PRAGMA foreign_key_list(restore_executions)")
            .fetch_all(&pool)
            .await
            .expect("el `PRAGMA foreign_key_list` debe poder leerse");

        assert_eq!(
            claves.len(),
            1,
            "la tabla debe tener exactamente una clave foránea"
        );

        let clave = &claves[0];
        let tabla_destino: String =
            sqlx::Row::try_get(clave, "table").expect("la clave foránea nombra su tabla");
        let columna_hija: String =
            sqlx::Row::try_get(clave, "from").expect("la clave foránea nombra su columna hija");
        let columna_padre: String =
            sqlx::Row::try_get(clave, "to").expect("la clave foránea nombra su columna padre");
        let al_borrar: String =
            sqlx::Row::try_get(clave, "on_delete").expect("la clave foránea declara su borrado");

        assert_eq!(
            tabla_destino, "backup_targets",
            "la clave foránea apunta a otra tabla"
        );
        assert_eq!(
            columna_hija, "target_id",
            "la clave foránea sale de otra columna"
        );
        assert_eq!(
            columna_padre, "id",
            "la clave foránea no apunta al `id` del target"
        );
        assert_eq!(
            al_borrar, "CASCADE",
            "el borrado del target no arrastra el histórico"
        );

        // El índice del listado, y sobre qué columnas está.
        let existe: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' \
             AND name = 'idx_restore_executions_status_started_at'",
        )
        .fetch_one(&pool)
        .await
        .expect("el catálogo debe poder leerse");

        assert_eq!(existe, 1, "falta el índice del listado por estado y fecha");
        assert_eq!(
            columnas_del_indice(&pool, "idx_restore_executions_status_started_at").await,
            vec!["status".to_owned(), "started_at".to_owned()],
            "el índice no cubre la pareja (status, started_at) del contrato"
        );
    }

    /// 0.2 — Un `status` fuera del enumerado lo rechaza el `CHECK`, y no deja fila.
    ///
    /// La escritura va con SQL directo porque el enumerado de la capa de acceso **no puede**
    /// representar un estado inválido: lo que se mide aquí es la última defensa, la de la base,
    /// que es la única que ve una fila escrita por otra vía.
    #[tokio::test]
    async fn un_status_fuera_del_enumerado_lo_rechaza_el_check() {
        let (_directorio, pool, _restauraciones, target, _clave) =
            base_con_restauracion_posible().await;

        let resultado = sqlx::query(
            "INSERT INTO restore_executions (id, snapshot_id, target_id, status, started_at) \
             VALUES (?1, ?2, ?3, 'cancelada', ?4)",
        )
        .bind(Uuid::new_v4().hyphenated().to_string())
        .bind(INSTANTANEA)
        .bind(target.hyphenated().to_string())
        .bind(datetime!(2026-03-01 10:00:00 UTC))
        .execute(&pool)
        .await;

        let error = resultado.expect_err("un status fuera del enumerado tiene que rechazarse");

        assert!(
            error.to_string().contains("CHECK"),
            "la escritura falló, pero no por el `CHECK`: {error}"
        );
        assert_eq!(filas(&pool).await, 0, "el rechazo dejó una fila escrita");
    }

    /// 0.2 — Borrar el target borra su histórico de restauraciones.
    ///
    /// Se comprueba además que la cascada ocurre **con las claves foráneas encendidas** en el
    /// pool, que es lo que la hace ocurrir: con el pragma apagado, el `DELETE` del target
    /// dejaría la fila huérfana sin que nada lo delatara.
    #[tokio::test]
    async fn borrar_el_target_borra_su_historico() {
        let (_directorio, pool, restauraciones, target, clave) =
            base_con_restauracion_posible().await;

        let encendidas: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .expect("el estado de las claves foráneas debe poder consultarse");

        assert_eq!(
            encendidas, 1,
            "el pool de prueba no tiene las claves foráneas encendidas: la cascada no ocurriría"
        );

        let creada = restauraciones
            .alta(alta_restauracion(Some(target)))
            .await
            .expect("el alta no puede fallar");

        assert_eq!(
            filas(&pool).await,
            1,
            "el alta no dejó la fila que la cascada tiene que arrastrar"
        );

        let borrado = Targets::nuevo(pool.clone(), clave.clone())
            .borrar(&target)
            .await
            .expect("el borrado del target no puede fallar");

        assert!(
            borrado,
            "el target existía, así que el borrado debe decir que sí"
        );
        assert_eq!(
            filas(&pool).await,
            0,
            "borrar el target dejó su histórico de restauraciones huérfano"
        );
        assert!(
            restauraciones
                .leer(&creada.id)
                .await
                .expect("la lectura no puede fallar")
                .is_none(),
            "la fila de la restauración sobrevivió al borrado de su target"
        );
    }

    /// 0.2 — El alta deja la fila en `pending`, con su instantánea, su target y sin final.
    ///
    /// La comprobación con SQL directo es la que fija que el enumerado no se guarde como número
    /// y que las dos columnas anulables sean `NULL` de verdad, no cadenas vacías.
    #[tokio::test]
    async fn el_alta_deja_la_fila_en_pending_sin_final_ni_motivo() {
        let (_directorio, pool, restauraciones, target, _clave) =
            base_con_restauracion_posible().await;

        let creada = restauraciones
            .alta(alta_restauracion(Some(target)))
            .await
            .expect("el alta no puede fallar");

        assert_eq!(
            creada.status,
            Estado::Pending,
            "una restauración nace encolada"
        );
        assert_eq!(creada.snapshot_id, INSTANTANEA);
        assert_eq!(creada.target_id, Some(target));
        assert_eq!(creada.error_message, None);
        assert_eq!(creada.finished_at, None);
        assert_eq!(
            creada.id.hyphenated().to_string().len(),
            36,
            "el identificador no se guardó en forma canónica con guiones"
        );

        let leida = restauraciones
            .leer(&creada.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila recién dada de alta debe estar");

        assert_eq!(leida.id, creada.id, "el id leído no es el del alta");
        assert_eq!(leida.status, Estado::Pending);
        assert_eq!(
            leida.started_at, creada.started_at,
            "el inicio no volvió igual"
        );

        let estado_almacenado: String =
            sqlx::query_scalar("SELECT status FROM restore_executions WHERE id = ?1")
                .bind(creada.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el estado debe poder leerse");

        assert_eq!(
            estado_almacenado, "pending",
            "el enumerado no se guardó como texto, sino como {estado_almacenado}"
        );

        let nulos: i64 = sqlx::query_scalar(
            "SELECT (finished_at IS NULL) + (error_message IS NULL) FROM restore_executions \
             WHERE id = ?1",
        )
        .bind(creada.id.hyphenated().to_string())
        .fetch_one(&pool)
        .await
        .expect("el `IS NULL` debe poder leerse");

        assert_eq!(
            nulos, 2,
            "una restauración encolada no tiene final ni motivo de fallo"
        );

        // Y sin target: la columna es anulable y se lee como `None`, no como un UUID inventado.
        let sin_target = restauraciones
            .alta(alta_restauracion(None))
            .await
            .expect("un alta sin target no puede fallar");

        assert_eq!(sin_target.target_id, None);
        assert_ne!(
            sin_target.id, creada.id,
            "dos altas no pueden compartir identificador"
        );
        assert_eq!(
            restauraciones
                .leer(&sin_target.id)
                .await
                .expect("la lectura no puede fallar")
                .expect("la fila sin target debe estar")
                .target_id,
            None,
            "el `NULL` de `target_id` no volvió como `None`"
        );
    }

    /// Una fecha enlazada con desplazamiento **distinto de cero** vuelve en UTC.
    ///
    /// El `Encode` de `time` conserva el desplazamiento del valor, así que sin la normalización
    /// del alta la columna no estaría en UTC. Se afirma contra la **entrada** y no contra otra
    /// fecha leída, que es lo que distingue una corrupción sistemática del codificador de un
    /// valor correcto.
    #[tokio::test]
    async fn una_fecha_con_desplazamiento_vuelve_en_utc() {
        let (_directorio, _pool, restauraciones, target, _clave) =
            base_con_restauracion_posible().await;

        let inicio = datetime!(2026-03-14 09:26:53 +2:30);

        assert!(
            !inicio.offset().is_utc(),
            "el aparato de la prueba no usó un desplazamiento distinto de cero"
        );

        let creada = restauraciones
            .alta(AltaDeRestauracion {
                started_at: inicio,
                ..alta_restauracion(Some(target))
            })
            .await
            .expect("el alta no puede fallar");

        let leida = restauraciones
            .leer(&creada.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert!(
            leida.started_at.offset().is_utc(),
            "el started_at volvió con desplazamiento, no en UTC: {}",
            leida.started_at
        );
        assert_eq!(
            leida.started_at, inicio,
            "el instante guardado no es el de entrada"
        );
    }

    /// 0.2 — El desenlace con éxito escribe `success` y el final, sin motivo de fallo.
    #[tokio::test]
    async fn actualizar_a_success_escribe_el_final() {
        let (_directorio, pool, restauraciones, target, _clave) =
            base_con_restauracion_posible().await;

        let creada = restauraciones
            .alta(alta_restauracion(Some(target)))
            .await
            .expect("el alta no puede fallar");

        let mut terminada = creada.clone();
        terminada.status = Estado::Success;
        terminada.finished_at = Some(creada.started_at + Duration::seconds(30));

        assert!(
            restauraciones
                .actualizar(&terminada)
                .await
                .expect("la actualización no puede fallar"),
            "la fila existía, así que la actualización debe decir que sí"
        );

        let releida = restauraciones
            .leer(&creada.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe seguir existiendo");

        assert_eq!(
            releida.status,
            Estado::Success,
            "el estado tocado no cambió"
        );
        assert_eq!(
            releida.finished_at, terminada.finished_at,
            "el final no se escribió, o no volvió igual"
        );
        assert_eq!(
            releida.error_message, None,
            "un éxito no tiene motivo de fallo"
        );
        assert_eq!(
            releida.started_at, creada.started_at,
            "el started_at no debía cambiar"
        );
        assert_eq!(
            releida.snapshot_id, creada.snapshot_id,
            "el snapshot_id no debía cambiar"
        );
        assert_eq!(
            releida.target_id, creada.target_id,
            "el target_id no debía cambiar"
        );

        let final_escrito: i64 = sqlx::query_scalar(
            "SELECT (status = 'success') + (finished_at IS NOT NULL) FROM restore_executions \
             WHERE id = ?1",
        )
        .bind(creada.id.hyphenated().to_string())
        .fetch_one(&pool)
        .await
        .expect("las columnas deben poder leerse");

        assert_eq!(
            final_escrito, 2,
            "la tabla no quedó con `success` y el final escrito"
        );

        // Un identificador que no está no es un error: es un `false`.
        let fantasma = Restauracion {
            id: Uuid::new_v4(),
            ..creada
        };

        assert!(
            !restauraciones
                .actualizar(&fantasma)
                .await
                .expect("actualizar una fila que no existe no puede fallar"),
            "una actualización sobre una fila que no existe no puede decir que sí"
        );
    }

    /// 0.2 — Un fallo queda en `failed` con su motivo y su final, y **no** en `running`.
    #[tokio::test]
    async fn actualizar_a_failed_escribe_el_motivo_y_el_final() {
        let (_directorio, pool, restauraciones, target, _clave) =
            base_con_restauracion_posible().await;

        let creada = restauraciones
            .alta(alta_restauracion(Some(target)))
            .await
            .expect("el alta no puede fallar");

        let mut fallida = creada.clone();
        fallida.status = Estado::Failed;
        fallida.error_message = Some("fallo distinguible de la prueba 0-2".to_owned());
        fallida.finished_at = Some(creada.started_at + Duration::minutes(1));

        assert!(
            restauraciones
                .actualizar(&fallida)
                .await
                .expect("la actualización no puede fallar"),
            "la fila existía, así que la actualización debe decir que sí"
        );

        let releida = restauraciones
            .leer(&creada.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe seguir existiendo");

        assert_eq!(
            releida.status,
            Estado::Failed,
            "la restauración no quedó fallida"
        );
        assert_eq!(
            releida.error_message.as_deref(),
            Some("fallo distinguible de la prueba 0-2"),
            "el motivo del fallo no se escribió"
        );
        assert_eq!(releida.finished_at, fallida.finished_at);
        assert_ne!(
            releida.status,
            Estado::Running,
            "un fallo cerrado no puede quedarse en `running`"
        );

        let motivo_escrito: String =
            sqlx::query_scalar("SELECT error_message FROM restore_executions WHERE id = ?1")
                .bind(creada.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el motivo debe poder leerse");

        assert_eq!(motivo_escrito, "fallo distinguible de la prueba 0-2");
    }

    /// 0.2 — El listado acota por los dos filtros, ordena descendente, desempata por el
    /// identificador y pagina **con el total**.
    ///
    /// Fija la **sentencia** y no el manejador: lo que se afirma es que un filtro ausente no
    /// filtra, que el orden es `started_at DESC, id` y que la ventana se aplica sin tocar el
    /// total, que cuenta lo que cumple el filtro y no lo que cabe en la página.
    #[tokio::test]
    async fn el_listado_filtrado_acota_ordena_descendente_y_pagina_con_total() {
        let (_directorio, pool, restauraciones, target_a, clave) =
            base_con_restauracion_posible().await;

        let target_b = Targets::nuevo(pool.clone(), clave.clone())
            .alta(alta_target("target-b-de-restauracion"))
            .await
            .expect("el alta del segundo target no puede fallar")
            .id;

        // Cuatro filas: dos con la **misma** fecha —para el desempate—, una intermedia y una
        // anterior. Las fechas son elegidas y no `now_utc`: dos altas seguidas podrían caer en
        // el mismo instante y el orden quedaría en manos de lo que aquí no se quiere medir.
        let empate = datetime!(2026-03-01 10:00:00 UTC);
        let intermedia = empate - Duration::hours(1);
        let anterior = empate - Duration::hours(2);

        let mut identificadores = Vec::new();

        for (target, estado, fecha) in [
            (target_a, Estado::Success, anterior),
            (target_a, Estado::Failed, empate),
            (target_b, Estado::Success, empate),
            (target_b, Estado::Running, intermedia),
        ] {
            let mut fila = restauraciones
                .alta(AltaDeRestauracion {
                    target_id: Some(target),
                    started_at: fecha,
                    ..alta_restauracion(Some(target))
                })
                .await
                .expect("el alta de la restauración no puede fallar");

            // El alta nace siempre en `pending`: el estado que la prueba pide se escribe
            // después, por el camino que usa el motor para cerrar el ciclo de vida.
            fila.status = estado;

            assert!(
                restauraciones
                    .actualizar(&fila)
                    .await
                    .expect("la actualización no puede fallar"),
                "la fila recién dada de alta tiene que poder actualizarse"
            );

            identificadores.push(fila.id);
        }

        let sin_filtros = FiltroDeRestauraciones {
            target_id: None,
            status: None,
            limite: 50,
            desplazamiento: 0,
        };

        let PaginaDeRestauraciones { filas, total } = restauraciones
            .listar_filtrado(&sin_filtros)
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(
            total, 4,
            "sin filtros el total tiene que ser el de la tabla"
        );
        assert_eq!(
            filas.len(),
            4,
            "sin filtros tienen que salir las cuatro filas"
        );
        assert_eq!(
            filas[3].restauracion.id, identificadores[0],
            "la más antigua tiene que quedar la última"
        );

        // El nombre y el tipo del target salen del **vínculo** y no de la fila —la tabla de
        // restauraciones no los guarda—, así que se afirma aquí que el `LEFT JOIN` los trae: es lo
        // que permite al listado anunciar el target sin una consulta por fila.
        assert_eq!(
            filas[3].target_name.as_deref(),
            Some("target-de-restauracion"),
            "el listado tiene que resolver el nombre del target por su vínculo"
        );
        assert_eq!(
            filas[3].target_type,
            Some(TipoDeTarget::Directory),
            "y su tipo, con el valor que guarda el `CHECK`"
        );

        // Las dos que empatan son las **dos primeras**, y entre ellas el orden lo decide el
        // identificador. Se afirma el conjunto —que van delante de la anterior— y después el
        // desempate por identificador, que es lo que la sentencia fija.
        let mut de_las_empatadas = [filas[0].restauracion.id, filas[1].restauracion.id];
        de_las_empatadas.sort();

        let mut esperadas = [identificadores[1], identificadores[2]];
        esperadas.sort();

        assert_eq!(
            de_las_empatadas, esperadas,
            "las dos filas que empatan en fecha tienen que ir delante, y en orden de identificador"
        );
        assert_eq!(
            filas[2].restauracion.id, identificadores[3],
            "la intermedia tiene que quedar entre las empatadas y la antigua"
        );

        // El filtro por estado acota, y el total cuenta lo acotado.
        let solo_fallidas = restauraciones
            .listar_filtrado(&FiltroDeRestauraciones {
                status: Some(Estado::Failed),
                ..sin_filtros
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(solo_fallidas.total, 1, "el total no contó las fallidas");
        assert_eq!(
            solo_fallidas.filas.len(),
            1,
            "el filtro por estado tiene que acotar"
        );
        assert_eq!(solo_fallidas.filas[0].restauracion.id, identificadores[1]);

        // El filtro por target acota, y los dos filtros se combinan.
        let del_target_b = restauraciones
            .listar_filtrado(&FiltroDeRestauraciones {
                target_id: Some(target_b),
                ..sin_filtros
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(del_target_b.total, 2, "el total no acotó por target");
        assert!(
            del_target_b
                .filas
                .iter()
                .all(|fila| fila.restauracion.target_id == Some(target_b)),
            "el filtro por target trajo filas de otro target"
        );

        let de_b_en_curso = restauraciones
            .listar_filtrado(&FiltroDeRestauraciones {
                target_id: Some(target_b),
                status: Some(Estado::Running),
                ..sin_filtros
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(de_b_en_curso.total, 1, "los dos filtros no se combinaron");
        assert_eq!(de_b_en_curso.filas[0].restauracion.id, identificadores[3]);

        // Un target que no es el de las filas no devuelve ninguna: el filtro no se ignora.
        let de_otro_target = restauraciones
            .listar_filtrado(&FiltroDeRestauraciones {
                target_id: Some(Uuid::new_v4()),
                ..sin_filtros
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(
            de_otro_target.total, 0,
            "un filtro sin filas tiene que dar total cero"
        );
        assert!(
            de_otro_target.filas.is_empty(),
            "un filtro por un target sin restauraciones no puede devolver filas"
        );

        // La ventana se aplica sin tocar el total, y la segunda página trae las restantes.
        let primera_pagina = restauraciones
            .listar_filtrado(&FiltroDeRestauraciones {
                limite: 2,
                desplazamiento: 0,
                ..sin_filtros
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(
            primera_pagina.filas.len(),
            2,
            "la ventana tiene que devolver dos filas"
        );
        assert_eq!(
            primera_pagina.total, 4,
            "el total no puede encogerse con la ventana"
        );
        assert_eq!(
            primera_pagina.filas[0].restauracion.id, filas[0].restauracion.id,
            "la primera página no empieza por la primera fila del orden"
        );

        let segunda_pagina = restauraciones
            .listar_filtrado(&FiltroDeRestauraciones {
                limite: 2,
                desplazamiento: 2,
                ..sin_filtros
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(
            segunda_pagina.filas.len(),
            2,
            "la segunda página tiene que traer las restantes"
        );
        assert_eq!(segunda_pagina.total, 4, "el total no cambia entre páginas");
        assert_eq!(
            segunda_pagina.filas[0].restauracion.id, filas[2].restauracion.id,
            "la segunda página tiene que empezar donde acaba la primera"
        );
        assert_eq!(
            segunda_pagina.filas[1].restauracion.id,
            filas[3].restauracion.id
        );

        let mut paginadas: Vec<Uuid> = primera_pagina
            .filas
            .iter()
            .chain(segunda_pagina.filas.iter())
            .map(|fila| fila.restauracion.id)
            .collect();
        let mut completas: Vec<Uuid> = filas.iter().map(|fila| fila.restauracion.id).collect();

        paginadas.sort();
        completas.sort();

        assert_eq!(
            paginadas, completas,
            "las dos páginas juntas no son el listado entero: se repitió o se perdió una fila"
        );
    }

    /// 0.3b — El listado publica el target resuelto, y `null` cuando la fila no tiene ninguno.
    ///
    /// La otra mitad del `LEFT JOIN`: una restauración **sin** target —el contrato lo admite— no
    /// puede sacar un nombre ni un tipo inventados, y tampoco puede desaparecer del listado por no
    /// tenerlos. Se siembran las dos —una con target y otra sin— y se afirma cada una por separado,
    /// que es lo que distingue el hueco legítimo del mapeo roto.
    #[tokio::test]
    async fn el_listado_publica_el_target_y_null_cuando_la_fila_no_lo_tiene() {
        let (_directorio, _pool, restauraciones, target, _clave) =
            base_con_restauracion_posible().await;

        restauraciones
            .alta(alta_restauracion(Some(target)))
            .await
            .expect("el alta con target no puede fallar");

        restauraciones
            .alta(alta_restauracion(None))
            .await
            .expect("el alta sin target no puede fallar");

        let pagina = restauraciones
            .listar_filtrado(&FiltroDeRestauraciones {
                target_id: None,
                status: None,
                limite: 50,
                desplazamiento: 0,
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(
            pagina.total, 2,
            "las dos filas tienen que seguir listándose"
        );

        let con_target = pagina
            .filas
            .iter()
            .find(|fila| fila.restauracion.target_id.is_some())
            .expect("la fila con target tiene que estar en el listado");

        assert_eq!(
            con_target.target_name.as_deref(),
            Some("target-de-restauracion"),
            "una fila con target tiene que traer su nombre"
        );
        assert_eq!(
            con_target.target_type,
            Some(TipoDeTarget::Directory),
            "y su tipo"
        );

        let sin_target = pagina
            .filas
            .iter()
            .find(|fila| fila.restauracion.target_id.is_none())
            .expect("la fila sin target tiene que estar en el listado");

        assert_eq!(
            sin_target.target_name, None,
            "una fila sin target no puede inventarse un nombre"
        );
        assert_eq!(
            sin_target.target_type, None,
            "ni un tipo: los dos son huecos y viajan como `null`"
        );
    }

    /// 0.3c — La página y el conteo se resuelven en **una sola** transacción.
    ///
    /// El contrato de «el total no cuenta otra cosa que lo que la página acota» solo se sostiene si
    /// las dos consultas van dentro del mismo `begin()`/`commit()`: sueltas, entre una y otra puede
    /// entrar una escritura del motor. No se puede forzar esa carrera de forma determinista desde
    /// fuera, así que se mide su **causa**: cuántas veces el listado pide una conexión al pool. Con
    /// la transacción es **una** (la conexión se retiene hasta el `commit`); sin ella serían **dos**
    /// —una por consulta— y ahí es donde cabría la escritura intermedia.
    #[tokio::test]
    async fn el_listado_resuelve_la_pagina_y_el_conteo_en_una_sola_transaccion() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        use sqlx::sqlite::SqlitePoolOptions;

        use crate::db::{migrate, opciones_del_dsn};
        use crate::test_support::directorio_de_prueba;

        let directorio = directorio_de_prueba();

        let adquisiciones = Arc::new(AtomicUsize::new(0));
        let cuenta = adquisiciones.clone();

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .before_acquire(move |_conexion, _meta| {
                let cuenta = cuenta.clone();
                Box::pin(async move {
                    cuenta.fetch_add(1, Ordering::SeqCst);
                    Ok::<bool, sqlx::Error>(true)
                })
            })
            .connect_with(
                opciones_del_dsn(&directorio.dsn()).expect("el DSN de prueba tiene que ser válido"),
            )
            .await
            .expect("el pool de prueba tiene que abrirse");

        migrate::aplicar(&pool, &migrate::migrador())
            .await
            .expect("el esquema del §3 tiene que aplicarse");

        let restauraciones = Restauraciones::nuevo(pool.clone());

        restauraciones
            .alta(alta_restauracion(None))
            .await
            .expect("el alta no puede fallar");
        restauraciones
            .alta(alta_restauracion(None))
            .await
            .expect("el alta no puede fallar");

        // A partir de aquí solo cuenta lo que el **listado** adquiere.
        adquisiciones.store(0, Ordering::SeqCst);

        let pagina = restauraciones
            .listar_filtrado(&FiltroDeRestauraciones {
                target_id: None,
                status: None,
                limite: 50,
                desplazamiento: 0,
            })
            .await
            .expect("el listado filtrado no puede fallar");

        assert_eq!(pagina.total, 2, "el total tiene que contar las dos filas");

        assert_eq!(
            adquisiciones.load(Ordering::SeqCst),
            1,
            "el listado tiene que resolver la página y el conteo en una sola transacción: dos \
             adquisiciones de conexión son dos consultas sueltas, y entre ellas cabría una \
             escritura que dejaría el total contando lo que la página no acota"
        );

        pool.close().await;
    }
}
