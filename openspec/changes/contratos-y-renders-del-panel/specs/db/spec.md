# db — delta

## MODIFIED Requirements

### Requirement: Las cuatro tablas del §3 se reproducen exactas, con sus `CHECK`, sus cascadas y los índices añadidos

Las migraciones SHALL declarar las cuatro tablas del §3 —`repositories`, `backup_targets`, `backup_jobs` y `job_executions`— con las columnas, los tipos y los valores por defecto de ese apartado, incluidos los `CHECK` de enumerado `target_type IN ('mariadb', 'postgresql', 'directory', 'sqlite')` y `status IN ('pending', 'running', 'success', 'failed')`, y las tres claves foráneas `ON DELETE CASCADE`. `backup_targets` SHALL incluir además la columna `db_path` (`TEXT`, anulable), que junto a `container_name` describe una fuente `sqlite`. `backup_jobs` SHALL incluir además la columna `name` (`TEXT`, **no nula**, con `UNIQUE(name)`), declarada aquí **como un añadido al SRS** —el §3 no la tiene en su versión original—: un UUID no identifica la tarea al operador, y el nombre es lo que hace legible el histórico y los snapshots. La creó anulable la `0005` y la hizo obligatoria y única la `0007`; este apartado fija el **estado final**, y el escenario que describe el estado intermedio lo dice en su propio `GIVEN`. La forma exacta de esa columna la fija el requisito «La obligatoriedad y la unicidad del nombre reconstruyen `backup_jobs`, y la reconstrucción no se lleva ninguna fila».

Las restricciones SHALL estar **en la base** y no solo en Rust: un `INSERT` hecho con `sqlite3` a mano tiene que chocar con el mismo `CHECK` que choca la aplicación. Es la única forma de que el enumerado sea una garantía y no una convención.

Además de lo que el SRS declara, SHALL crearse **cuatro índices**, que el §3 no tiene y que **se declaran aquí como un añadido al SRS**:

- `idx_backup_jobs_target_id` — columna hija de la FK a `backup_targets`
- `idx_backup_jobs_repository_id` — columna hija de la FK a `repositories`
- `idx_job_executions_job_id` — columna hija de la FK a `backup_jobs`
- `idx_job_executions_status_started_at` — para el listado del dashboard, por estado y fecha

Los tres primeros SHALL existir porque **SQLite no crea índices para las claves foráneas**: sin índice en la columna hija, cada borrado en cascada recorre la tabla entera, y el SRS declara tres cascadas sin pagar ninguno de los tres recorridos. El cuarto cubre el listado del §6: sin él, la consulta del panel ordena recorriendo.

Filtrar las ejecuciones **por repositorio** SHALL resolverse con el `JOIN` por `backup_jobs` apoyado en los índices anteriores, y NO SHALL añadirse una columna `repository_id` a `job_executions`: duplicaría un dato que ya vive en la tarea y crearía una segunda verdad que se puede desincronizar.

Los índices SHALL nombrarse `idx_{tabla}_{columna}`. Una reconstrucción de la tabla SHALL recrearlos: el `DROP TABLE` de la vieja se los lleva con ella.

#### Scenario: Las cuatro tablas existen con sus columnas
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se enumeran las tablas y sus columnas
- **THEN** SHALL existir `repositories`, `backup_targets`, `backup_jobs` y `job_executions`
- **AND** sus columnas SHALL ser las del §3, más `backup_targets.db_path` y `backup_jobs.name`

#### Scenario: Los enumerados están garantizados por un `CHECK`
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se intenta insertar un `target_type` o un `status` fuera de su enumerado, con SQL directo
- **THEN** la inserción SHALL fallar
- **AND** NO SHALL depender de que la validación la haya hecho la aplicación

#### Scenario: El `CHECK` de `target_type` admite `sqlite`
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se inserta un `target_type = 'sqlite'` con SQL directo
- **THEN** la inserción SHALL aceptarse
- **AND** un `target_type = 'sqlite3'` SHALL rechazarse

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

