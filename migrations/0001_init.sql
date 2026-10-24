-- Esquema del §3 de `docs/DEFINITION.md`, tal cual: columnas, tipos, valores por
-- defecto, los dos `CHECK` de enumerado y los tres `ON DELETE CASCADE`.
--
-- Los índices NO van aquí: son un añadido al SRS y viven en `0002_indexes.sql`,
-- para que se lean como lo que son y no se confundan con lo que el §3 declara.

-- Repositorios S3 de destino
CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY NOT NULL, -- UUIDv4
    name TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    bucket TEXT NOT NULL,
    region TEXT NOT NULL,
    access_key_id TEXT NOT NULL,
    secret_access_key TEXT NOT NULL,
    passphrase TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Fuentes de Backup (Targets)
CREATE TABLE IF NOT EXISTS backup_targets (
    id TEXT PRIMARY KEY NOT NULL, -- UUIDv4
    name TEXT NOT NULL,
    target_type TEXT NOT NULL CHECK(target_type IN ('mariadb', 'postgresql', 'directory')),
    container_name TEXT,          -- Nullable si es un directorio local
    db_user TEXT,                 -- Requerido para DBs
    db_password TEXT,             -- Encrypted/Plain para DBs
    db_name TEXT,                 -- Opcional (NULL implica dump global)
    host_path TEXT,               -- Requerido para tipo 'directory'
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Tareas Programadas (Jobs)
CREATE TABLE IF NOT EXISTS backup_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    target_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    cron_expression TEXT NOT NULL,
    keep_daily INTEGER NOT NULL DEFAULT 7,
    keep_weekly INTEGER NOT NULL DEFAULT 4,
    keep_monthly INTEGER NOT NULL DEFAULT 12,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(target_id) REFERENCES backup_targets(id) ON DELETE CASCADE,
    FOREIGN KEY(repository_id) REFERENCES repositories(id) ON DELETE CASCADE
);

-- Histórico de Ejecuciones
CREATE TABLE IF NOT EXISTS job_executions (
    id TEXT PRIMARY KEY NOT NULL,
    job_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'success', 'failed')),
    snapshot_id TEXT,             -- ID devuelto por rustic_core
    bytes_processed INTEGER NOT NULL DEFAULT 0,
    bytes_uploaded INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    started_at DATETIME NOT NULL,
    finished_at DATETIME,
    FOREIGN KEY(job_id) REFERENCES backup_jobs(id) ON DELETE CASCADE
);
