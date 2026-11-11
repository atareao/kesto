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
pub async fn aplicar(pool: &SqlitePool, migrator: &Migrator) -> Result<(), DbError> {
    migrator
        .run(pool)
        .await
        .map_err(|causa| DbError::Migracion { causa })
}

#[cfg(test)]
mod tests {
    // El texto de `EXPLAIN QUERY PLAN` —los literales `SCAN` y `SEARCH`, y el nombre
    // de los índices— **no es API estable** entre versiones de SQLite: un salto de
    // versión puede pedir retocar los literales que afirman los tests 1.15, 1.16 y
    // 1.17. El plan real medido va anotado junto a cada aserción.
    use sqlx::Row;
    use sqlx::SqlitePool;

    use crate::db::preparar;
    use crate::test_support::{config_con_dsn, directorio_de_prueba};

    /// Una base con el esquema ya aplicado, y su directorio temporal vivo.
    async fn base_migrada() -> (crate::test_support::DirectorioDePrueba, SqlitePool) {
        let directorio = directorio_de_prueba();
        let pool = preparar(&config_con_dsn(&directorio.dsn()))
            .await
            .expect("preparar debe dejar el esquema aplicado");

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

    /// 1.11 — `0001_init.sql` declara las cuatro tablas con sus columnas.
    ///
    /// Afirma los **cuatro** campos de cada columna —nombre, tipo, `NOT NULL` y
    /// valor por defecto— contra el §3, no solo el nombre: el escenario pide «las
    /// columnas, los tipos y los valores por defecto».
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
}
