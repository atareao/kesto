## ADDED Requirements

### Requirement: La ejecución de restauración se persiste y su ciclo de vida lo escribe el motor

Toda restauración lanzada SHALL tener una fila en `restore_executions` **antes** de contestar el `200`, en estado `pending`, con su `snapshot_id`, su `target_id` y su `started_at`, y ese `id` SHALL ser el `execution_id` que devuelve la respuesta. Al empezar a trabajar SHALL pasar a `running`; al terminar con éxito, a `success` con `finished_at`; al fallar, a `failed` con `error_message` y `finished_at`. La escritura final SHALL ocurrir **también** cuando el trabajo falla, de modo que ninguna restauración se quede en `running` para siempre.

El trabajo de restauración SHALL correr fuera del runtime de Tokio, como el de copia, porque las llamadas de `rustic_core` que leen el repositorio son bloqueantes y la biblioteca construye su propio runtime.

El porqué: una ejecución en `pending` sin paso a `running`, o en `running` sin cierre, miente al operador y ensucia el histórico; y el motivo del fallo es justo lo que el operador necesita para saber si el problema fue la contraseña, el binario o el contenedor.

#### Scenario: La fila existe antes de responder

- **GIVEN** una petición de restauración aceptada
- **WHEN** llega el `200`
- **THEN** SHALL existir una fila en `restore_executions` con ese `execution_id` y estado `pending`

#### Scenario: El ciclo de vida se escribe

- **GIVEN** una restauración que empieza
- **WHEN** el motor la toma
- **THEN** su fila SHALL pasar a `running`
- **AND** al terminar bien SHALL quedar en `success` con `finished_at`

#### Scenario: Un fallo se registra con su motivo

- **GIVEN** una restauración que falla
- **WHEN** el trabajo termina
- **THEN** su fila SHALL quedar en `failed` con `error_message` y `finished_at`
- **AND** NO SHALL quedarse en `running`

### Requirement: El motor restaura según el tipo de target y reserva el destino antes de trabajar

El motor SHALL aceptar la restauración de un target `directory` y de un target de base de datos, y SHALL elegir el camino según el tipo: para `directory`, la escritura a `host_path` con `LocalDestination`; para `mariadb`, `postgresql` y `sqlite`, el flujo inverso de `restauracion` dentro del contenedor del target. NO SHALL enviarse al camino de directorio un target de base, ni al revés.

Antes de arrancar el trabajo, el motor SHALL **reservar el destino** —el `host_path` de un directorio, o el `container_name` junto con la base o el `db_path` de un target de base— y SHALL rechazar una segunda restauración sobre un destino ya reservado. La reserva SHALL liberarse al terminar el trabajo, también cuando falla.

#### Scenario: Un target de base va por el camino inverso

- **GIVEN** una restauración de un target `mariadb`
- **WHEN** el motor la ejecuta
- **THEN** SHALL leer el volcado del repositorio e inyectarlo por el `stdin` del contenedor
- **AND** NO SHALL escribir en el `host_path` del sistema de ficheros de Kesto

#### Scenario: La reserva impide dos restauraciones sobre el mismo destino

- **GIVEN** una restauración en curso sobre un destino
- **WHEN** se intenta una segunda sobre ese destino
- **THEN** el motor SHALL rechazarla
- **AND** la reserva SHALL liberarse al terminar la primera