#### Scenario: La columna `name` de `backup_jobs` existe y es anulable
- **GIVEN** una base llevada a mano hasta la `0005` —el estado intermedio que la `0007` tiene que migrar, no el final—
- **WHEN** se consulta la tabla `backup_jobs`
- **THEN** SHALL existir la columna `name` de tipo `TEXT`
- **AND** SHALL admitir `NULL` en una fila insertada con SQL directo `INSERT ... (id, target_id, repository_id, cron_expression) VALUES (...)` sin especificarla

#### Scenario: La columna `name` de `backup_jobs` es obligatoria y única
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se consulta la tabla `backup_jobs` y se intenta insertar una fila con SQL directo sin `name`, o con un `name` que ya tiene otra fila
- **THEN** SHALL existir la columna `name` de tipo `TEXT`
- **AND** su nulabilidad SHALL ser `NOT NULL`
- **AND** las dos inserciones SHALL fallar por la restricción de la base

## ADDED Requirements

### Requirement: La obligatoriedad y la unicidad del nombre reconstruyen `backup_jobs`, y la reconstrucción no se lleva ninguna fila

La migración `0007_nombre_de_tarea_obligatorio.sql` SHALL declarar `backup_jobs.name` como `TEXT NOT NULL`, SHALL declarar la restricción `UNIQUE(name)` y SHALL hacerlo **reconstruyendo** la tabla —crear la nueva con la columna y la restricción, copiar las filas, borrar la vieja y renombrar—, porque SQLite no puede reescribir con `ALTER TABLE` la nulabilidad de una columna que ya existe —el `ADD COLUMN` de la `0005` la declaró anulable— ni añadir una restricción de tabla a una tabla existente. El nombre del fichero SHALL llevar el prefijo numérico entero que exige `sqlx::migrate!()`.

La reconstrucción SHALL copiar **columna a columna, nombradas** —no con un `SELECT *`, que dependería del orden de la tabla vieja—, SHALL recrear los dos índices de la tabla, `idx_backup_jobs_target_id` e `idx_backup_jobs_repository_id`, que el `DROP TABLE` se lleva con la tabla vieja, y SHALL declarar `name` en la **última** posición, como la dejó la `0005`, para que el orden que publica `PRAGMA table_info` no cambie bajo los pies de nadie.

`backup_jobs` es **padre** de `job_executions`: su `DROP TABLE` con las claves foráneas encendidas dispara el `ON DELETE CASCADE` y se lleva por delante las ejecuciones de **todas** las tareas —y con ellas el histórico de copias— antes de que esta migración haya copiado nada, sin que nada lo delate. La migración SHALL correr con `foreign_keys` **apagado**, como corre el resto del migrador (`db::migrate::aplicar`, que lo apaga en la conexión y fuera de transacción), y la comprobación posterior del migrador SHALL confirmar que no quedaron filas huérfanas. Ninguna fila de ninguna tabla SHALL perderse por la reconstrucción.

Las filas que ya existían con `name` a `NULL` SHALL rellenarse **dentro de la propia migración** con un nombre derivado, porque `NOT NULL` no admite un `NULL` preexistente y una migración que dejara filas ilegibles no serviría de nada. El derivado SHALL ser único entre **todas** las filas de la tabla —no sólo entre las rellenadas—, NO SHALL coincidir con ningún nombre que ya estuviera presente, NO SHALL ser el identificador de la tarea ni una copia de él, SHALL quedar no vacío tras recortar y SHALL medir **120** caracteres o menos. La forma concreta del derivado —algo como `tarea <octeto del id>`— es una elección de la implementación, no del contrato.

Las filas que ya tenían nombre NO SHALL tocarse, y el nombre pasa a ser una identidad de la tarea: dos tareas distintas NO SHALL poder compartirlo. La unicidad SHALL ser sensible a mayúsculas —la colación `BINARY` de SQLite, el defecto sin colación declarada—, de modo que lo que la base rechaza es exactamente lo que el borde HTTP contesta `409`.

