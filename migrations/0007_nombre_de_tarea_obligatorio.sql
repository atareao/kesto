-- `backup_jobs.name` pasa a ser **obligatoria y única** (issues #87 y #89): el nombre es la
-- identidad legible de la tarea, y anulable obliga a cada pantalla a inventarse un sustituto —el
-- hueco en blanco de Tareas, el identificador de la tarea que cae en la columna «Tarea» de
-- Snapshots—, sustitutos que se contradicen entre sí.
--
-- SQLite **no** sabe reescribir con `ALTER TABLE` la nulabilidad de una columna ya declarada —la
-- `0005` la añadió anulable con un `ADD COLUMN`— ni añadirle una restricción de tabla a una que
-- existe, así que la tabla se **reconstruye**: crear la nueva, copiar, borrar la vieja y renombrar.
-- Es el procedimiento que ya usó la `0003` y el único camino que convierte «el nombre es
-- obligatorio y único» en una garantía de la base y no en una convención de la aplicación.
--
-- El `DROP TABLE` de la vieja es lo delicado, y aquí más que en la `0003`: `backup_jobs` es
-- **padre** de `job_executions`, así que con las claves foráneas encendidas ese `DROP` ejecutaría
-- su `ON DELETE CASCADE` y se llevaría por delante el histórico de copias de **todas** las tareas
-- antes de que esta migración hubiera copiado nada, sin que nada lo delatara. Por eso las
-- migraciones corren con `foreign_keys` **apagado**, y se apaga en la conexión y fuera de
-- transacción (`db::migrate::aplicar`): `PRAGMA foreign_keys` es un no-op dentro de una, y la marca
-- `-- no-transaction` no sirve —`sqlx-sqlite` 0.8.6 lee el flag en `sqlx-core` y no lo mira—.
-- `aplicar` lo vuelve a encender y comprueba después que no quedaran filas huérfanas.

-- 1) El relleno de las filas sin nombre, sobre la tabla **vieja**.
--
-- Se hace aquí, antes de reconstruir, y no en la copia, por una razón concreta: la tabla vieja
-- **no** tiene todavía el `UNIQUE`, así que un intento que se dejara un tramo a medias no aborta la
-- migración a mitad de camino. El nombre derivado es `tarea <octeto corto del id>` —el literal y el
-- primer octeto del identificador, catorce caracteres—, y **no** el identificador entero ni una
-- copia suya: eso sería exactamente el valor de la etiqueta de la instantánea, y volvería a dejar
-- «Tarea» y «Etiqueta» diciendo lo mismo.
--
-- Si ese primer tramo ya está ocupado —por un nombre que ya existía o por otra fila sin nombre que
-- produce el mismo octeto—, se ensancha al **siguiente tramo del mismo identificador**. Las dos
-- guardas de cada paso son las que hacen que el ensanchado termine: `NOT EXISTS` descarta los
-- tramos que ya son de alguien, y el `MIN(id)` deja que, de dos filas que comparten un tramo, solo
-- la del identificador menor se lo quede —la otra ensancha—, de modo que un paso nunca intenta
-- escribir dos veces el mismo nombre.

UPDATE backup_jobs
SET name = 'tarea ' || substr(id, 1, 8)
WHERE name IS NULL
  AND NOT EXISTS (
      SELECT 1
      FROM backup_jobs ocupado
      WHERE ocupado.name = 'tarea ' || substr(backup_jobs.id, 1, 8)
  )
  AND backup_jobs.id = (
      SELECT MIN(otra.id)
      FROM backup_jobs otra
      WHERE otra.name IS NULL
        AND substr(otra.id, 1, 8) = substr(backup_jobs.id, 1, 8)
  );

UPDATE backup_jobs
SET name = 'tarea ' || substr(id, 1, 16)
WHERE name IS NULL
  AND NOT EXISTS (
      SELECT 1
      FROM backup_jobs ocupado
      WHERE ocupado.name = 'tarea ' || substr(backup_jobs.id, 1, 16)
  )
  AND backup_jobs.id = (
      SELECT MIN(otra.id)
      FROM backup_jobs otra
      WHERE otra.name IS NULL
        AND substr(otra.id, 1, 16) = substr(backup_jobs.id, 1, 16)
  );

