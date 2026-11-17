-- `repositories` gana la salud de su repositorio Rustic (issue #111): el desenlace de la última
-- comprobación, la fecha en que se hizo y un motivo acotado.
--
-- Va con tres `ALTER TABLE ... ADD COLUMN` y **no** reconstruyendo la tabla: aquí no se cambia
-- ningún `CHECK` —el único que se añade es el de la columna nueva—, y el `DROP TABLE` de una
-- reconstrucción se llevaría por delante, con las claves foráneas encendidas, las `backup_jobs`
-- que cuelgan de `repositories` y sus `job_executions`. El `ADD COLUMN` ni las mira.
--
-- Las tres son **anulables y sin valor por defecto**, y eso es contrato, no comodidad:
--
-- * Anulables porque **desconocido** es un estado válido y es el de todas las filas que ya
--   existían: la salud la escribe la comprobación, y hasta que no se comprueba una fila no hay
--   nada que decir de ella. Un `NOT NULL` con defecto obligaría a inventar un estado —`ok` o
--   `inaccesible`— en el momento de migrar, y las dos mentiras son peores que el silencio.
-- * Sin valor por defecto porque `''` y `NULL` no son lo mismo: solo el `NULL` significa «no se
--   sabe», y una cadena vacía en `health` chocaría además con el `CHECK` de abajo.
--
-- El `CHECK(health IN (...) OR health IS NULL)` es el enumerado cerrado del contrato, con la
-- pareja en un solo sitio: los cinco literales del texto almacenado —`ok`, `sin_repositorio`,
-- `passphrase`, `inaccesible`, `sin_permiso`— y el `NULL` explícito del «desconocido». Es lo que
-- hace que la columna sea legible con `sqlite3` y que un estado inventado desde fuera no entre:
-- la capa de acceso escribe el nombre de la variante en snake_case, y este `CHECK` es el que
-- decide si ese nombre es uno de los cinco.
--
-- El orden de las columnas es el de las tres sentencias: van las **últimas** del orden declarado
-- de la tabla —detrás de `created_at`—, porque es el orden en que `ALTER TABLE` las añade y el
-- que publica `PRAGMA table_info`. El §3 no las declara, así que no hay una posición que
-- respetar; lo que sí importa es que el mapeo fila→struct las lea **por nombre**.

ALTER TABLE repositories
    ADD COLUMN health TEXT CHECK (
        health IN ('ok', 'sin_repositorio', 'passphrase', 'inaccesible', 'sin_permiso')
        OR health IS NULL
    );

ALTER TABLE repositories ADD COLUMN health_checked_at DATETIME;

ALTER TABLE repositories ADD COLUMN health_detail TEXT;
