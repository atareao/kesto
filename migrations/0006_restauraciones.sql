-- Histórico de restauraciones: un **añadido** al §3, no una tabla que el SRS declare.
--
-- El §3 modela la copia —`job_executions` es el histórico de los intentos de copia— y no
-- dice nada de la restauración. Pero una restauración es una ejecución igual que una copia:
-- tiene un principio, un desenlace y un motivo cuando falla, y sin fila propia el
-- `execution_id` que la API devuelve en el `200` no diría nada después —ni si acabó, ni
-- cuándo, ni sobre qué—. Por eso la tabla se declara aquí con la misma forma que su
-- hermana, y se documenta como añadido en `docs/`.
--
-- Lo que **no** se copia de `job_executions` es el vínculo: una restauración no cuelga de
-- una tarea —la tarea es «cuándo y qué se copia», y restaurar es un acto puntual del
-- operador—, sino del `target` sobre el que se escribe. De ahí la clave foránea a
-- `backup_targets` y no a `backup_jobs`.
--
-- `id`, `snapshot_id` y `target_id` son `TEXT` con la forma canónica con guiones de un
-- UUID, como el resto del esquema: `sqlx` codifica un `Uuid` como un BLOB de 16 bytes, y
-- un BLOB no se lee con `sqlite3` ni se compara con lo que devuelve la API. La capa de
-- acceso es la que escribe `uuid::fmt::Hyphenated` y la que vuelve a `Uuid` al leer.
--
-- `started_at` es `TEXT` y **no** `DATETIME` —la forma que usan las otras tablas— porque es
-- la que declara el contrato de este change: `sqlx` escribe y lee las dos con el mismo
-- codificador (`time::OffsetDateTime` en RFC 3339), así que el tipo declarado no cambia el
-- valor, solo lo que `PRAGMA table_info` dice. Y `finished_at` es anulable porque una
-- restauración en curso no tiene final: `NULL` **no** significa «ahora».
--
-- `error_message` es anulable por lo mismo que en `job_executions`: una restauración que
-- termina bien no tiene motivo de fallo, y `''` y `NULL` no son lo mismo.
--
-- El `CHECK` de `status` es idéntico al de `job_executions` a propósito: son las mismas
-- cuatro fases del mismo ciclo de vida —encolada, en curso, terminada bien, terminada
-- mal—, y quien las escribe es el mismo enumerado de la capa de acceso. El valor por
-- defecto es `pending` porque una restauración nace encolada: el alta no tiene por qué
-- decir un estado que siempre es el mismo, y la base lo deja escrito aunque la fila entre
-- por una vía que no sea la capa de acceso.
--
-- `target_id` es **anulable** y su clave foránea es `ON DELETE CASCADE`: una restauración
-- sobre un target que no existe —o que se borró después— no puede inventarse el vínculo, y
-- borrar el target se lleva su histórico, que es lo mismo que hace `job_executions` con su
-- tarea. Anulable no es un descuido: el contrato de la tabla lo permite —el `target_id` que
-- resuelve la instantánea puede no existir— y la capa de acceso lo publica como `Option`.

CREATE TABLE IF NOT EXISTS restore_executions (
    id TEXT PRIMARY KEY NOT NULL, -- UUIDv4 en forma canónica con guiones
    snapshot_id TEXT NOT NULL,    -- ID devuelto por rustic_core
    target_id TEXT,               -- NULL: restauración sin target resuelto
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'running', 'success', 'failed')),
    error_message TEXT,           -- NULL: no falló
    started_at TEXT NOT NULL,
    finished_at TEXT,             -- NULL: sigue en curso
    FOREIGN KEY(target_id) REFERENCES backup_targets(id) ON DELETE CASCADE
);

-- El listado del panel: las restauraciones de un estado, lo último primero. Es la misma
-- pareja —filtro, orden— que cubre `idx_job_executions_status_started_at` para el histórico
-- de copias, y sin él el listado ordena recorriendo la tabla entera.
CREATE INDEX IF NOT EXISTS idx_restore_executions_status_started_at
    ON restore_executions(status, started_at);

-- La columna **hija** de la clave foránea, por la misma razón que los tres índices de la
-- `0002`: SQLite no crea índices para las claves foráneas, y sin éste cada borrado de un
-- target recorre `restore_executions` entera para decidir qué filas arrastra la cascada.
-- Además cubre el filtro `target_id` del listado, que filtra por la misma columna.
CREATE INDEX IF NOT EXISTS idx_restore_executions_target_id
    ON restore_executions(target_id);