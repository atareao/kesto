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
/// Si ese reencendido fallara —el único punto que podía devolver una conexión
/// envenenada—, la conexión se **descarta** en vez de devolverla al pool, y el error
/// de la migración se propaga por delante del del pragma. Y ya encendidas se comprueba
/// que no quedaran filas huérfanas, que es lo único que el apagado podía dejar pasar.
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

    // El pragma de vuelta se intenta **siempre**, y su fallo **no** se propaga con un `?`: un `?`
    // aquí devolvería antes que el error de la migración —tapándolo con un fallo de restauración— y,
    // sobre todo, dejaría la conexión de regreso en el pool con las claves **apagadas**, que es el
    // silencio que [`opciones_del_dsn`] existe para evitar. Cuando el pragma falla no hay forma de
    // reencender esa conexión, así que se **descarta**: `close` la saca del pool en vez de
    // devolverla, y la siguiente que el pool abra nace con `foreign_keys` encendido, que va en las
    // opciones de conexión. El apagado, así, no sobrevive a esta función.
    if let Err(causa) = pragma_de_la_conexion(&mut conexion, PRAGMA_CON_CLAVES_FORANEAS).await {
        tracing::error!(
            %causa,
            "no se pudo reencender `foreign_keys`; la conexión se descarta para que no vuelva al \
             pool con las claves apagadas"
        );

        if let Err(error) = conexion.close().await {
            tracing::error!(%error, "tampoco se pudo cerrar la conexión envenenada");
        }

        // El error de la migración manda: `and` deja pasar el suyo y solo estrena el del pragma
        // cuando la migración fue bien, que es el orden en que se leen las causas.
        return resultado.and(Err(causa));
    }

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

    use crate::db::{abrir, preparar, preparar_con};
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
    /// esquema que dejan **todas** las migraciones: el §3 más lo que añaden la `0003`
    /// (`db_path`), la `0005` (`backup_jobs.name`) y la `0008` (la salud de
    /// `repositories`).
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
                    // Las tres columnas de salud las añade la `0008` (issue #111): van
                    // **detrás** de `created_at`, que es donde las deja el `ALTER
                    // TABLE ... ADD COLUMN`, y las tres son anulables y sin valor por
                    // defecto —el `NULL` de `health` es el estado «desconocido»—.
                    columna("health", "TEXT", false, None),
                    columna("health_checked_at", "DATETIME", false, None),
                    columna("health_detail", "TEXT", false, None),
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
                    // La columna la añade la `0005` (issue #65) **sin** reconstruir la tabla, y la
                    // reconstruye la `0007` para hacerla obligatoria y única: sigue en la última
                    // posición, porque el orden declarado no cambió, pero deja de ser anulable.
                    columna("name", "TEXT", true, None),
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

    /// 1.1 (db) — La `0005` añade `backup_jobs.name`, `TEXT` y anulable, **sin reconstruir** la
    /// tabla.
    ///
    /// Se parte de una base llevada hasta la `0004` —el estado de una instalación anterior a este
    /// change— y **con tareas dentro**, y se aplica el migrador **hasta la `0005`**. Lo que se mide
    /// es lo que distingue un `ALTER TABLE ... ADD COLUMN` de una reconstrucción: que el fichero
    /// entre, que deje la columna declarada con su tipo y su anulabilidad, y que las filas que ya
    /// había **sigan ahí** con `name` a `NULL`. Una reconstrucción —`CREATE`/`INSERT SELECT`/`DROP`/
    /// `RENAME`— que se equivocara se llevaría las tareas por delante, y eso no lo ve una prueba
    /// que solo mire el directorio de migraciones.
    #[tokio::test]
    async fn la_0005_anade_el_nombre_sin_reconstruir_backup_jobs() {
        let (_directorio, pool) = base_hasta(4).await;

        // Una instalación anterior con **dos** tareas: son las filas que tienen que sobrevivir a la
        // migración, y las dos sin nombre, que es el estado en el que quedan las tareas que ya
        // existían.
        for sentencia in [
            "INSERT INTO repositories (id, name, endpoint, bucket, region, access_key_id, \
             secret_access_key, passphrase) VALUES ('r-1', 'repo', 'e', 'b', 'r', 'a', 's', 'p')",
            "INSERT INTO backup_targets (id, name, target_type, host_path) \
             VALUES ('t-1', 'carpeta', 'directory', '/srv/datos')",
            "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression) \
             VALUES ('j-1', 't-1', 'r-1', '0 3 * * *')",
            "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression) \
             VALUES ('j-2', 't-1', 'r-1', '0 4 * * *')",
        ] {
            sqlx::query(sentencia)
                .execute(&pool)
                .await
                .expect("la siembra de la prueba del nombre no puede fallar");
        }

        let antes = columnas_de(&pool, "backup_jobs").await;
        assert!(
            !antes.iter().any(|columna| columna == "name"),
            "la columna `name` no puede existir antes de la `0005`: {antes:?}"
        );

        // La migración, sobre esa misma base con las filas dentro. Se aplica **solo** hasta la
        // `0005`: el estado que se mide es el que deja esa migración, y el migrador entero llegaría
        // hasta la `0007`, que es la que después vuelve el nombre obligatorio y único.
        super::aplicar(&pool, &migrador_hasta(5))
            .await
            .expect("la `0005` tiene que aplicarse sobre una base llevada hasta la `0004`");

        let declaradas = columnas_completas_de(&pool, "backup_jobs").await;
        let nombre = declaradas
            .iter()
            .find(|columna| columna.nombre == "name")
            .unwrap_or_else(|| panic!("la `0005` no dejó la columna `name`: {declaradas:?}"));

        assert_eq!(nombre.tipo, "TEXT", "`name` tiene que ser de tipo TEXT");
        assert!(!nombre.obligatoria, "`name` tiene que admitir NULL");
        assert_eq!(
            nombre.por_defecto, None,
            "`name` NO tiene valor por defecto"
        );

        // Y las filas de antes siguen ahí, sin nombre: la tabla no se reconstruyó.
        let tareas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect("la tabla tiene que poder contarse");

        assert_eq!(
            tareas, 2,
            "la migración se llevó tareas por delante: quedaron {tareas}"
        );

        let sin_nombre: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs WHERE name IS NULL")
                .fetch_one(&pool)
                .await
                .expect("la columna `name` tiene que poder consultarse");

        assert_eq!(
            sin_nombre, 2,
            "las tareas anteriores al change tienen que quedar con `name` a NULL"
        );

        pool.close().await;
    }

    /// 1.1 (db — salud por repositorio) — La `0008` añade las tres columnas de salud a
    /// `repositories`, con su `CHECK`, y **no toca las filas** que ya había.
    ///
    /// Es el mismo patrón que la 1.1 de la `0005`: se parte del estado de una instalación anterior a
    /// este change —la base llevada hasta la `0007`— y **con una fila dentro**, y se aplica el
    /// migrador sobre ella. Lo que se mide no es que el directorio tenga un fichero más: es que la
    /// fila siga ahí con sus valores intactos y su salud a `NULL` —el «desconocido»—, que las tres
    /// columnas queden declaradas con su tipo y su anulabilidad, y que el `CHECK` del enumerado esté
    /// **en la base**: los cinco estados del contrato entran y un sexto no.
    #[tokio::test]
    async fn la_0008_anade_la_salud_sin_tocar_las_filas() {
        let (_directorio, pool) = base_hasta(7).await;

        // Una instalación anterior a este change, con su fila: es la que tiene que sobrevivir. Los
        // secretos van como texto suelto —el sobre cifrado no le importa a esta prueba—, pero
        // distinguible por columna, para que una copia con las columnas cruzadas no pase por buena.
        sqlx::query(
            "INSERT INTO repositories (id, name, endpoint, bucket, region, access_key_id, \
             secret_access_key, passphrase, created_at) \
             VALUES ('11111111-1111-1111-1111-111111111111', 'repo-de-antes', \
             'https://s3.example.test/antes', 'cubo-de-antes', 'eu-west-1', 'AKIA-DE-ANTES', \
             'sobre-del-secreto', 'sobre-de-la-passphrase', '2026-01-02 03:04:05')",
        )
        .execute(&pool)
        .await
        .expect("la fila anterior a la migración tiene que entrar");

        let antes = columnas_de(&pool, "repositories").await;

        for columna in ["health", "health_checked_at", "health_detail"] {
            assert!(
                !antes.iter().any(|declarada| declarada == columna),
                "`{columna}` no puede existir antes de la `0008`: {antes:?}"
            );
        }

        // La migración, sobre esa misma base con la fila dentro.
        super::aplicar(&pool, &super::migrador())
            .await
            .expect("la 0008 tiene que aplicarse sobre una base llevada hasta la 0007");

        // 1. Las tres columnas, declaradas como contrato: texto y fecha, y las tres anulables y sin
        // valor por defecto —una fila anterior entra sin que nadie escriba la salud—.
        let declaradas = columnas_completas_de(&pool, "repositories").await;

        for (nombre, tipo, obligatoria) in [
            ("health", "TEXT", false),
            ("health_checked_at", "DATETIME", false),
            ("health_detail", "TEXT", false),
        ] {
            let declarada = declaradas
                .iter()
                .find(|columna| columna.nombre == nombre)
                .unwrap_or_else(|| panic!("la `0008` no dejó la columna `{nombre}`"));

            assert_eq!(declarada.tipo, tipo, "`{nombre}` no es de tipo {tipo}");
            assert!(
                declarada.obligatoria == obligatoria,
                "`{nombre}` tiene que admitir NULL: el «desconocido» es un estado válido"
            );
            assert_eq!(
                declarada.por_defecto, None,
                "`{nombre}` NO tiene valor por defecto: la salud no se inventa por omisión"
            );
        }

        // 2. La fila de antes sigue ahí, con sus valores intactos y la salud a nulo.
        let fila = sqlx::query(
            "SELECT name, endpoint, bucket, region, access_key_id, secret_access_key, passphrase, \
             created_at, health, health_checked_at, health_detail FROM repositories WHERE id = ?1",
        )
        .bind("11111111-1111-1111-1111-111111111111")
        .fetch_one(&pool)
        .await
        .expect("la fila anterior tiene que seguir ahí tras la migración");

        for (columna, esperado) in [
            ("name", "repo-de-antes"),
            ("endpoint", "https://s3.example.test/antes"),
            ("bucket", "cubo-de-antes"),
            ("region", "eu-west-1"),
            ("access_key_id", "AKIA-DE-ANTES"),
            ("secret_access_key", "sobre-del-secreto"),
            ("passphrase", "sobre-de-la-passphrase"),
            ("created_at", "2026-01-02 03:04:05"),
        ] {
            let valor: String = fila
                .try_get(columna)
                .unwrap_or_else(|error| panic!("`{columna}` tiene que poder leerse: {error}"));

            assert_eq!(valor, esperado, "la migración tocó `{columna}`");
        }

        for columna in ["health", "health_checked_at", "health_detail"] {
            let valor: Option<String> = fila
                .try_get(columna)
                .unwrap_or_else(|error| panic!("`{columna}` tiene que poder leerse: {error}"));

            assert_eq!(
                valor, None,
                "la fila anterior a la `0008` tiene que quedar en «desconocido», no en `{columna}`"
            );
        }

        // 3. El `CHECK` está **en la base**: los cinco estados entran y un sexto no, sin que nadie
        // los valide antes. Es el contrato que la capa de acceso escribe —el nombre de la variante
        // en snake_case—, y aquí se fija con los literales, que son los del texto almacenado.
        for estado in [
            "ok",
            "sin_repositorio",
            "passphrase",
            "inaccesible",
            "sin_permiso",
        ] {
            sqlx::query("UPDATE repositories SET health = ?1 WHERE id = ?2")
                .bind(estado)
                .bind("11111111-1111-1111-1111-111111111111")
                .execute(&pool)
                .await
                .unwrap_or_else(|error| panic!("`{estado}` tiene que entrar en el CHECK: {error}"));
        }

        let fuera_de_enum =
            sqlx::query("UPDATE repositories SET health = 'desconocido' WHERE id = ?1")
                .bind("11111111-1111-1111-1111-111111111111")
                .execute(&pool)
                .await
                .expect_err("'desconocido' NO es un valor del enumerado: en la fila es NULL");

        assert!(
            fuera_de_enum
                .to_string()
                .contains("CHECK constraint failed"),
            "el rechazo tiene que venir del CHECK de la base, no de otra cosa: {fuera_de_enum}"
        );

        // Y el `NULL` del «desconocido» sí pasa: es lo que deja a la fila sin comprobar todavía.
        sqlx::query("UPDATE repositories SET health = NULL WHERE id = ?1")
            .bind("11111111-1111-1111-1111-111111111111")
            .execute(&pool)
            .await
            .expect("el NULL del «desconocido» tiene que entrar en el CHECK");

        pool.close().await;
    }

    /// El nombre del índice **único** que cubre una columna, si lo hay.
    ///
    /// `UNIQUE (name)` no se declara con un `CREATE INDEX` sino como restricción de columna, así que
    /// el índice que la base crea para ella es el **implícito** de la tabla
    /// (`sqlite_autoindex_backup_jobs_N`) y no lleva el prefijo `idx_`: [`indices_de_tabla`] no lo
    /// ve. Se recorre `pragma_index_list` y, de los índices únicos, se devuelve el que cubre
    /// **exactamente** la columna pedida —`id` es `PRIMARY KEY` y también estrena su implícito, así
    /// que «que haya algún índice único» no bastaría—.
    async fn indice_unico_sobre(pool: &SqlitePool, tabla: &str, columna: &str) -> Option<String> {
        let indices = sqlx::query("SELECT name, \"unique\" FROM pragma_index_list(?1)")
            .bind(tabla)
            .fetch_all(pool)
            .await
            .expect("los índices deben poder consultarse");

        for fila in &indices {
            let unico = fila
                .try_get::<i64, _>("unique")
                .expect("unique es un entero")
                != 0;

            if !unico {
                continue;
            }

            let nombre = fila.try_get::<String, _>("name").expect("name es texto");

            let columnas: Vec<String> = sqlx::query("SELECT name FROM pragma_index_info(?1)")
                .bind(&nombre)
                .fetch_all(pool)
                .await
                .expect("las columnas del índice deben poder consultarse")
                .iter()
                .map(|columna| columna.try_get::<String, _>("name").expect("name es texto"))
                .collect();

            if columnas == [columna.to_owned()] {
                return Some(nombre);
            }
        }

        None
    }

    /// 1.0 (db) — La `0007` hace el nombre **obligatorio y único** sin llevarse ninguna fila.
    ///
    /// Cubre `db`, requisito «La obligatoriedad y la unicidad del nombre reconstruyen `backup_jobs`,
    /// y la reconstrucción no se lleva ninguna fila», y sus escenarios. Se parte de una base llevada
    /// hasta la `0006` —el estado de una instalación anterior a este change— y **con filas dentro**:
    /// dos tareas sin nombre, una con él, y las ejecuciones colgando de ellas. La `0007` es la
    /// primera reconstrucción de una tabla **padre** de un histórico, así que lo que se mide no es
    /// solo el esquema final: es que el `DROP TABLE` **no** se llevó por delante las ejecuciones con
    /// las claves foráneas encendidas.
    #[tokio::test]
    async fn la_0007_hace_el_nombre_obligatorio_y_unico_sin_perder_filas() {
        let (_directorio, pool) = base_hasta(6).await;

        // Los padres de las tareas y las tareas: dos sin nombre y una con él —la que NO se puede
        // tocar—. Los identificadores tienen forma de UUID porque el §3 los declara así, y de ahí
        // sale el «octeto» del nombre derivado.
        let sin_nombre = [
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
        ];
        let con_nombre = "33333333-3333-3333-3333-333333333333";

        for sentencia in [
            "INSERT INTO repositories (id, name, endpoint, bucket, region, access_key_id, \
             secret_access_key, passphrase) VALUES ('r-1', 'repo', 'e', 'b', 'r', 'a', 's', 'p')",
            "INSERT INTO backup_targets (id, name, target_type, host_path) \
             VALUES ('t-1', 'carpeta', 'directory', '/srv/datos')",
        ] {
            sqlx::query(sentencia)
                .execute(&pool)
                .await
                .expect("la siembra de los padres no puede fallar");
        }

        for (indice, id) in sin_nombre.iter().copied().enumerate() {
            sqlx::query(
                "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression) \
                 VALUES (?1, 't-1', 'r-1', ?2)",
            )
            .bind(id)
            .bind(format!("0 {} * * *", indice + 3))
            .execute(&pool)
            .await
            .expect("las tareas sin nombre tienen que entrar antes de la 0007");
        }

        sqlx::query(
            "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression, name) \
             VALUES (?1, 't-1', 'r-1', '0 9 * * *', 'copia nocturna')",
        )
        .bind(con_nombre)
        .execute(&pool)
        .await
        .expect("la tarea con nombre tiene que entrar antes de la 0007");

        // Las ejecuciones: son las filas que el `DROP TABLE` de la vieja se llevaría por delante con
        // las claves foráneas encendidas. Una cuelga de cada tarea, la nombrada incluida.
        for (indice, id) in [sin_nombre[0], sin_nombre[1], con_nombre]
            .into_iter()
            .enumerate()
        {
            sqlx::query(
                "INSERT INTO job_executions (id, job_id, status, started_at) \
                 VALUES (?1, ?2, 'success', '2026-01-01T00:00:00Z')",
            )
            .bind(format!("e-{indice}"))
            .bind(id)
            .execute(&pool)
            .await
            .expect("las ejecuciones tienen que entrar antes de la 0007");
        }

        // La migración, sobre esa misma base con las filas dentro.
        super::aplicar(&pool, &super::migrador())
            .await
            .expect("la 0007 tiene que aplicarse sobre una base llevada hasta la 0006");

        // 1. La columna queda obligatoria, y con el índice único que la restricción crea.
        let declaradas = columnas_completas_de(&pool, "backup_jobs").await;
        let nombre = declaradas
            .iter()
            .find(|columna| columna.nombre == "name")
            .unwrap_or_else(|| panic!("la 0007 no dejó la columna `name`: {declaradas:?}"));

        assert_eq!(nombre.tipo, "TEXT", "`name` tiene que ser de tipo TEXT");
        assert!(nombre.obligatoria, "`name` tiene que ser NOT NULL");
        assert_eq!(
            nombre.por_defecto, None,
            "`name` NO tiene valor por defecto: el nombre no se inventa por omisión"
        );
        assert_eq!(
            declaradas.last().map(|columna| columna.nombre.as_str()),
            Some("name"),
            "`name` tiene que seguir en la **última** posición, como la dejó la 0005: {declaradas:?}"
        );

        assert!(
            indice_unico_sobre(&pool, "backup_jobs", "name")
                .await
                .is_some(),
            "la 0007 tiene que dejar un índice único sobre `name`"
        );

        // 2. Ninguna fila se perdió, y las dos sin nombre se rellenaron con el derivado.
        let filas = sqlx::query("SELECT id, name FROM backup_jobs ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("las tareas tienen que poder releerse");

        assert_eq!(
            filas.len(),
            3,
            "la reconstrucción se llevó tareas por delante"
        );

        let mut nombres = Vec::new();

        for fila in &filas {
            let id = fila.try_get::<String, _>("id").expect("id es texto");
            let nombre = fila
                .try_get::<Option<String>, _>("name")
                .expect("name es texto");
            let nombre = nombre.unwrap_or_else(|| panic!("la tarea `{id}` quedó sin nombre"));

            assert!(
                !nombre.trim().is_empty(),
                "el nombre de `{id}` quedó en blanco: {nombre:?}"
            );
            assert!(
                nombre.chars().count() <= 120,
                "el nombre derivado de `{id}` supera el tope: {nombre:?}"
            );
            assert_ne!(
                nombre, id,
                "el nombre derivado NO puede ser el identificador de la tarea"
            );

            if id == con_nombre {
                assert_eq!(
                    nombre, "copia nocturna",
                    "la fila que ya tenía nombre NO puede cambiar"
                );
            } else {
                let esperado = format!("tarea {}", &id[..8]);

                assert_eq!(
                    nombre, esperado,
                    "el nombre derivado tiene que ser `tarea <octeto corto del id>`: {nombre:?}"
                );
            }

            nombres.push(nombre);
        }

        nombres.sort();
        nombres.dedup();

        assert_eq!(
            nombres.len(),
            3,
            "los tres nombres tienen que ser distintos entre sí: {nombres:?}"
        );

        // 3. Las ejecuciones sobreviven, y ninguna quedó huérfana.
        let ejecuciones = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM job_executions")
            .fetch_one(&pool)
            .await
            .expect("las ejecuciones tienen que poder contarse");

        assert_eq!(
            ejecuciones, 3,
            "la reconstrucción se llevó ejecuciones por delante: quedaron {ejecuciones}"
        );

        let huerfanos = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT \"table\" FROM pragma_foreign_key_check ORDER BY \"table\"",
        )
        .fetch_all(&pool)
        .await
        .expect("la comprobación de huérfanos no puede fallar");

        assert!(
            huerfanos.is_empty(),
            "la reconstrucción dejó filas huérfanas en: {huerfanos:?}"
        );

        // 4. Los dos índices de la tabla, que el `DROP TABLE` se lleva, vuelven a estar.
        let indices = indices_de_tabla(&pool, "backup_jobs").await;

        for esperado in ["idx_backup_jobs_target_id", "idx_backup_jobs_repository_id"] {
            assert!(
                indices.iter().any(|indice| indice == esperado),
                "la reconstrucción se dejó {esperado} por el camino: {indices:?}"
            );
        }

        // 5. La restricción está **en la base**: sin nombre, con nombre repetido y en blanco choca
        // cada uno con su regla, sin depender de la aplicación.
        for (sentencia, motivo) in [
            (
                "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression) \
                 VALUES ('44444444-4444-4444-4444-444444444444', 't-1', 'r-1', '0 1 * * *')",
                "una fila sin `name` tiene que chocar con el NOT NULL",
            ),
            (
                "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression, name) \
                 VALUES ('55555555-5555-5555-5555-555555555555', 't-1', 'r-1', '0 1 * * *', \
                 'copia nocturna')",
                "un `name` repetido tiene que chocar con el UNIQUE",
            ),
            (
                "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression, name) \
                 VALUES ('66666666-6666-6666-6666-666666666666', 't-1', 'r-1', '0 1 * * *', '   ')",
                "un `name` en blanco tiene que chocar con el CHECK",
            ),
        ] {
            let rechazo = sqlx::query(sentencia)
                .execute(&pool)
                .await
                .expect_err(motivo);

            assert!(
                rechazo.to_string().contains("constraint failed"),
                "{motivo}, y el rechazo fue: {rechazo}"
            );
        }

        pool.close().await;
    }

    /// 1.0b (db) — El nombre derivado **no** choca con uno que ya estaba: ensancha el identificador.
    ///
    /// Cubre el escenario «El nombre derivado no choca con uno que ya estaba». Se prepara la
    /// colisión exacta: una tarea ya nombrada con el nombre derivado que le tocaría a otra tarea sin
    /// nombre. El derivado no puede repetirlo —el `UNIQUE` no lo admitiría—, así que la migración
    /// tiene que **ensanchar** el tramo del identificador hasta quedar libre; lo que se afirma es que
    /// los dos nombres siguen siendo distintos y que el que ya estaba no se toca.
    #[tokio::test]
    async fn el_nombre_derivado_no_choca_con_uno_que_ya_estaba() {
        let (_directorio, pool) = base_hasta(6).await;

        let sin_nombre = "11111111-1111-1111-1111-111111111111";
        let ocupado = "22222222-2222-2222-2222-222222222222";

        for sentencia in [
            "INSERT INTO repositories (id, name, endpoint, bucket, region, access_key_id, \
             secret_access_key, passphrase) VALUES ('r-1', 'repo', 'e', 'b', 'r', 'a', 's', 'p')",
            "INSERT INTO backup_targets (id, name, target_type, host_path) \
             VALUES ('t-1', 'carpeta', 'directory', '/srv/datos')",
        ] {
            sqlx::query(sentencia)
                .execute(&pool)
                .await
                .expect("la siembra de los padres no puede fallar");
        }

        // La tarea sin nombre, cuyo derivado de un octeto sería `tarea 11111111`.
        sqlx::query(
            "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression) \
             VALUES (?1, 't-1', 'r-1', '0 3 * * *')",
        )
        .bind(sin_nombre)
        .execute(&pool)
        .await
        .expect("la tarea sin nombre tiene que entrar");

        // Y otra tarea que **ya** se llama así: le roba el derivado de un octeto.
        sqlx::query(
            "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression, name) \
             VALUES (?1, 't-1', 'r-1', '0 4 * * *', ?2)",
        )
        .bind(ocupado)
        .bind(format!("tarea {}", &sin_nombre[..8]))
        .execute(&pool)
        .await
        .expect("la tarea que ocupa el derivado tiene que entrar");

        super::aplicar(&pool, &super::migrador())
            .await
            .expect("la 0007 tiene que aplicarse con el derivado ya ocupado");

        let derivado: String = sqlx::query_scalar("SELECT name FROM backup_jobs WHERE id = ?1")
            .bind(sin_nombre)
            .fetch_one(&pool)
            .await
            .expect("el nombre derivado tiene que poder leerse");

        let intacto: String = sqlx::query_scalar("SELECT name FROM backup_jobs WHERE id = ?1")
            .bind(ocupado)
            .fetch_one(&pool)
            .await
            .expect("el nombre que ya estaba tiene que poder leerse");

        assert_eq!(
            intacto,
            format!("tarea {}", &sin_nombre[..8]),
            "el nombre que ya estaba NO puede cambiar"
        );
        assert_ne!(
            derivado, intacto,
            "el derivado tiene que ensancharse para no repetir un nombre que ya estaba"
        );
        assert!(
            derivado.starts_with(&intacto),
            "el derivado ensanchado tiene que ser el siguiente tramo del mismo identificador: \
             {derivado:?} sobre {intacto:?}"
        );
        assert!(
            derivado.len() > intacto.len(),
            "ensanchar el tramo tiene que dar un nombre más largo: {derivado:?}"
        );

        pool.close().await;
    }

    /// Los dos padres que `backup_jobs` necesita para no ser huérfana.
    async fn padres_de_la_tarea(pool: &SqlitePool) {
        for sentencia in [
            "INSERT INTO repositories (id, name, endpoint, bucket, region, access_key_id, \
             secret_access_key, passphrase) VALUES ('r-1', 'repo', 'e', 'b', 'r', 'a', 's', 'p')",
            "INSERT INTO backup_targets (id, name, target_type, host_path) \
             VALUES ('t-1', 'carpeta', 'directory', '/srv/datos')",
        ] {
            sqlx::query(sentencia)
                .execute(pool)
                .await
                .expect("la siembra de los padres no puede fallar");
        }
    }

    /// Da de alta una tarea **por SQL directo**, con el nombre que se le pase.
    ///
    /// Es la única forma de llegar al estado previo a la `0007` con nombres que la aplicación
    /// vieja no habría escrito —dos homónimos, un blanco—: el borde los admite hoy porque la
    /// columna es anulable y no tiene `UNIQUE`.
    async fn sembrar_tarea(pool: &SqlitePool, id: &str, nombre: Option<&str>) {
        let consulta = match nombre {
            Some(_) => {
                "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression, name) \
                 VALUES (?1, 't-1', 'r-1', '0 3 * * *', ?2)"
            }
            None => {
                "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression) \
                 VALUES (?1, 't-1', 'r-1', '0 3 * * *')"
            }
        };

        let consulta = sqlx::query(consulta).bind(id);
        let consulta = match nombre {
            Some(nombre) => consulta.bind(nombre),
            None => consulta,
        };

        consulta
            .execute(pool)
            .await
            .expect("la siembra de la tarea no puede fallar");
    }

    /// Una ejecución que cuelga de la tarea dada.
    async fn sembrar_ejecucion(pool: &SqlitePool, id: &str, job_id: &str) {
        sqlx::query(
            "INSERT INTO job_executions (id, job_id, status, started_at) \
             VALUES (?1, ?2, 'success', '2026-01-01T00:00:00Z')",
        )
        .bind(id)
        .bind(job_id)
        .execute(pool)
        .await
        .expect("la siembra de la ejecución no puede fallar");
    }

    /// El nombre que quedó en la tarea con ese identificador.
    async fn nombre_de(pool: &SqlitePool, id: &str) -> String {
        sqlx::query_scalar("SELECT name FROM backup_jobs WHERE id = ?1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap_or_else(|error| panic!("la tarea `{id}` tiene que seguir ahí: {error}"))
    }

    /// Afirma que las ejecuciones siguen ahí y que ninguna quedó huérfana.
    ///
    /// Es la mitad de la prueba 1.0 que el `DROP TABLE` de una tabla **padre** pone en juego, y la
    /// que los escenarios nuevos —homónimos y blancos— tienen que seguir cumpliendo.
    async fn exigir_ejecuciones_intactas(pool: &SqlitePool, esperadas: i64) {
        let ejecuciones = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM job_executions")
            .fetch_one(pool)
            .await
            .expect("las ejecuciones tienen que poder contarse");

        assert_eq!(
            ejecuciones, esperadas,
            "la reconstrucción se llevó ejecuciones por delante: quedaron {ejecuciones}"
        );

        let huerfanos = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT \"table\" FROM pragma_foreign_key_check ORDER BY \"table\"",
        )
        .fetch_all(pool)
        .await
        .expect("la comprobación de huérfanos no puede fallar");

        assert!(
            huerfanos.is_empty(),
            "la reconstrucción dejó filas huérfanas en: {huerfanos:?}"
        );
    }

    /// 1.0c (db) — La `0007` **desambigua** los nombres repetidos antes de exigir la unicidad.
    ///
    /// Cubre el escenario «Los nombres repetidos se desambiguan antes de exigir la unicidad». El
    /// contrato anterior era explícitamente **no único** —el borde viejo no comprobaba
    /// duplicados—, así que una instalación real puede tener dos tareas llamadas igual, y el
    /// `INSERT ... SELECT` de la reconstrucción chocaría entonces contra el `UNIQUE(name)` y
    /// **abortaría la migración entera**: Kesto no arrancaría, y sin ninguna pantalla donde
    /// arreglarlo, porque el arranque falla antes de servir.
    ///
    /// Lo que se mide: de las tres homónimas, la de **menor `rowid`** —la primera sembrada—
    /// conserva su nombre y las otras dos reciben el sufijo ` (N)` avanzando hasta dar con uno
    /// libre en **toda** la tabla; aquí ` (2)` ya está ocupado por una cuarta tarea, así que les
    /// toca ` (3)` y ` (4)`. La cuarta conserva el suyo. Y las ejecuciones de las cuatro siguen
    /// ahí: el `DROP TABLE` de la tabla padre no se llevó ninguna.
    #[tokio::test]
    async fn la_0007_desambigua_los_nombres_repetidos_antes_de_exigir_la_unicidad() {
        let (_directorio, pool) = base_hasta(6).await;
        padres_de_la_tarea(&pool).await;

        // Las tres homónimas, en orden de `rowid`: la primera es la que conserva el nombre.
        let homonimas = [
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
            "33333333-3333-3333-3333-333333333333",
        ];
        let ocupado = "44444444-4444-4444-4444-444444444444";

        for id in homonimas {
            sembrar_tarea(&pool, id, Some("copia nocturna")).await;
        }

        // Y una cuarta que ya ocupa el primer sufijo: el desempate no puede contar el grupo, tiene
        // que mirar la tabla entera.
        sembrar_tarea(&pool, ocupado, Some("copia nocturna (2)")).await;

        for (indice, id) in homonimas.into_iter().chain([ocupado]).enumerate() {
            sembrar_ejecucion(&pool, &format!("e-{indice}"), id).await;
        }

        super::aplicar(&pool, &super::migrador())
            .await
            .expect("la 0007 tiene que desambiguar los homónimos en vez de abortar");

        assert_eq!(
            nombre_de(&pool, homonimas[0]).await,
            "copia nocturna",
            "la homónima de menor `rowid` tiene que conservar su nombre"
        );
        assert_eq!(
            nombre_de(&pool, homonimas[1]).await,
            "copia nocturna (3)",
            "la segunda homónima tiene que saltarse el sufijo que ya estaba ocupado"
        );
        assert_eq!(
            nombre_de(&pool, homonimas[2]).await,
            "copia nocturna (4)",
            "la tercera homónima tiene que tomar el siguiente sufijo libre"
        );
        assert_eq!(
            nombre_de(&pool, ocupado).await,
            "copia nocturna (2)",
            "la tarea que ya tenía el sufijo NO puede cambiar de nombre"
        );

        let distintos =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT name) FROM backup_jobs")
                .fetch_one(&pool)
                .await
                .expect("los nombres tienen que poder contarse");

        assert_eq!(distintos, 4, "los cuatro nombres tienen que ser distintos");

        exigir_ejecuciones_intactas(&pool, 4).await;

        pool.close().await;
    }

    /// 1.0d (db) — Un `name` vacío o en blanco se trata como **ausente**.
    ///
    /// Cubre el escenario «Un nombre vacío o en blanco se trata como ausente». La aplicación vieja
    /// convertía el blanco a `NULL` al guardar, así que un nombre así solo llega por SQL directo
    /// —una instalación retocada a mano—; pero si llega, choca contra el
    /// `CHECK (length(trim(name)) > 0)` de la tabla nueva y aborta la migración, igual que los
    /// homónimos. Se le da el nombre derivado que le tocaría si estuviera a `NULL`.
    #[tokio::test]
    async fn la_0007_da_nombre_derivado_a_un_nombre_en_blanco() {
        let (_directorio, pool) = base_hasta(6).await;
        padres_de_la_tarea(&pool).await;

        let vacio = "11111111-1111-1111-1111-111111111111";
        let espacios = "22222222-2222-2222-2222-222222222222";

        sembrar_tarea(&pool, vacio, Some("")).await;
        sembrar_tarea(&pool, espacios, Some("   ")).await;
        sembrar_ejecucion(&pool, "e-0", vacio).await;
        sembrar_ejecucion(&pool, "e-1", espacios).await;

        super::aplicar(&pool, &super::migrador())
            .await
            .expect("la 0007 tiene que tratar el blanco como ausente en vez de abortar");

        for (id, nombre) in [
            (vacio, nombre_de(&pool, vacio).await),
            (espacios, nombre_de(&pool, espacios).await),
        ] {
            assert!(
                !nombre.trim().is_empty(),
                "la tarea `{id}` quedó con el nombre en blanco: {nombre:?}"
            );
            assert_eq!(
                nombre,
                format!("tarea {}", &id[..8]),
                "un nombre en blanco tiene que recibir el mismo derivado que un `NULL`: {nombre:?}"
            );
        }

        exigir_ejecuciones_intactas(&pool, 2).await;

        pool.close().await;
    }

    /// 1.0e (db) — La desambiguación también funciona sobre un nombre que ya acaba en `(N)`.
    ///
    /// El sufijo es texto, no un contador que se pueda leer de la columna: un nombre que ya tiene
    /// forma de sufijado se desambigua **añadiendo** otro, sin reescribirlo ni confundirlo con un
    /// índice. Dos tareas llamadas `copia (2)` quedan en `copia (2)` y `copia (2) (2)`.
    #[tokio::test]
    async fn la_0007_desambigua_un_nombre_que_ya_acaba_en_parentesis() {
        let (_directorio, pool) = base_hasta(6).await;
        padres_de_la_tarea(&pool).await;

        let primera = "11111111-1111-1111-1111-111111111111";
        let segunda = "22222222-2222-2222-2222-222222222222";

        sembrar_tarea(&pool, primera, Some("copia (2)")).await;
        sembrar_tarea(&pool, segunda, Some("copia (2)")).await;
        sembrar_ejecucion(&pool, "e-0", primera).await;
        sembrar_ejecucion(&pool, "e-1", segunda).await;

        super::aplicar(&pool, &super::migrador())
            .await
            .expect("la 0007 tiene que desambiguar un nombre sufijado");

        assert_eq!(
            nombre_de(&pool, primera).await,
            "copia (2)",
            "la de menor `rowid` conserva su nombre tal cual"
        );
        assert_eq!(
            nombre_de(&pool, segunda).await,
            "copia (2) (2)",
            "el sufijo se añade al nombre tal y como estaba"
        );

        exigir_ejecuciones_intactas(&pool, 2).await;

        pool.close().await;
    }

    /// 1.0f (db) — La desambiguación aguanta un grupo grande de homónimos, saltando los huecos.
    ///
    /// Tres filas homónimas ya afinan el reparto, pero no el número: con **doce** hay que encadenar
    /// once sufijos y el reparto tiene que seguir siendo determinista y sin repetir ninguno, aunque
    /// uno de ellos —el ` (7)`— ya esté ocupado por otra tarea y quede un hueco en medio de la serie.
    #[tokio::test]
    async fn la_0007_desambigua_un_grupo_grande_de_homonimos() {
        let (_directorio, pool) = base_hasta(6).await;
        padres_de_la_tarea(&pool).await;

        let homonimas: Vec<String> = (0..12)
            .map(|indice| format!("{indice:08x}-1111-1111-1111-111111111111"))
            .collect();
        let ocupado = "ffffffff-2222-2222-2222-222222222222";

        for id in &homonimas {
            sembrar_tarea(&pool, id, Some("copia nocturna")).await;
        }

        sembrar_tarea(&pool, ocupado, Some("copia nocturna (7)")).await;

        for (indice, id) in homonimas
            .iter()
            .map(String::as_str)
            .chain([ocupado])
            .enumerate()
        {
            sembrar_ejecucion(&pool, &format!("e-{indice}"), id).await;
        }

        super::aplicar(&pool, &super::migrador())
            .await
            .expect("la 0007 tiene que desambiguar un grupo grande de homónimos");

        // La de menor `rowid` conserva el nombre y las demás toman los sufijos libres en orden: los
        // seis primeros, el ` (7)` se salta por estar ocupado, y sigue con el ` (8)`.
        let esperados = [
            "copia nocturna",
            "copia nocturna (2)",
            "copia nocturna (3)",
            "copia nocturna (4)",
            "copia nocturna (5)",
            "copia nocturna (6)",
            "copia nocturna (8)",
            "copia nocturna (9)",
            "copia nocturna (10)",
            "copia nocturna (11)",
            "copia nocturna (12)",
            "copia nocturna (13)",
        ];

        for (id, esperado) in homonimas.iter().zip(esperados) {
            assert_eq!(
                nombre_de(&pool, id).await,
                esperado,
                "el reparto de sufijos no es el determinista"
            );
        }

        assert_eq!(
            nombre_de(&pool, ocupado).await,
            "copia nocturna (7)",
            "la tarea que ya ocupaba un sufijo NO puede cambiar de nombre"
        );

        let distintos =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT name) FROM backup_jobs")
                .fetch_one(&pool)
                .await
                .expect("los nombres tienen que poder contarse");

        assert_eq!(
            distintos, 13,
            "los trece nombres tienen que ser distintos entre sí"
        );

        exigir_ejecuciones_intactas(&pool, 13).await;

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
            "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression, name) \
             VALUES ('j-ok', 't-ok', 'r-ok', '0 3 * * *', 'tarea-de-los-enumerados')",
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

    /// 1.54 — La `0004` añade el índice de `job_executions.snapshot_id`, y se usa.
    ///
    /// `snapshot_id` no es clave foránea de nada —es el identificador de la instantánea que la
    /// ejecución anotó—, así que se quedó fuera de los cuatro índices de la `0002`, y las dos
    /// consultas que resuelven sobre qué target cae una instantánea
    /// (`SQL_OBJETIVO_DE_LA_INSTANTANEA` y `SQL_OBJETIVOS_DE_LAS_INSTANTANEAS`, en
    /// `src/db/entities/execution.rs`) entran justo por esa columna y **además** ordenan por
    /// `started_at DESC, id DESC`, sobre la tabla que más crece: una fila por cada ejecución de
    /// cada copia.
    ///
    /// Lo que se mide, sin adornos:
    ///
    /// * **Sin estadísticas** —el estado de la base, que solo tiene `sqlite_stat1` si alguien
    ///   corre `ANALYZE`, y este árbol no lo corre— la consulta de **una** instantánea cambia de
    ///   plan y para bien: ya no tiene que entrar por el camino de las claves foráneas
    ///   (`idx_backup_jobs_repository_id` → `idx_job_executions_job_id`) y busca directamente las
    ///   ejecuciones de esa instantánea. La del **lote** no lo usa todavía: su `IN` sobre
    ///   `json_each` no le da un valor por el que buscar, y sin estadísticas nadie le ha dicho que
    ///   la clave sea única.
    /// * **Con estadísticas** —`ANALYZE` sobre un histórico con la forma de verdad, casi todas
    ///   las ejecuciones con su instantánea— las **dos** entran por el índice. Y la forma de la
    ///   columna importa: con una columna casi toda a `NULL`, `ANALYZE` mide muchos registros por
    ///   clave y el planificador prefiere el camino viejo. Por eso el histórico que se siembra
    ///   abajo deja la instantánea también en las ejecuciones que no la tienen, que son la
    ///   excepción —una copia que falló antes de anotarla—, y no al revés.
    ///
    /// El plan medido va anotado junto a cada aserción, como en 1.15, 1.16 y 1.17, y los
    /// literales de `EXPLAIN QUERY PLAN` no son API estable: si un salto de SQLite los cambia, lo
    /// que hay que rehacer es la medición, no la migración.
    #[tokio::test]
    async fn la_migracion_0004_indexa_el_snapshot_id() {
        // El texto literal de `SQL_OBJETIVO_DE_LA_INSTANTANEA` y de
        // `SQL_OBJETIVOS_DE_LAS_INSTANTANEAS`, la de una instantánea y la del lote: son
        // constantes privadas de `job_executions` y aquí se repiten igual que el 1.17 repite su
        // `JOIN`.
        const DE_UNA: &str = "SELECT e.job_id, t.id, t.target_type, t.host_path, t.name \
             FROM job_executions e \
             JOIN backup_jobs j ON j.id = e.job_id \
             JOIN backup_targets t ON t.id = j.target_id \
             WHERE e.snapshot_id = ?1 AND j.repository_id = ?2 \
             ORDER BY e.started_at DESC, e.id DESC LIMIT 1";

        const DE_LOTE: &str = "SELECT e.snapshot_id, e.job_id, t.id, t.target_type, t.host_path, \
             t.name FROM job_executions e \
             JOIN backup_jobs j ON j.id = e.job_id \
             JOIN backup_targets t ON t.id = j.target_id \
             WHERE e.snapshot_id IN (SELECT value FROM json_each(?1)) AND j.repository_id = ?2 \
             ORDER BY e.started_at DESC, e.id DESC";

        // El filtro **suelto** por la columna del índice: deja ver si el índice está donde tiene
        // que estar sin que el resto de la consulta lo estorbe.
        const SOLO_EL_FILTRO: &str = "SELECT e.id FROM job_executions e WHERE e.snapshot_id = ?1";

        // El estado de partida: una base con las migraciones hasta la `0003`, que es la que tiene
        // una instalación anterior a esta migración, y con histórico dentro.
        let (directorio, pool) = base_hasta(3).await;
        poblar_para_el_plan(&pool).await;

        let antes = indices_de_tabla(&pool, "job_executions").await;
        assert!(
            !antes
                .iter()
                .any(|indice| indice == "idx_job_executions_snapshot_id"),
            "el índice de la 0004 no puede existir en una base en la 0003: {antes:?}"
        );

        // Los planes del «antes», con las mismas filas que el «después». Medido:
        //   DE_UNA  SEARCH j USING INDEX idx_backup_jobs_repository_id (repository_id=?)
        //           SEARCH t USING INDEX sqlite_autoindex_backup_targets_1 (id=?)
        //           SEARCH e USING INDEX idx_job_executions_job_id (job_id=?)
        //   Soltó   SCAN e
        for (nombre, consulta) in [("la de una instantánea", DE_UNA), ("la del lote", DE_LOTE)] {
            let plan = plan_de(&pool, &format!("EXPLAIN QUERY PLAN {consulta}")).await;
            assert!(
                !plan.contains("idx_job_executions_snapshot_id"),
                "el plan de {nombre} usa un índice que la migración no ha creado: {plan}"
            );
            assert!(
                !plan.contains("SCAN job_executions"),
                "el plan de {nombre} recorre `job_executions` entera cuando debería entrar por la \
                 clave foránea de la tarea: {plan}"
            );
        }

        let suelto_sin_indice =
            plan_de(&pool, &format!("EXPLAIN QUERY PLAN {SOLO_EL_FILTRO}")).await;
        assert!(
            suelto_sin_indice.contains("SCAN e"),
            "sin índice, el filtro por `snapshot_id` debería recorrer la tabla: {suelto_sin_indice}"
        );

        // La migración, sobre esa misma base: el migrador entero continúa desde la versión 3 y
        // aplica la 0004 sin tocar nada más. Es lo que se comprueba de verdad —que el fichero
        // entra y deja el índice declarado—, y no que el directorio tenga uno más.
        super::aplicar(&pool, &super::migrador())
            .await
            .expect("la 0004 debe aplicarse sobre una base llevada hasta la 0003");

        let declarados = indices_de_tabla(&pool, "job_executions").await;
        assert!(
            declarados
                .iter()
                .any(|indice| indice == "idx_job_executions_snapshot_id"),
            "la 0004 se aplicó pero el índice no está declarado: {declarados:?}"
        );

        // El «después» se mide sobre un pool **recién abierto**, y no es un detalle de estilo:
        // SQLite decide el plan al preparar la sentencia y una conexión que ya tenía preparado
        // ese mismo texto sigue sirviendo el plan viejo hasta que el esquema cambie a **sus**
        // ojos —medido: midiendo el «después» sobre el pool que ya midió el «antes» sale otra vez
        // el plan del «antes»—. Prepararlo de cero es, además, lo que hace un proceso que arranca
        // contra una base ya migrada.
        let dsn = directorio.dsn();
        pool.close().await;
        let pool = abrir(&dsn)
            .await
            .expect("la base migrada debe poder reabrirse");

        // Sin estadísticas: la consulta de una instantánea entra por el índice nuevo —medido:
        // SEARCH e USING INDEX idx_job_executions_snapshot_id (snapshot_id=?), y después SEARCH j
        // y SEARCH t por sus claves primarias—, y el del lote sigue yendo por las claves
        // foráneas, porque sin `ANALYZE` nadie le ha dicho que la clave sea única.
        let de_una = plan_de(&pool, &format!("EXPLAIN QUERY PLAN {DE_UNA}")).await;
        assert!(
            de_una.contains("idx_job_executions_snapshot_id"),
            "el plan de la consulta de una instantánea no usa el índice de su columna: {de_una}"
        );

        let de_lote = plan_de(&pool, &format!("EXPLAIN QUERY PLAN {DE_LOTE}")).await;
        assert!(
            !de_lote.contains("idx_job_executions_snapshot_id"),
            "sin estadísticas, la consulta del lote no tiene por qué usar el índice: {de_lote}"
        );

        // Y el filtro suelto, que es el que no tenía otra puerta: de `SCAN e` a
        // `SEARCH e USING INDEX idx_job_executions_snapshot_id (snapshot_id=?)`.
        let suelto = plan_de(&pool, &format!("EXPLAIN QUERY PLAN {SOLO_EL_FILTRO}")).await;
        assert!(
            suelto.contains("idx_job_executions_snapshot_id"),
            "el filtro por `snapshot_id` no usa el índice de su columna: {suelto}"
        );
        assert!(
            !suelto.contains("SCAN e"),
            "el filtro por `snapshot_id` sigue recorriendo la tabla entera: {suelto}"
        );

        // Con estadísticas, las dos entran por el índice. Medido: DE_UNA queda en `SCAN t`,
        // `SEARCH e USING INDEX idx_job_executions_snapshot_id (snapshot_id=?)` y `SEARCH j` por su
        // clave primaria; DE_LOTE añade el `LIST SUBQUERY` del `json_each` con el mismo `SEARCH e`
        // por el índice. La `ANALYZE` cambia el esquema —crea `sqlite_stat1`—, así que la medición
        // vuelve a ir sobre un pool nuevo, por lo mismo de antes.
        sqlx::query("ANALYZE")
            .execute(&pool)
            .await
            .expect("`ANALYZE` sobre la base de prueba no puede fallar");

        pool.close().await;
        let pool = abrir(&dsn)
            .await
            .expect("la base con estadísticas debe poder reabrirse");

        for (nombre, consulta) in [("la de una instantánea", DE_UNA), ("la del lote", DE_LOTE)] {
            let plan = plan_de(&pool, &format!("EXPLAIN QUERY PLAN {consulta}")).await;
            assert!(
                plan.contains("idx_job_executions_snapshot_id"),
                "con estadísticas, el plan de {nombre} debería entrar por el índice nuevo: {plan}"
            );
        }

        // Una migración que solo crea un índice es idempotente por el `IF NOT EXISTS`, y volver a
        // aplicarla no puede fallar ni dejar el esquema distinto.
        super::aplicar(&pool, &super::migrador())
            .await
            .expect("reaplicar el migrador sobre una base ya migrada no puede fallar");

        assert_eq!(
            indices_de_tabla(&pool, "job_executions").await,
            declarados,
            "reaplicar la migración cambió los índices de la tabla"
        );

        pool.close().await;
    }

    /// Siembra la base de la prueba 1.54 con un histórico con la forma de la de verdad.
    ///
    /// Las cifras no son un adorno: el planificador decide con lo que **cree** saber de los
    /// tamaños de cada tabla, y sin estadísticas eso no son los datos. Con estadísticas sí, y de
    /// ahí salen las dos mitades que la prueba mide: `backup_jobs` con doscientas tareas,
    /// `job_executions` con dos mil ejecuciones y `backup_targets` con un solo target.
    ///
    /// La instantánea la lleva **casi** toda ejecución y el `NULL` es la excepción —una copia que
    /// falló antes de anotarla—, que es la forma de un histórico real. No es un detalle: con la
    /// columna al revés —casi toda a `NULL`— `ANALYZE` mide cientos de registros por clave,
    /// el índice parece inútil y el planificador se queda con el camino viejo; medido, y por eso
    /// la siembra va así.
    async fn poblar_para_el_plan(pool: &SqlitePool) {
        for sentencia in [
            "INSERT INTO repositories (id, name, endpoint, bucket, region, access_key_id, \
             secret_access_key, passphrase) \
             VALUES ('r-1', 'repo', 'e', 'b', 'r', 'a', 's', 'p')",
            "INSERT INTO backup_targets (id, name, target_type, host_path) \
             VALUES ('t-1', 'carpeta', 'directory', '/srv/datos')",
            "INSERT INTO backup_jobs (id, target_id, repository_id, cron_expression) \
             SELECT 'j-'||x, 't-1', 'r-1', '0 3 * * *' FROM (WITH RECURSIVE c(x) AS \
             (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 200) SELECT x FROM c)",
            "INSERT INTO job_executions (id, job_id, status, snapshot_id, started_at) \
             SELECT 'e-'||x, 'j-'||((x % 200)+1), 'success', \
             CASE WHEN x % 50 = 0 THEN NULL ELSE 'snap-'||x END, \
             datetime('now', '-'||x||' seconds') FROM (WITH RECURSIVE c(x) AS \
             (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 2000) SELECT x FROM c)",
        ] {
            sqlx::query(sentencia)
                .execute(pool)
                .await
                .expect("la siembra de la prueba 1.54 no puede fallar");
        }
    }

    /// Los índices declarados sobre una tabla, por nombre y ordenados.
    ///
    /// Hermana de la consulta suelta del 1.53: lo que se repite es el predicado
    /// —`type = 'index'`, `tbl_name`, `LIKE 'idx_%'` y el orden—, que es lo que hace comparables
    /// dos listados de tablas distintas.
    async fn indices_de_tabla(pool: &SqlitePool, tabla: &str) -> Vec<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = ?1 \
             AND name LIKE 'idx_%' ORDER BY name",
        )
        .bind(tabla)
        .fetch_all(pool)
        .await
        .expect("los índices deben poder consultarse")
    }
}
