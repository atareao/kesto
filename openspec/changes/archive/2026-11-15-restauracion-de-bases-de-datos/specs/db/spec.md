## ADDED Requirements

### Requirement: El esquema registra las ejecuciones de restauración

La migración `0006_restauraciones.sql` SHALL crear la tabla `restore_executions` con: `id` (`TEXT`, clave primaria, UUID en su forma canónica con guiones), `snapshot_id` (`TEXT`, no nulo), `target_id` (`TEXT`, anulable, clave foránea a `backup_targets(id)` con `ON DELETE CASCADE`), `status` (`TEXT`, no nulo, `CHECK (status IN ('pending','running','success','failed'))`, por defecto `'pending'`), `error_message` (`TEXT`, anulable), `started_at` (`TEXT`, no nulo) y `finished_at` (`TEXT`, anulable). SHALL declarar además los índices `idx_restore_executions_status_started_at`, para el listado por estado y fecha, y `idx_restore_executions_target_id`, columna hija de la clave foránea y apoyo del filtro `target_id` del listado, por coherencia con el resto del esquema.

La tabla SHALL declararse **como un añadido al SRS** —el §3 modela la copia, no la restauración—, y la migración SHALL seguir la forma de nombre con prefijo numérico entero que exige `sqlx::migrate!()`. La capa de acceso SHALL escribir y leer los identificadores en su forma canónica con guiones, como en el resto del esquema.

`target_id` SHALL ser anulable y la clave foránea SHALL ser `ON DELETE CASCADE`, de modo que borrar un target borre su histórico de restauraciones, coherente con el resto del esquema.

El porqué: sin registro, el `execution_id` que hoy devuelve el `200` no dice nada después —no se sabe si la restauración acabó, ni cuándo, ni sobre qué—, y el operador no tiene histórico. La ejecución de restauración merece la misma trazabilidad que la de copia.

#### Scenario: La tabla existe tras migrar

- **GIVEN** una base recién migrada
- **WHEN** se consultan las tablas y sus índices
- **THEN** SHALL existir `restore_executions` con sus columnas, su `CHECK` de `status` y su clave foránea a `backup_targets`
- **AND** SHALL existir `idx_restore_executions_status_started_at` e `idx_restore_executions_target_id`

#### Scenario: El estado no admite un valor fuera del enumerado

- **GIVEN** la tabla `restore_executions`
- **WHEN** se intenta escribir un `status` distinto de `pending`, `running`, `success` o `failed`
- **THEN** la escritura SHALL rechazarse por el `CHECK`

#### Scenario: Borrar el target borra su histórico de restauraciones

- **GIVEN** una restauración ligada a un target
- **WHEN** se borra ese target
- **THEN** su fila en `restore_executions` NO SHALL existir