La columna SHALL declarar además `CHECK (length(trim(name)) > 0)`, y esta parte es una **propuesta de este change** —decisión D7 del proposal, pendiente del visto bueno del operador—: un nombre en blanco es una fila sin valor, y el momento de cerrarle la puerta en la base es **éste**, porque sólo se reconstruye una vez; si se prefiere la coherencia literal con la `0005` —ninguna restricción sobre la columna—, este `SHALL` y su escenario se retiran.

El §3 del SRS SHALL declarar la columna en su forma nueva —`name TEXT NOT NULL` con `UNIQUE(name)`—: sigue siendo un añadido al SRS, y esta migración actualiza su forma. El tope de **120** caracteres NO SHALL declararse como `CHECK`: lo impone el borde HTTP, único que puede contestar un `400` nombrando el campo, como decidió la `0005`.

El porqué de reconstruir en vez de parchear: es el procedimiento canónico de SQLite para lo que `ALTER TABLE` no sabe hacer, ya usado por la `0003` para ampliar un `CHECK`, y el único camino que convierte «el nombre es obligatorio y único» en una garantía de la base en vez de una convención de la aplicación. Y el porqué del relleno: una tarea sin nombre es un estado **anterior** a esta migración, no un estado válido después de ella; conservar las filas rellenándolas es lo que permite exigir el nombre sin borrar el histórico de nadie.

#### Scenario: La migración deja la columna obligatoria y única

- **GIVEN** una base con las migraciones aplicadas hasta la `0007`
- **WHEN** se consulta `PRAGMA table_info(backup_jobs)` y se intentan dos inserciones directas —una sin `name` y otra con un `name` repetido—
- **THEN** la columna `name` SHALL ser `NOT NULL`
- **AND** SHALL existir un índice único sobre `name`
- **AND** las dos inserciones SHALL fallar

#### Scenario: Las tareas que no tenían nombre se rellenan con nombres únicos

- **GIVEN** una base llevada a mano hasta la `0006` con dos tareas cuyo `name` es `NULL` y una tercera con un nombre puesto
- **WHEN** se aplica la `0007`
- **THEN** las tres filas SHALL tener un nombre no vacío
- **AND** los tres nombres SHALL ser distintos entre sí
- **AND** el nombre de la tercera NO SHALL haber cambiado
- **AND** ningún nombre derivado SHALL ser el identificador de su tarea

#### Scenario: El nombre derivado no choca con uno que ya estaba

- **GIVEN** una tarea ya nombrada cuyo nombre coincide con el derivado que le tocaría a otra tarea sin nombre
- **WHEN** se aplica la `0007`
- **THEN** los nombres de las dos filas SHALL seguir siendo distintos
- **AND** el nombre que ya estaba NO SHALL haber cambiado

#### Scenario: Ninguna ejecución se pierde en la reconstrucción

- **GIVEN** una base con tareas —algunas sin nombre— y filas en `job_executions` colgando de ellas
- **WHEN** se aplica la `0007`
- **THEN** todas las filas de `job_executions` SHALL seguir ahí, con su `job_id` resolviendo a su tarea
- **AND** NO SHALL haber quedado ninguna fila huérfana
- **AND** las claves foráneas de `job_executions` NO SHALL quedar rotas

#### Scenario: Los índices de la tabla sobreviven a la reconstrucción

- **GIVEN** una base migrada a la `0007`
- **WHEN** se consultan los índices de `backup_jobs`
- **THEN** SHALL seguir existiendo `idx_backup_jobs_target_id` e `idx_backup_jobs_repository_id`
- **AND** SHALL existir el índice único de `name`

#### Scenario: Un nombre en blanco no cabe en la base

- **GIVEN** una base migrada a la `0007`
- **WHEN** se inserta con SQL directo una fila cuyo `name` es una cadena de solo espacios
- **THEN** la inserción SHALL fallar por la restricción de la columna
- **AND** NO SHALL depender de que la validación la haya hecho la aplicación
