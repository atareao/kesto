# Optional container stop before backup

## ADDED Requirements

### Requirement: Parada y reinicio opcional de contenedores alrededor de una copia

El motor SHALL detener, antes de iniciar una copia, los contenedores enumerados en `KESTO_STOP_CONTAINERS` cuando la variable esté configurada, y SHALL reiniciarlos una vez terminada la copia. Cuando la variable no esté configurada, el motor NO SHALL detener ni iniciar ningún contenedor y la copia SHALL transcurrir con normalidad. Las operaciones de parada y arranque SHALL realizarse mediante Bollard a través del motor privilegiado.

#### Scenario: No containers specified → backup proceeds normally
- **GIVEN** a backup target configured for a database
- **AND** `KESTO_STOP_CONTAINERS` is not set (no containers specified)
- **WHEN** the backup runs
- **THEN** no Docker containers are stopped or started
- **AND** the backup proceeds normally

#### Scenario: One or more containers specified → containers are stopped, backup taken, restarted after
- **GIVEN** `KESTO_STOP_CONTAINERS` lists container names/IDs
- **AND** those containers are running
- **WHEN** the backup runs
- **THEN** each listed container is stopped before the backup starts
- **AND** the backup is taken against the database
- **AND** each stopped container is restarted after the backup completes

### Requirement: Los contenedores ya detenidos se restauran siempre y el cierre de la ejecución informa de los fallos

El motor SHALL reiniciar todos los contenedores que haya detenido incluso cuando la copia falle, SHALL revertir los contenedores ya detenidos si detener otro falla y NO SHALL intentar la copia en ese caso. Un fallo al detener un contenedor SHALL dejar la ejecución en `failed` con `finished_at` y un `error_message` que nombre el contenedor y SHALL publicar el evento final `execution_progress`. Un fallo al reiniciar tras una copia correcta SHALL considerarse no fatal: la ejecución SHALL permanecer en `success` con su `snapshot_id`, SHALL llevar una advertencia en `error_message` y en `current_action`, y el evento final SHALL publicarse después de la advertencia.

#### Scenario: Backup fails → containers are still restarted and error logged
- **GIVEN** `KESTO_STOP_CONTAINERS` lists container names/IDs
- **AND** those containers are running
- **WHEN** the backup fails
- **THEN** each stopped container is restarted despite the backup failure
- **AND** the failure is logged
- **AND** the backup error is surfaced

#### Scenario: A container cannot be stopped → already stopped containers are rolled back, the row is `failed` and the final event is published
- **GIVEN** `KESTO_STOP_CONTAINERS` lists `web,db`
- **AND** both containers are running
- **AND** stopping `db` fails
- **WHEN** the backup runs
- **THEN** no backup is attempted
- **AND** `web` is restarted before the error is returned
- **AND** `db` is never sent a stop request
- **AND** the execution row has `status: failed`, a populated `finished_at` and an `error_message` naming `db`
- **AND** an `execution_progress` event is published for that execution with `status: failed` and `current_action: "terminando"`

#### Scenario: A container cannot be restarted after a successful backup → the result stays `success` and the operator is warned
- **GIVEN** `KESTO_STOP_CONTAINERS` lists `web,db`
- **AND** both containers were stopped successfully
- **AND** the backup completes successfully
- **AND** restarting `db` fails
- **WHEN** the execution is closed
- **THEN** the execution row has `status: success` and its `snapshot_id`
- **AND** `error_message` holds a non-fatal warning naming `db`
- **AND** an `execution_progress` event carries that warning in `current_action` with `status: success`
- **AND** the final `execution_progress` event is published after the warning with `status: success` and `current_action: "terminando"`
- **AND** the warning tells the operator to restart `db` manually
