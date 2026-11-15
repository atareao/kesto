## ADDED Requirements

### Requirement: El listado de snapshots declara, por instantánea, el target del que procede

`GET /api/v1/snapshots?repository_id=<id>` SHALL añadir a **cada** elemento de `data` tres campos nuevos: `target_id`, `target_name` y `target_type`, resueltos por el mismo vínculo que usa la restauración —`snapshot → job_executions → backup_jobs → backup_targets`— y acotados al `repository_id` que se pide. Los tres SHALL ser anulables y SHALL publicarse como `null` cuando la instantánea no case con ninguna ejecución, tarea ni target de ese repositorio: una copia hecha por otra herramienta, o cuya fila en `job_executions` se borró en cascada al borrar su tarea. `null` SHALL ser un valor del contrato, no un hueco: un elemento con `target_id` a `null` es un elemento sin target conocido, y NO SHALL confundirse con un error del listado ni impedir que se listen las demás instantáneas.

El listado SHALL ser tolerante a la ausencia del vínculo —una consulta por elemento o una única unión externa, es indiferente—, y NO SHALL fallar, ni acortar el listado, ni ocultar la instantánea huérfana por el hecho de que no tenga target.

El porqué: el panel necesita saber **sobre qué target** se va a restaurar antes de confirmarlo, y si ese target es restaurable, y hoy el listado no lo dice. Sin estos tres campos, la confirmación no puede nombrar el destino —que es justo lo que evita escribir en el sitio equivocado— y la acción no puede saber si debe ofrecerse o explicar su limitación; y como el vínculo con el target puede faltar por causas legítimas, el contrato tiene que decir qué significa su ausencia en vez de dejar que cada cliente la adivine.

#### Scenario: Una instantánea con ejecución trae el target

- **GIVEN** una instantánea cuya fila en `job_executions` casa con una tarea y su target del repositorio que se pide
- **WHEN** se pide `GET /api/v1/snapshots?repository_id=<id>`
- **THEN** el elemento de esa instantánea SHALL traer `target_id`, `target_name` y `target_type` del target de origen
- **AND** los campos que ya publicaba —`id`, `time`, `paths`, `label`, `files`, `bytes`— SHALL seguir ahí sin cambios

#### Scenario: Una instantánea sin ejecución trae los tres campos a null

- **GIVEN** una instantánea de ese repositorio que no casa con ninguna ejecución, tarea ni target —copia huérfana, hecha por otra herramienta, o cuya fila se borró en cascada
- **WHEN** se pide `GET /api/v1/snapshots?repository_id=<id>`
- **THEN** los campos `target_id`, `target_name` y `target_type` de ese elemento SHALL ser `null`
- **AND** la instantánea SHALL seguir apareciendo en el listado
- **AND** NO SHALL contestarse `404` ni `409` por esa instantánea

### Requirement: La restauración de un snapshot se pide por su recurso y usa el target de origen

El panel SHALL poder pedir la restauración de un snapshot completo mediante `POST /api/v1/snapshots/{id}/restore`, con el `repository_id` en el cuerpo. La petición SHALL resolverse contra la ejecución que produjo el snapshot y su tarea, y SHALL restaurar sobre el target de origen. La respuesta SHALL seguir la envoltura de `api-shell` y SHALL incluir, en `data`, el identificador de la ejecución de restauración, el `snapshot_id`, el `target_id` y un nombre legible del target.

El endpoint SHALL comprobar, **antes de contestar**, que la instantánea existe en el almacenamiento del repositorio indicado; si no está, SHALL responder `404` y NO SHALL lanzar ninguna ejecución ni publicar ningún aviso. El `200` NO SHALL ser optimista: no SHALL devolverse por el solo hecho de que el `repository_id` resuelva, sino solo cuando la instantánea se haya encontrado de verdad.

Si el snapshot existe pero no está asociado a ninguna ejecución o target, la API SHALL responder `409` con un mensaje claro. Si el target no es restaurable en esta versión (tipos de base de datos), la API SHALL responder `422` explicando la limitación.

El progreso SHALL publicarse en el stream global de `/api/v1/events` con `event: restore_progress` y un payload con `execution_id`, `snapshot_id`, `phase`, `files_done`, `files_total`, `bytes_done` y `bytes_total`. La fase SHALL ser `restaurando` al arrancar y SHALL publicarse un aviso de cierre con fase `terminada` si la restauración termina bien o `fallida` si falla; el cierre SHALL publicarse en ambos desenlaces. En esta versión los cuatro contadores SHALL publicarse a `0` y NO SHALL interpretarse como «cero ficheros»: el aviso describe el **empezó/terminó** y la fase, no un avance incremental, porque `Repository::restore` de `rustic_core` 0.13 no ofrece un callback de progreso por su API pública. El contrato del evento NO SHALL depender de esos contadores —el campo existe para cuando la biblioteca lo permita, no para que un cliente lea hoy una cifra—, y el aviso NO SHALL prometer un avance incremental.

El porqué: restaurar es la operación que hace útil una copia; sin endpoint no hay forma de recuperar los datos desde el producto. El mapeo por ejecución es el único vínculo fiable entre un snapshot y su target, porque el snapshot no guarda el nombre de la tarea y el esquema relaciona la ejecución con la tarea y esta con el target. Y la comprobación contra el almacenamiento es lo que distingue «la copia no está» de «la copia está pero no sé sobre qué restaurarla»: sin ella, el `200` prometería un trabajo contra una instantánea que no existe y el panel daría por lanzada una restauración que nunca ocurrirá.

#### Scenario: Restaurar un snapshot de un target de directorio
- **GIVEN** un snapshot de un target `directory`
- **WHEN** se envía `POST /api/v1/snapshots/{id}/restore` con su `repository_id`
- **THEN** la respuesta SHALL traer `data.target_id` y `data.execution_id`
- **AND** SHALL publicarse `restore_progress` en `/api/v1/events`

#### Scenario: Snapshot sin target
- **GIVEN** un snapshot que existe en el repositorio pero no casa con ninguna ejecución
- **WHEN** se pide restaurarlo
- **THEN** la API SHALL responder `409`

#### Scenario: Snapshot que no está en el repositorio
- **GIVEN** un `id` de instantánea que no existe en el almacenamiento del repositorio indicado
- **WHEN** se envía `POST /api/v1/snapshots/{id}/restore` con su `repository_id`
- **THEN** la API SHALL responder `404`
- **AND** NO SHALL publicarse ningún `restore_progress`
- **AND** NO SHALL lanzarse ninguna ejecución de restauración

#### Scenario: Target no restaurable
- **GIVEN** un snapshot de un target `mariadb`
- **WHEN** se pide restaurarlo
- **THEN** la API SHALL responder `422`

#### Scenario: Cierre de la restauración
- **GIVEN** una restauración ya lanzada
- **WHEN** el trabajo termina bien o falla
- **THEN** SHALL publicarse `restore_progress` con fase `terminada` o `fallida`, respectivamente
- **AND** los cuatro contadores del aviso SHALL seguir a `0`
- **AND** el `Result` del motor SHALL reflejar el mismo desenlace
