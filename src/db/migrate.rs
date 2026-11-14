//! El esquema, aplicado con migraciones embebidas en el binario.
//!
//! `sqlx::migrate!()` **no lee `migrations/` al arrancar**: la macro lee el
//! directorio **en tiempo de compilación** y mete su contenido en el binario. Dos
//! consecuencias que conviene tener presentes:
//!
//! * El directorio `migrations/` tiene que existir **antes del primer
//!   `cargo check`** que incluya la macro. Si falta, el fallo es de compilación y
//!   no de arranque, y quien no lo sepa buscará el error en el sitio equivocado.
//! * Un binario no necesita el directorio para arrancar: el esquema viaja dentro.
//!
//! Las migraciones aplicadas son **idempotentes**: `sqlx` lleva su tabla de
//! control (`_sqlx_migrations`), así que arrancar dos veces sobre la misma base no
//! vuelve a aplicar nada ni cambia el esquema.

use sqlx::SqlitePool;
use sqlx::migrate::Migrator;

use super::DbError;

/// El migrador embebido: el que lee `migrations/` al compilar.
pub fn migrador() -> Migrator {
    sqlx::migrate!()
}

/// Aplica el migrador embebido sobre el pool.
pub async fn migrar(pool: &SqlitePool) -> Result<(), DbError> {
    aplicar(pool, &migrador()).await
}

/// Aplica un migrador **dado** sobre el pool.
///
/// Se separa de [`migrar`] para poder ejercitar el camino de fallo con un
/// migrador que falla de verdad: el embebido está en verde por construcción.
///
/// Las migraciones corren con las **claves foráneas apagadas**, y eso es
/// deliberado: una migración que cambie un `CHECK` —lo único que SQLite no admite
/// con `ALTER TABLE`— tiene que **reconstruir** la tabla, y reconstruir pasa por un
/// `DROP TABLE` de la vieja. Con las claves encendidas ese `DROP` ejecuta el
/// `ON DELETE CASCADE` de las tablas hijas y se lleva sus filas por delante antes
/// de que la migración haya podido copiarlas: es la pérdida silenciosa que la `0003`
/// evita, y la evitaría cualquier reconstrucción futura.
///
/// El pragma va en la **conexión** y fuera de transacción, que es la única forma de
/// que surta efecto: `PRAGMA foreign_keys` es un no-op dentro de una, y `sqlx`
/// envuelve cada migración en una. La marca `-- no-transaction` del fichero **no**
/// sirve para esto: `sqlx-sqlite` 0.8.6 lee el flag en `sqlx-core` y no lo mira.
/// Apagarlo así —fuera de transacción y para la tanda entera— deja a la `0003`
/// reconstruir su tabla sin tocar las hijas.
///
/// Se vuelve a encender **siempre**, también cuando la migración ha fallado: la
/// conexión regresa al pool y la usa el motor, y una conexión del pool con las
/// claves apagadas es justo el silencio que [`opciones_del_dsn`] existe para evitar.
/// Y ya encendidas se comprueba que no quedaran filas huérfanas, que es lo único que
/// el apagado podía dejar pasar.
pub async fn aplicar(pool: &SqlitePool, migrator: &Migrator) -> Result<(), DbError> {
    let mut conexion = pool
        .acquire()
        .await
        .map_err(|causa| DbError::EsquemaNoAplicado { causa })?;

    pragma_de_la_conexion(&mut conexion, PRAGMA_SIN_CLAVES_FORANEAS).await?;

    // `run_direct` y no `run`: es el mismo camino —`run` solo resuelve el `Acquire` y
    // llama a éste— pero `run` es genérico sobre `Acquire`, y ese genérico deja al
    // futuro sin `Send` demostrable cuando esto se encadena dentro de un `tokio::spawn`
    // (el `implementation of Acquire is not general enough` que la propia `sqlx`
    // documenta como motivo de que `run_direct` exista). Con la conexión concreta, el
    // futuro es `Send` y el arranque puede montarse en él.
    let resultado = migrator
        .run_direct(&mut *conexion)
        .await
        .map_err(|causa| DbError::Migracion { causa });

    pragma_de_la_conexion(&mut conexion, PRAGMA_CON_CLAVES_FORANEAS).await?;

    resultado?;

    exigir_que_no_queden_huerfanos(&mut conexion).await
}

/// Apaga las claves foráneas de la conexión con la que se aplica el esquema.
///
/// Las dos sentencias son constantes del código y no se construyen con nada: el
/// pragma no admite un valor enlazado, y aquí no hay ningún valor que enlazar.
const PRAGMA_SIN_CLAVES_FORANEAS: &str = "PRAGMA foreign_keys = OFF";

