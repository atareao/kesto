-- `sqlite` entra en el enumerado de `target_type` y `backup_targets` gana `db_path`
-- (issue #11). Una fuente `sqlite` se describe con `container_name` + `db_path`,
-- igual que una de cliente/servidor se describe con `container_name` + `db_*`.
--
-- SQLite **no** permite modificar un `CHECK` con `ALTER TABLE`: la tabla se
-- **reconstruye** —crear la nueva con el `CHECK` ampliado y la columna, copiar las
-- filas, borrar la vieja y renombrar—, que es el procedimiento canónico. Se
-- reconstruye `backup_targets` y nada más: `repositories`, `backup_jobs` y
-- `job_executions` no cambian, y sus índices no se tocan.
--
-- El `DROP TABLE` de la vieja es lo delicado. Con las claves foráneas encendidas
-- ejecuta el `ON DELETE CASCADE` de `backup_jobs` y se lleva por delante sus filas
-- —y las de `job_executions`, que cuelgan de ellas— antes de que esta migración
-- hubiera copiado nada. Por eso las migraciones corren con `foreign_keys`
-- **apagado**, y se apaga en la **conexión** y fuera de transacción
-- (`db::migrate::aplicar`): `PRAGMA foreign_keys` es un no-op dentro de una, y la
-- marca `-- no-transaction` no sirve aquí —`sqlx-sqlite` 0.8.6 lee el flag en
-- `sqlx-core` y no lo mira—. `db::migrate::aplicar` vuelve a encenderlo y comprueba
-- después que no quedaran filas huérfanas.
--
-- La clave foránea de `backup_jobs` se queda como está —apunta a `backup_targets`
-- por **nombre**, y ese nombre vuelve a existir—, así que no hay que tocar la tabla
-- hija ni se dispara ninguna cascada. El índice implícito de la clave primaria lo
-- recrea la propia base al crearla; ninguna otra tabla lleva índice sobre
-- `backup_targets`.

CREATE TABLE backup_targets_nueva (
    id TEXT PRIMARY KEY NOT NULL, -- UUIDv4
    name TEXT NOT NULL,
    target_type TEXT NOT NULL CHECK(target_type IN ('mariadb', 'postgresql', 'directory', 'sqlite')),
    container_name TEXT,          -- Nullable si es un directorio local
    db_user TEXT,                 -- Requerido para bases de cliente/servidor
    db_password TEXT,             -- Cifrado en reposo
    db_name TEXT,                 -- Opcional (NULL implica dump global)
    host_path TEXT,               -- Requerido para tipo 'directory'
    db_path TEXT,                 -- Requerido para tipo 'sqlite': ruta del .db dentro del contenedor
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Las filas se copian **columna a columna**, nombradas: con un `SELECT *` la copia
-- dependería del orden de la tabla vieja, y `db_path` no existía en ella.
INSERT INTO backup_targets_nueva
    (id, name, target_type, container_name, db_user, db_password, db_name, host_path, db_path, created_at)
    SELECT id, name, target_type, container_name, db_user, db_password, db_name, host_path, NULL, created_at
    FROM backup_targets;

DROP TABLE backup_targets;

ALTER TABLE backup_targets_nueva RENAME TO backup_targets;
