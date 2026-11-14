## MODIFIED Requirements

### Requirement: Las cuatro tablas del §3 se reproducen exactas, con sus `CHECK`, sus cascadas y los índices añadidos

Las migraciones SHALL declarar las cuatro tablas del §3 —`repositories`, `backup_targets`, `backup_jobs` y `job_executions`— con las columnas, los tipos y los valores por defecto de ese apartado, incluidos los `CHECK` de enumerado `target_type IN ('mariadb', 'postgresql', 'directory', 'sqlite')` y `status IN ('pending', 'running', 'success', 'failed')`, y las tres claves foráneas `ON DELETE CASCADE`. `backup_targets` SHALL incluir además la columna `db_path` (`TEXT`, anulable), que junto a `container_name` describe una fuente `sqlite`.

Las restricciones SHALL estar **en la base** y no solo en Rust: un `INSERT` hecho con `sqlite3` a mano tiene que chocar con el mismo `CHECK` que choca la aplicación. Es la única forma de que el enumerado sea una garantía y no una convención.

Además de lo que el SRS declara, SHALL crearse **cuatro índices**, que el §3 no tiene y que **se declaran aquí como un añadido al SRS**:

- `idx_backup_jobs_target_id` — columna hija de la FK a `backup_targets`
- `idx_backup_jobs_repository_id` — columna hija de la FK a `repositories`
- `idx_job_executions_job_id` — columna hija de la FK a `backup_jobs`
- `idx_job_executions_status_started_at` — para el listado del dashboard, por estado y fecha

Los tres primeros SHALL existir porque **SQLite no crea índices para las claves foráneas**: sin índice en la columna hija, cada borrado en cascada recorre la tabla entera, y el SRS declara tres cascadas sin pagar ninguno de los tres recorridos. El cuarto cubre el listado del §6: sin él, la consulta del panel ordena recorriendo.

Filtrar las ejecuciones **por repositorio** SHALL resolverse con el `JOIN` por `backup_jobs` apoyado en los índices anteriores, y NO SHALL añadirse una columna `repository_id` a `job_executions`: duplicaría un dato que ya vive en la tarea y crearía una segunda verdad que se puede desincronizar.

Los índices SHALL nombrarse `idx_{tabla}_{columna}`.

#### Scenario: Las cuatro tablas existen con sus columnas
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se enumeran las tablas y sus columnas
- **THEN** SHALL existir `repositories`, `backup_targets`, `backup_jobs` y `job_executions`
- **AND** sus columnas SHALL ser las del §3, más `backup_targets.db_path`

#### Scenario: Los enumerados están garantizados por un `CHECK`
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se intenta insertar un `target_type` o un `status` fuera de su enumerado, con SQL directo
- **THEN** la inserción SHALL fallar
- **AND** NO SHALL depender de que la validación la haya hecho la aplicación

#### Scenario: El `CHECK` de `target_type` admite `sqlite`
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se inserta un `target_type = 'sqlite'` con SQL directo
- **THEN** la inserción SHALL aceptarse
- **AND** un `target,`target_type = 'sqlite3'` SHALL rechazarse

#### Scenario: Cada clave foránea tiene su índice
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se consultan los índices declarados
- **THEN** SHALL existir `idx_backup_jobs_target_id`, `idx_backup_jobs_repository_id` y `idx_job_executions_job_id`
- **AND** el plan de una consulta que borra una fila padre NO SHALL recorrer la tabla hija entera

#### Scenario: El listado de ejecuciones tiene su índice
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se planifica el listado de ejecuciones ordenado por fecha y filtrado por estado
- **THEN** el plan SHALL usar `idx_job_executions_status_started_at`
- **AND** NO SHALL recorrer la tabla entera

#### Scenario: Filtrar por repositorio no necesita una columna duplicada
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se planifica el `JOIN` de `job_executions` con `backup_jobs` filtrando por `repository_id`
- **THEN** el plan SHALL apoyarse en los índices de clave foránea
- **AND** `job_executions` NO SHALL tener columna `repository_id`

## ADDED Requirements

### Requirement: La ampliación del enumerado `target_type` reconstruye la tabla, porque SQLite no altera un `CHECK` en sitio

La migración `0003` SHALL añadir `sqlite` al `CHECK` de `target_type` y la columna `db_path`, y SHALL hacerlo **reconstruyendo** `backup_targets` —crear la tabla nueva con el `CHECK` y la columna, copiar las filas, borrar la vieja y renombrar—, porque SQLite no permite modificar un `CHECK` existente con `ALTER TABLE`. La reconstrucción SHALL preservar todas las filas existentes, con `db_path` a `NULL`, y SHALL recrear los índices de la tabla. El manejo de `foreign_keys` durante la reconstrucción SHALL ser correcto, de modo que la operación no dispare cascadas ni deje filas huérfanas.

El porqué: la tabla del §3 fue declarada con el enumerado cerrado y la feature lo amplía con un valor más. Rehacer la tabla es el procedimiento canónico de SQLite para cambiar un `CHECK`; hacerlo con cuidado —dentro de la propia migración, sin pérdida de filas y con las claves foráneas bajo control— es lo único que mantiene la garantía de la base intacta.

#### Scenario: La migración conserva las filas existentes
- **GIVEN** una base con targets `mariadb`, `postgresql` y `directory` ya escritos
- **WHEN** se aplica la migración `0003`
- **THEN** todas las filas SHALL seguir ahí, con las mismas columnas de antes
- **AND** su `db_path` SHALL ser `NULL`

#### Scenario: Los índices sobreviven a la reconstrucción
- **GIVEN** una base migrada a `0003`
- **WHEN** se consultan los índices de `backup_targets` y de las tablas con FK a ella
- **THEN** los índices declarados SHALL seguir existiendo
- **AND** las claves foráneas que apuntan a `backup_targets` NO SHALL quedar rotas