/// Las vuelve a encender: es el estado en el que el pool entrega sus conexiones.
const PRAGMA_CON_CLAVES_FORANEAS: &str = "PRAGMA foreign_keys = ON";

/// Ejecuta un pragma sobre la conexión dada.
async fn pragma_de_la_conexion(
    conexion: &mut sqlx::SqliteConnection,
    pragma: &'static str,
) -> Result<(), DbError> {
    sqlx::query(pragma)
        .execute(conexion)
        .await
        .map(|_| ())
        .map_err(|causa| DbError::EsquemaNoAplicado { causa })
}

/// Exige que el esquema que dejaron las migraciones no tenga filas huérfanas.
///
/// Con las claves foráneas apagadas, una fila que apunte a una tabla que ya no está
/// entra sin que nada se queje, y el fallo aparecería más tarde —en un volcado, en
/// una cascada que ya no ocurre— hablando de otra cosa. Se usa la función de tabla
/// del pragma, que devuelve la tabla de cada huérfano y admite `DISTINCT`: el
/// mensaje nombra las tablas afectadas y no cada fila.
async fn exigir_que_no_queden_huerfanos(
    conexion: &mut sqlx::SqliteConnection,
) -> Result<(), DbError> {
    let tablas = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT \"table\" FROM pragma_foreign_key_check ORDER BY \"table\"",
    )
    .fetch_all(conexion)
    .await
    .map_err(|causa| DbError::EsquemaNoAplicado { causa })?;

    if tablas.is_empty() {
        return Ok(());
    }

    Err(DbError::HuerfanosTrasMigrar {
        tablas: tablas.join(", "),
    })
}

#[cfg(test)]
mod tests {
    // El texto de `EXPLAIN QUERY PLAN` —los literales `SCAN` y `SEARCH`, y el nombre
    // de los índices— **no es API estable** entre versiones de SQLite: un salto de
    // versión puede pedir retocar los literales que afirman los tests 1.15, 1.16 y
    // 1.17. El plan real medido va anotado junto a cada aserción.
    use std::borrow::Cow;

    use sqlx::Row;
    use sqlx::SqlitePool;
    use sqlx::migrate::Migrator;

    use crate::db::{preparar, preparar_con};
    use crate::test_support::{DirectorioDePrueba, config_con_dsn, directorio_de_prueba};

    /// Una base con el esquema ya aplicado, y su directorio temporal vivo.
    async fn base_migrada() -> (DirectorioDePrueba, SqlitePool) {
        let directorio = directorio_de_prueba();
        let pool = preparar(&config_con_dsn(&directorio.dsn()))
            .await
            .expect("preparar debe dejar el esquema aplicado");

        (directorio, pool)
    }

    /// El migrador embebido **recortado** a las migraciones hasta una versión.
    ///
    /// Es lo que permite llegar al estado anterior a la migración que se prueba:
    /// con el migrador entero no hay forma de poblar la base «con las filas de
    /// antes» sin retocar el esquema a mano, y entonces lo que se probaría no sería
    /// la migración. Las migraciones son las de verdad —el recorte solo filtra por
    /// versión—, así que el estado de partida es el que tendría una base en
    /// producción.
    fn migrador_hasta(version: i64) -> Migrator {
        let mut migraciones = super::migrador().migrations.into_owned();
        migraciones.retain(|migracion| migracion.version <= version);

        Migrator {
            migrations: Cow::Owned(migraciones),
            ..super::migrador()
        }
    }

    /// Una base llevada hasta una versión concreta de las migraciones.
    async fn base_hasta(version: i64) -> (DirectorioDePrueba, SqlitePool) {
        let directorio = directorio_de_prueba();
        let pool = preparar_con(&config_con_dsn(&directorio.dsn()), &migrador_hasta(version))
            .await
            .expect("preparar debe dejar el esquema hasta esa versión aplicado");

        (directorio, pool)
    }

    /// Columnas de una tabla, en el orden en que las declara el esquema.
    ///
    /// Se usa la función de tabla del PRAGMA y no `PRAGMA table_info(...)`: la
    /// primera acepta un valor enlazado y la segunda exigiría interpolar el nombre,
    /// que es justo lo que no se hace en este proyecto ni en una prueba.
    async fn columnas_de(pool: &SqlitePool, tabla: &str) -> Vec<String> {
        sqlx::query("SELECT name FROM pragma_table_info(?1)")
            .bind(tabla)
            .fetch_all(pool)
            .await
            .expect("la consulta de columnas no puede fallar")
            .iter()
            .map(|fila| fila.try_get::<String, _>("name").expect("name es texto"))
            .collect()
    }