UPDATE backup_jobs
SET name = 'tarea ' || substr(id, 1, 24)
WHERE name IS NULL
  AND NOT EXISTS (
      SELECT 1
      FROM backup_jobs ocupado
      WHERE ocupado.name = 'tarea ' || substr(backup_jobs.id, 1, 24)
  )
  AND backup_jobs.id = (
      SELECT MIN(otra.id)
      FROM backup_jobs otra
      WHERE otra.name IS NULL
        AND substr(otra.id, 1, 24) = substr(backup_jobs.id, 1, 24)
  );

-- El identificador entero —los 36 caracteres con guiones de un UUIDv4—, que ya distingue cualquier
-- pareja de filas de la tabla: aquí se agota el ensanchado del tramo.
UPDATE backup_jobs
SET name = 'tarea ' || substr(id, 1, 36)
WHERE name IS NULL
  AND NOT EXISTS (
      SELECT 1
      FROM backup_jobs ocupado
      WHERE ocupado.name = 'tarea ' || substr(backup_jobs.id, 1, 36)
  )
  AND backup_jobs.id = (
      SELECT MIN(otra.id)
      FROM backup_jobs otra
      WHERE otra.name IS NULL
        AND substr(otra.id, 1, 36) = substr(backup_jobs.id, 1, 36)
  );

-- La red de seguridad del final: una fila cuyo identificador entero ya estuviera ocupado como
-- nombre —alguien que hubiera bautizado una tarea `tarea <uuid de otra>`— no tiene tramo siguiente
-- que probar. Se desempata con el `rowid`, que es único dentro de la tabla por construcción, para
-- que la fila quede con nombre en vez de quedarse a `NULL` y hacer fallar el `NOT NULL` de la
-- reconstrucción. Es un caso que la aplicación no puede producir; la línea está para que la
-- migración **termine siempre**, no para que alguien lo vea.
UPDATE backup_jobs
SET name = 'tarea ' || substr(id, 1, 8) || ' ' || rowid
WHERE name IS NULL
  AND NOT EXISTS (
      SELECT 1
      FROM backup_jobs ocupado
      WHERE ocupado.name = 'tarea ' || substr(backup_jobs.id, 1, 8) || ' ' || backup_jobs.rowid
  );

-- 2) La reconstrucción, ya con la columna en su forma final.
--
-- `name` va la **última**, como la dejó el `ADD COLUMN` de la `0005`: es el orden que publica
-- `PRAGMA table_info` y el que recorre la capa de acceso, así que no se toca bajo los pies de
-- nadie. La columna declara `NOT NULL`, el `UNIQUE` que la convierte en identidad de la tarea y el
-- `CHECK` de no-vacío —un nombre en blanco es una fila sin valor, y éste es el único momento en
-- que cerrarle la puerta cuesta una línea—. El tope de 120 caracteres **no** se declara aquí: lo
-- impone el borde HTTP, el único que puede contestar un `400` nombrando el campo, como decidió la
-- `0005`.

CREATE TABLE backup_jobs_nueva (
    id TEXT PRIMARY KEY NOT NULL, -- UUIDv4
    target_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    cron_expression TEXT NOT NULL,
    keep_daily INTEGER NOT NULL DEFAULT 7,
    keep_weekly INTEGER NOT NULL DEFAULT 4,
    keep_monthly INTEGER NOT NULL DEFAULT 12,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    UNIQUE(name),
    FOREIGN KEY(target_id) REFERENCES backup_targets(id) ON DELETE CASCADE,
    FOREIGN KEY(repository_id) REFERENCES repositories(id) ON DELETE CASCADE
);

-- Las filas se copian **columna a columna**, nombradas: con un `SELECT *` la copia dependería del
-- orden de la tabla vieja, y el de la nueva no tiene por qué ser el mismo.
INSERT INTO backup_jobs_nueva
    (id, target_id, repository_id, cron_expression, keep_daily, keep_weekly, keep_monthly, enabled,
     created_at, name)
    SELECT id, target_id, repository_id, cron_expression, keep_daily, keep_weekly, keep_monthly,
           enabled, created_at, name
    FROM backup_jobs;

DROP TABLE backup_jobs;

ALTER TABLE backup_jobs_nueva RENAME TO backup_jobs;

-- 3) Los dos índices de la tabla, que el `DROP TABLE` se lleva con la vieja.
CREATE INDEX IF NOT EXISTS idx_backup_jobs_target_id
    ON backup_jobs(target_id);

CREATE INDEX IF NOT EXISTS idx_backup_jobs_repository_id
    ON backup_jobs(repository_id);