    /// Una columna tal y como la declara el esquema, con sus **cuatro** campos.
    ///
    /// `name` solo no basta: el §3 fija además el tipo, la obligatoriedad y el
    /// valor por defecto, y un `keep_daily TEXT`, un `DEFAULT` perdido o un
    /// `enabled` sin `DEFAULT 1` pasarían inadvertidos comparando nombres.
    #[derive(Debug, PartialEq)]
    struct Columna {
        /// Nombre de la columna.
        nombre: String,
        /// Tipo declarado (`TEXT`, `INTEGER`, `BOOLEAN`, `DATETIME`).
        tipo: String,
        /// `NOT NULL` declarado.
        obligatoria: bool,
        /// Valor por defecto declarado, como lo devuelve el pragma.
        por_defecto: Option<String>,
    }

    /// Columna esperada del §3, para no repetir la construcción en cada tabla.
    fn columna(nombre: &str, tipo: &str, obligatoria: bool, por_defecto: Option<&str>) -> Columna {
        Columna {
            nombre: nombre.to_owned(),
            tipo: tipo.to_owned(),
            obligatoria,
            por_defecto: por_defecto.map(ToOwned::to_owned),
        }
    }

    /// Columnas de una tabla con **nombre, tipo, obligatoriedad y valor por defecto**.
    ///
    /// Hermana de [`columnas_de`]: aquélla sigue sirviendo para el test que solo
    /// mira nombres (1.14), y ésta es la que afirma el esquema entero.
    async fn columnas_completas_de(pool: &SqlitePool, tabla: &str) -> Vec<Columna> {
        sqlx::query("SELECT name, type, \"notnull\", dflt_value FROM pragma_table_info(?1)")
            .bind(tabla)
            .fetch_all(pool)
            .await
            .expect("la consulta de columnas no puede fallar")
            .iter()
            .map(|fila| Columna {
                nombre: fila.try_get::<String, _>("name").expect("name es texto"),
                tipo: fila.try_get::<String, _>("type").expect("type es texto"),
                obligatoria: fila
                    .try_get::<i64, _>("notnull")
                    .expect("notnull es un entero")
                    != 0,
                por_defecto: fila
                    .try_get::<Option<String>, _>("dflt_value")
                    .expect("dflt_value es texto o nulo"),
            })
            .collect()
    }

    /// El plan que SQLite usaría para una consulta, como texto.
    async fn plan_de(pool: &SqlitePool, consulta: &str) -> String {
        let filas = sqlx::query(consulta)
            .fetch_all(pool)
            .await
            .unwrap_or_else(|error| panic!("el plan de `{consulta}` no puede fallar: {error}"));

        assert!(
            !filas.is_empty(),
            "`{consulta}` no devolvió ninguna línea de plan"
        );

        filas
            .iter()
            .map(|fila| {
                fila.try_get::<String, _>("detail")
                    .expect("el plan trae la columna detail")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 1.11 — El esquema declara las cuatro tablas del §3 con sus columnas.
    ///
    /// Afirma los **cuatro** campos de cada columna —nombre, tipo, `NOT NULL` y
    /// valor por defecto— contra el §3, no solo el nombre: el escenario pide «las
    /// columnas, los tipos y los valores por defecto». Lo que se compara es el
    /// esquema que dejan **todas** las migraciones: el §3 más lo que añade la `0003`.
    #[tokio::test]
    async fn las_cuatro_tablas_tienen_las_columnas_del_srs() {
        let (_directorio, pool) = base_migrada().await;

        // Los valores por defecto llegan tal y como los declara el esquema:
        // `CURRENT_TIMESTAMP` sin paréntesis y los enteros como texto (`"7"`).
        let esperadas: [(&str, Vec<Columna>); 4] = [
            (
                "repositories",
                vec![
                    columna("id", "TEXT", true, None),
                    columna("name", "TEXT", true, None),
                    columna("endpoint", "TEXT", true, None),
                    columna("bucket", "TEXT", true, None),
                    columna("region", "TEXT", true, None),
                    columna("access_key_id", "TEXT", true, None),
                    columna("secret_access_key", "TEXT", true, None),
                    columna("passphrase", "TEXT", true, None),
                    columna("created_at", "DATETIME", true, Some("CURRENT_TIMESTAMP")),
                ],
            ),
            (
                "backup_targets",
                vec![
                    columna("id", "TEXT", true, None),
                    columna("name", "TEXT", true, None),
                    columna("target_type", "TEXT", true, None),
                    columna("container_name", "TEXT", false, None),
                    columna("db_user", "TEXT", false, None),
                    columna("db_password", "TEXT", false, None),
                    columna("db_name", "TEXT", false, None),
                    columna("host_path", "TEXT", false, None),
                    // La columna la añade la `0003` (issue #11): una fuente `sqlite`
                    // se describe con `container_name` + `db_path`, y el `db_path` va
                    // junto a `host_path`, que es la ruta de la otra fuente de fichero.
                    columna("db_path", "TEXT", false, None),
                    columna("created_at", "DATETIME", true, Some("CURRENT_TIMESTAMP")),
                ],
            ),
            (
                "backup_jobs",
                vec![
                    columna("id", "TEXT", true, None),
                    columna("target_id", "TEXT", true, None),
                    columna("repository_id", "TEXT", true, None),
                    columna("cron_expression", "TEXT", true, None),
                    columna("keep_daily", "INTEGER", true, Some("7")),
                    columna("keep_weekly", "INTEGER", true, Some("4")),
                    columna("keep_monthly", "INTEGER", true, Some("12")),
                    columna("enabled", "BOOLEAN", true, Some("1")),
                    columna("created_at", "DATETIME", true, Some("CURRENT_TIMESTAMP")),
                ],
            ),
            (
                "job_executions",
                vec![
                    columna("id", "TEXT", true, None),
                    columna("job_id", "TEXT", true, None),
                    columna("status", "TEXT", true, None),
                    columna("snapshot_id", "TEXT", false, None),
                    columna("bytes_processed", "INTEGER", true, Some("0")),
                    columna("bytes_uploaded", "INTEGER", true, Some("0")),
                    columna("error_message", "TEXT", false, None),
                    // `started_at` es `NOT NULL` **sin** valor por defecto: el §3 lo
                    // declara así y la capa de acceso lo informa al insertar.
                    columna("started_at", "DATETIME", true, None),
                    columna("finished_at", "DATETIME", false, None),
                ],
            ),
        ];

        for (tabla, columnas) in esperadas {
            let declaradas = columnas_completas_de(&pool, tabla).await;

            assert_eq!(
                declaradas, columnas,
                "las columnas de {tabla} no son las del §3 (nombre, tipo, NOT NULL o \
                 valor por defecto)"
            );
        }

        pool.close().await;
    }

    /// 1.12 — Los enumerados los garantiza un `CHECK` de la base.
    #[tokio::test]
    async fn los_enumerados_los_garantiza_un_check_de_la_base() {
        let (_directorio, pool) = base_migrada().await;

        // Un target válido entra: lo que se prueba abajo no falla por otra cosa.
        sqlx::query(
            "INSERT INTO backup_targets (id, name, target_type, host_path) \
             VALUES ('t-ok', 'carpeta', 'directory', '/srv/datos')",
        )
        .execute(&pool)
        .await
        .expect("un target de tipo 'directory' debe entrar");

        let fuera_de_enum = sqlx::query(
            "INSERT INTO backup_targets (id, name, target_type) VALUES ('t-mal', 'x', 'mysql')",
        )
        .execute(&pool)
        .await
        .expect_err("'mysql' no está en el enumerado de target_type");

        assert!(
            fuera_de_enum
                .to_string()
                .contains("CHECK constraint failed"),
            "el rechazo debe venir del CHECK de la base, no de otra cosa: {fuera_de_enum}"
        );

        // `sqlite` entró en el enumerado con la `0003` (issue #11), y entra **en la
        // base**: el valor tiene que pasarla sin que nadie lo valide antes. Su vecino
        // peligroso es `sqlite3`, que es el binario y no el tipo: si el enumerado se
        // ampliara por copia, ese valor colaría y el motor buscaría un tipo que no
        // existe. La pareja se afirma junta para que el límite quede fijado.
        sqlx::query(
            "INSERT INTO backup_targets (id, name, target_type, container_name, db_path) \
             VALUES ('t-sqlite', 'fichero', 'sqlite', 'ctr-1', '/data/kesto.db')",
        )
        .execute(&pool)
        .await
        .expect("'sqlite' tiene que estar en el enumerado de target_type");

        let sin_el_tres = sqlx::query(
            "INSERT INTO backup_targets (id, name, target_type) VALUES ('t-sqlite3', 'x', 'sqlite3')",
        )
        .execute(&pool)
        .await
        .expect_err("'sqlite3' es el binario, no un valor del enumerado de target_type");

        assert!(
            sin_el_tres.to_string().contains("CHECK constraint failed"),
            "el rechazo debe venir del CHECK de la base, no de otra cosa: {sin_el_tres}"
        );

        // Y lo mismo con el estado de una ejecución, sobre una tarea que sí existe.
        sqlx::query(
            "INSERT INTO repositories (id, name, endpoint, bucket, region, access_key_id, \
             secret_access_key, passphrase) VALUES ('r-ok', 'repo', 'e', 'b', 'r', 'a', 's', 'p')",
        )
        .execute(&pool)
        .await
        .expect("un repositorio debe entrar");

        sqlx::query(
            "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression) \
             VALUES ('j-ok', 't-ok', 'r-ok', '0 3 * * *')",
        )
        .execute(&pool)
        .await
        .expect("una tarea debe entrar");

        sqlx::query(
            "INSERT INTO job_executions (id, job_id, status, started_at) \
             VALUES ('e-ok', 'j-ok', 'pending', '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect("una ejecución con estado 'pending' debe entrar");

        let estado_fuera_de_enum = sqlx::query(
            "INSERT INTO job_executions (id, job_id, status, started_at) \
             VALUES ('e-mal', 'j-ok', 'cancelled', '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect_err("'cancelled' no está en el enumerado de status");

        assert!(
            estado_fuera_de_enum
                .to_string()
                .contains("CHECK constraint failed"),
            "el rechazo debe venir del CHECK de la base, no de otra cosa: {estado_fuera_de_enum}"
        );

        pool.close().await;
    }

    /// 1.13 — Los cuatro índices que el SRS no declara existen.
    #[tokio::test]
    async fn los_cuatro_indices_anadidos_al_srs_existen() {
        let (_directorio, pool) = base_migrada().await;

        let declarados = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .expect("la consulta de índices no puede fallar");

        for esperado in [
            "idx_backup_jobs_target_id",
            "idx_backup_jobs_repository_id",
            "idx_job_executions_job_id",
            "idx_job_executions_status_started_at",
        ] {
            assert!(
                declarados.iter().any(|indice| indice == esperado),
                "falta el índice {esperado}: {declarados:?}"
            );
        }

        pool.close().await;
    }

    /// 1.14 — `job_executions` no duplica el repositorio.
    #[tokio::test]
    async fn las_ejecuciones_no_tienen_columna_de_repositorio() {
        let (_directorio, pool) = base_migrada().await;

        let columnas = columnas_de(&pool, "job_executions").await;

        assert!(
            !columnas.iter().any(|columna| columna == "repository_id"),
            "job_executions no debe llevar repository_id: {columnas:?}"
        );

        pool.close().await;
    }

    /// 1.15 — Borrar una fila padre no recorre la tabla hija entera.
    ///
    /// Lo que se fija aquí es el **plan del `DELETE` real**: `EXPLAIN QUERY PLAN`
    /// **sí expande** los disparadores que implementan el `ON DELETE CASCADE` —está
    /// medido contra la SQLite embebida de este árbol, `libsqlite3-sys 0.30.1` →
    /// SQLite 3.46.0—, así que cada `DELETE` muestra, además del `SEARCH` de la fila
    /// padre, el `SEARCH` de la tabla hija **por el índice de la clave foránea**. Si
    /// el índice faltara, esa segunda línea sería un `SCAN` de la tabla hija y el
    /// borrado en cascada recorrería la tabla entera.
    ///
    /// Se afirman los tres niveles de la cascada, uno por cada fila padre. La
    /// cascada a **dos saltos** (`repositories` → `backup_jobs` → `job_executions`)
    /// **no** se expande entera: el plan del `DELETE` de un repositorio solo baja un
    /// nivel —`backup_jobs` por `idx_backup_jobs_repository_id`— y no menciona
    /// `job_executions`, porque SQLite resuelve cada disparador como una sentencia
    /// propia en el momento en que se dispara. Por eso aquí se afirma el nivel que el
    /// plan sí produce, y la cascada de extremo a extremo —que las filas
    /// desaparecen— la prueban las tareas 1.32 y 1.33.
    #[tokio::test]
    async fn el_borrado_de_una_fila_padre_no_recorre_la_tabla_hija() {
        let (_directorio, pool) = base_migrada().await;

        // Plan medido:
        //   SEARCH backup_targets USING INDEX sqlite_autoindex_backup_targets_1 (id=?)
        //   SEARCH backup_jobs    USING COVERING INDEX idx_backup_jobs_target_id (target_id=?)
        let plan_target = plan_de(
            &pool,
            "EXPLAIN QUERY PLAN DELETE FROM backup_targets WHERE id = 'x'",
        )
        .await;

        assert!(
            plan_target.contains("idx_backup_jobs_target_id"),
            "el plan del borrado de un target no usa el índice de la clave foránea: {plan_target}"
        );
        assert!(
            !plan_target.contains("SCAN backup_jobs"),
            "el borrado en cascada recorre backup_jobs entera: {plan_target}"
        );

        // Plan medido:
        //   SEARCH repositories USING INDEX sqlite_autoindex_repositories_1 (id=?)
        //   SEARCH backup_jobs  USING COVERING INDEX idx_backup_jobs_repository_id (repository_id=?)
        // El segundo salto (backup_jobs → job_executions) no aparece: ver el doc.
        let plan_repositorio = plan_de(
            &pool,
            "EXPLAIN QUERY PLAN DELETE FROM repositories WHERE id = 'x'",
        )
        .await;

        assert!(
            plan_repositorio.contains("idx_backup_jobs_repository_id"),
            "el plan del borrado de un repositorio no usa el índice de la clave foránea: \
             {plan_repositorio}"
        );
        assert!(
            !plan_repositorio.contains("SCAN backup_jobs"),
            "el borrado en cascada recorre backup_jobs entera: {plan_repositorio}"
        );

        // Plan medido:
        //   SEARCH backup_jobs    USING INDEX sqlite_autoindex_backup_jobs_1 (id=?)
        //   SEARCH job_executions USING COVERING INDEX idx_job_executions_job_id (job_id=?)
        let plan_tarea = plan_de(
            &pool,
            "EXPLAIN QUERY PLAN DELETE FROM backup_jobs WHERE id = 'x'",
        )
        .await;

        assert!(
            plan_tarea.contains("idx_job_executions_job_id"),
            "el plan del borrado de una tarea no usa el índice de la clave foránea: {plan_tarea}"
        );
        assert!(
            !plan_tarea.contains("SCAN job_executions"),
            "el borrado en cascada recorre job_executions entera: {plan_tarea}"
        );

        pool.close().await;
    }

    /// 1.16 — El listado de ejecuciones usa el índice de estado y fecha.
    #[tokio::test]
    async fn el_listado_de_ejecuciones_usa_su_indice() {
        let (_directorio, pool) = base_migrada().await;

        let plan = plan_de(
            &pool,
            "EXPLAIN QUERY PLAN SELECT id, status FROM job_executions \
             WHERE status = 'pending' ORDER BY started_at",
        )
        .await;

        assert!(
            plan.contains("idx_job_executions_status_started_at"),
            "el plan no usa el índice del listado: {plan}"
        );
        assert!(
            !plan.contains("SCAN job_executions"),
            "el listado recorre la tabla entera: {plan}"
        );

        pool.close().await;
    }

    /// 1.17 — Filtrar por repositorio se apoya en los índices de las claves foráneas.
    #[tokio::test]
    async fn el_filtro_por_repositorio_usa_los_indices_de_las_claves_foraneas() {
        let (_directorio, pool) = base_migrada().await;

        let plan = plan_de(
            &pool,
            "EXPLAIN QUERY PLAN SELECT e.id FROM job_executions e \
             JOIN backup_jobs j ON j.id = e.job_id WHERE j.repository_id = 'x'",
        )
        .await;

        assert!(
            plan.contains("idx_backup_jobs_repository_id"),
            "el plan no usa el índice del repositorio: {plan}"
        );
        assert!(
            plan.contains("idx_job_executions_job_id"),
            "el plan no usa el índice de la tarea en las ejecuciones: {plan}"
        );
        assert!(
            !plan.contains("SCAN job_executions"),
            "el plan recorre las ejecuciones enteras: {plan}"
        );
        assert!(
            !plan.contains("SCAN backup_jobs"),
            "el plan recorre las tareas enteras: {plan}"
        );

        pool.close().await;
    }

    /// 1.53 — La `0003` amplía el enumerado de `target_type` sin perder nada por el camino.
    ///
    /// SQLite **no** modifica un `CHECK` con `ALTER TABLE`: la migración
    /// **reconstruye** `backup_targets` —crear la nueva, copiar, borrar la vieja,
    /// renombrar—, y ese `DROP TABLE` de la vieja es lo que hay que vigilar. Con las
    /// claves foráneas encendidas ejecutaría el `ON DELETE CASCADE` de `backup_jobs`
    /// y se llevaría por delante sus filas —y las de `job_executions`, que cuelgan de
    /// ellas— antes de que la migración hubiera copiado nada. Por eso las migraciones
    /// corren con `foreign_keys` apagado (ver [`super::aplicar`]), y por eso la base
    /// se lleva **a mano** hasta la `0002` y se puebla **antes** de aplicar la `0003`:
    /// «las filas sobreviven» solo se puede afirmar sobre filas que existían de verdad.
    ///
    /// Las filas se escriben con un valor distinguible por columna, para que una
    /// copia con las columnas cruzadas no pase por buena.
    #[tokio::test]
    async fn la_migracion_0003_amplia_el_enum_sin_perder_filas_ni_indices() {
        let (_directorio, pool) = base_hasta(2).await;

        let filas = [
            (
                "t-maria",
                "maria",
                "mariadb",
                Some("maria-1"),
                Some("u-maria"),
                Some("p-maria"),
                Some("d-maria"),
                None,
            ),
            (
                "t-postgre",
                "postgre",
                "postgresql",
                Some("pg-1"),
                Some("u-pg"),
                Some("p-pg"),
                None,
                None,
            ),
            (
                "t-carpeta",
                "carpeta",
                "directory",
                None,
                None,
                None,
                None,
                Some("/srv/datos"),
            ),
        ];

        for (id, nombre, tipo, contenedor, usuario, contrasena, base, ruta) in filas {
            sqlx::query(
                "INSERT INTO backup_targets \
                 (id, name, target_type, container_name, db_user, db_password, db_name, host_path) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .bind(id)
            .bind(nombre)
            .bind(tipo)
            .bind(contenedor)
            .bind(usuario)
            .bind(contrasena)
            .bind(base)
            .bind(ruta)
            .execute(&pool)
            .await
            .expect("los targets previos a la migración deben entrar");
        }

        // Y una tarea sobre uno de ellos, con su ejecución: son las filas que el
        // `DROP` de la tabla vieja se llevaría por delante con la cascada encendida.
        sqlx::query(
            "INSERT INTO repositories (id, name, endpoint, bucket, region, access_key_id, \
             secret_access_key, passphrase) VALUES ('r-1', 'repo', 'e', 'b', 'r', 'a', 's', 'p')",
        )
        .execute(&pool)
        .await
        .expect("el repositorio debe entrar");

        sqlx::query(
            "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression) \
             VALUES ('j-1', 't-maria', 'r-1', '0 3 * * *')",
        )
        .execute(&pool)
        .await
        .expect("la tarea sobre el target previo debe entrar");

        sqlx::query(
            "INSERT INTO job_executions (id, job_id, status, started_at) \
             VALUES ('e-1', 'j-1', 'success', '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect("la ejecución de esa tarea debe entrar");

        // Aquí se aplica la migración que se prueba, sobre el estado de la `0002`.
        super::aplicar(&pool, &super::migrador())
            .await
            .expect("la 0003 debe aplicarse sobre una base que está en la 0002");

        let leidas: Vec<_> = sqlx::query(
            "SELECT id, name, target_type, container_name, db_user, db_password, db_name, \
             host_path, db_path FROM backup_targets ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .expect("las filas de backup_targets deben poder releerse")
        .iter()
        .map(|fila| {
            (
                fila.try_get::<String, _>("id").expect("id es texto"),
                fila.try_get::<String, _>("name").expect("name es texto"),
                fila.try_get::<String, _>("target_type")
                    .expect("target_type es texto"),
                fila.try_get::<Option<String>, _>("container_name")
                    .expect("container_name es texto o nulo"),
                fila.try_get::<Option<String>, _>("db_user")
                    .expect("db_user es texto o nulo"),
                fila.try_get::<Option<String>, _>("db_password")
                    .expect("db_password es texto o nulo"),
                fila.try_get::<Option<String>, _>("db_name")
                    .expect("db_name es texto o nulo"),
                fila.try_get::<Option<String>, _>("host_path")
                    .expect("host_path es texto o nulo"),
                fila.try_get::<Option<String>, _>("db_path")
                    .expect("db_path es texto o nulo"),
            )
        })
        .collect();

        let esperadas = [
            (
                "t-carpeta",
                "carpeta",
                "directory",
                None,
                None,
                None,
                None,
                Some("/srv/datos"),
                None,
            ),
            (
                "t-maria",
                "maria",
                "mariadb",
                Some("maria-1"),
                Some("u-maria"),
                Some("p-maria"),
                Some("d-maria"),
                None,
                None,
            ),
            (
                "t-postgre",
                "postgre",
                "postgresql",
                Some("pg-1"),
                Some("u-pg"),
                Some("p-pg"),
                None,
                None,
                None,
            ),
        ]
        .map(
            |(id, nombre, tipo, contenedor, usuario, contrasena, base, ruta, fichero)| {
                (
                    id.to_owned(),
                    nombre.to_owned(),
                    tipo.to_owned(),
                    contenedor.map(str::to_owned),
                    usuario.map(str::to_owned),
                    contrasena.map(str::to_owned),
                    base.map(str::to_owned),
                    ruta.map(str::to_owned),
                    fichero.map(str::to_owned),
                )
            },
        );

        assert_eq!(
            leidas,
            esperadas.to_vec(),
            "la reconstrucción de backup_targets perdió o mezcló filas"
        );

        // Las de las tablas que cuelgan de ella: el escenario que el `DROP` pone en
        // juego. Sin las claves foráneas apagadas durante la migración, aquí habría
        // ceros.
        for (consulta, esperado) in [
            ("SELECT count(*) FROM backup_jobs", "la tarea"),
            ("SELECT count(*) FROM job_executions", "la ejecución"),
        ] {
            let cuantas = sqlx::query_scalar::<_, i64>(consulta)
                .fetch_one(&pool)
                .await
                .expect("el recuento no puede fallar");

            assert_eq!(
                cuantas, 1,
                "la cascada se llevó {esperado} por delante durante la reconstrucción"
            );
        }

        // El valor nuevo entra y el de al lado no: `sqlite` es el tipo, `sqlite3` es
        // el binario. Se afirma **después** de reconstruir, que es donde el `CHECK`
        // nuevo tiene que estar en vigor.
        sqlx::query(
            "INSERT INTO backup_targets (id, name, target_type, container_name, db_path) \
             VALUES ('t-sqlite', 'fichero', 'sqlite', 'ctr-1', '/data/kesto.db')",
        )
        .execute(&pool)
        .await
        .expect("tras la 0003, 'sqlite' tiene que entrar");

        let sin_el_tres = sqlx::query(
            "INSERT INTO backup_targets (id, name, target_type) VALUES ('t-sqlite3', 'x', 'sqlite3')",
        )
        .execute(&pool)
        .await
        .expect_err("'sqlite3' es el binario, no un valor del enumerado");

        assert!(
            sin_el_tres.to_string().contains("CHECK constraint failed"),
            "el rechazo debe venir del CHECK de la base: {sin_el_tres}"
        );

        // La columna nueva no se queda a medias en las filas viejas: `NULL` es «este
        // target no es de tipo fichero», y la cadena vacía sería otra cosa.
        let con_fichero = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_targets WHERE target_type = 'sqlite' AND db_path IS NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("el recuento no puede fallar");

        assert_eq!(con_fichero, 0, "el target sqlite quedó sin db_path");

        // La clave foránea de `backup_jobs` sigue apuntando a la tabla **viva**: si la
        // reconstrucción se hubiera dejado la tabla con el nombre temporal, aquí
        // aparecería. Y la cascada del §3 sigue ocurriendo, que es la otra mitad.
        let apuntadas = sqlx::query_scalar::<_, String>(
            "SELECT \"table\" FROM pragma_foreign_key_list('backup_jobs')",
        )
        .fetch_all(&pool)
        .await
        .expect("las claves foráneas de la tarea deben poder leerse");

        assert!(
            apuntadas.iter().any(|tabla| tabla == "backup_targets"),
            "la clave foránea de la tarea no apunta a backup_targets: {apuntadas:?}"
        );
        assert!(
            !apuntadas.iter().any(|tabla| tabla.ends_with("_nueva")),
            "la clave foránea quedó apuntando a la tabla temporal: {apuntadas:?}"
        );

        sqlx::query("DELETE FROM backup_targets WHERE id = 't-maria'")
            .execute(&pool)
            .await
            .expect("borrar el target no puede fallar");

        let tareas = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect("el recuento no puede fallar");
        let ejecuciones = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM job_executions")
            .fetch_one(&pool)
            .await
            .expect("el recuento no puede fallar");

        assert_eq!(
            (tareas, ejecuciones),
            (0, 0),
            "la cascada del §3 dejó de ocurrir tras reconstruir la tabla"
        );

        // Los índices añadidos al SRS siguen ahí —los de las tablas hijas no se tocan,
        // y el de `backup_targets` es el implícito de su clave primaria—, y no queda
        // ninguna tabla temporal en el esquema. Se afirman los **nombres concretos** y no
        // solo que haya alguno: la reconstrucción tiene que dejar de vuelta los tres
        // índices del SRS, y un `!is_empty()` daría por bueno un índice suelto.
        for (tabla, esperados) in [
            (
                "backup_jobs",
                ["idx_backup_jobs_target_id", "idx_backup_jobs_repository_id"],
            ),
            (
                "job_executions",
                [
                    "idx_job_executions_job_id",
                    "idx_job_executions_status_started_at",
                ],
            ),
        ] {
            let indices = sqlx::query_scalar::<_, String>(
                "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = ?1 \
                 AND name LIKE 'idx_%' ORDER BY name",
            )
            .bind(tabla)
            .fetch_all(&pool)
            .await
            .expect("los índices deben poder consultarse");

            for esperado in esperados {
                assert!(
                    indices.iter().any(|indice| indice == esperado),
                    "la reconstrucción se dejó {esperado} de {tabla} por el camino: {indices:?}"
                );
            }
        }

        let tablas = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .expect("las tablas deben poder consultarse");

        assert!(
            !tablas.iter().any(|tabla| tabla.ends_with("_nueva")),
            "la migración dejó la tabla temporal en el esquema: {tablas:?}"
        );

        pool.close().await;
    }
}
