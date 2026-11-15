## MODIFIED Requirements

### Requirement: La restauración de un snapshot se pide por su recurso y usa el target de origen

El panel SHALL poder pedir la restauración de un snapshot completo mediante `POST /api/v1/snapshots/{id}/restore`, con el `repository_id` en el cuerpo y, para un target de base de datos, la confirmación de sobrescritura `sobrescribir: true`. La petición SHALL resolverse contra la ejecución que produjo el snapshot y su tarea, y SHALL restaurar sobre el target de origen. La respuesta SHALL seguir la envoltura de `api-shell` y SHALL incluir, en `data`, el identificador de la ejecución de restauración —que SHALL ser la fila persistida en `restore_executions`—, el `snapshot_id`, el `target_id` y un nombre legible del target.

El endpoint SHALL comprobar, **antes de contestar**, que la instantánea existe en el almacenamiento del repositorio indicado; si no está, SHALL responder `404` y NO SHALL lanzar ninguna ejecución ni publicar ningún aviso. El `200` NO SHALL ser optimista: no SHALL devolverse por el solo hecho de que el `repository_id` resuelva, sino solo cuando la instantánea se haya encontrado de verdad.

Si el snapshot existe pero no está asociado a ninguna ejecución o target, la API SHALL responder `409` con un mensaje claro. Si el target es un `directory` sin `host_path`, la API SHALL responder `422` con su propio motivo. Si el target es de base de datos y **falta la confirmación de sobrescritura**, la API SHALL responder `428 Precondition Required` explicando que hace falta confirmar, sin lanzar ningún trabajo.

El progreso SHALL publicarse en el stream global de `/api/v1/events` con `event: restore_progress` y un payload con `execution_id`, `snapshot_id`, `phase`, `files_done`, `files_total`, `bytes_done` y `bytes_total`. La fase SHALL ser `restaurando` al arrancar y SHALL publicarse un aviso de cierre con fase `terminada` si la restauración termina bien o `fallida` si falla; el cierre SHALL publicarse en ambos desenlaces. Los cuatro contadores SHALL seguir publicándose a `0` en esta versión y NO SHALL interpretarse como «cero ficheros»: `Repository::dump`/`restore` de `rustic_core` 0.13 no ofrecen callback de progreso por su API pública.

El porqué: restaurar es la operación que hace útil una copia. La restauración de un target de base de datos pisa datos vivos, así que su petición exige una confirmación explícita; y como hoy el desenlace no se podía seguir después del `200`, la ejecución se persiste.

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

- **GIVEN** un snapshot de un target `directory` cuyo `host_path` falta o está en blanco
- **WHEN** se pide restaurarlo
- **THEN** la API SHALL responder `422` nombrando el campo que falta
- **AND** NO SHALL lanzarse ninguna ejecución de restauración

#### Scenario: Cierre de la restauración

- **GIVEN** una restauración ya lanzada
- **WHEN** el trabajo termina bien o falla
- **THEN** SHALL publicarse `restore_progress` con fase `terminada` o `fallida`, respectivamente
- **AND** la fila de `restore_executions` SHALL quedar en `success` o `failed`, respectivamente

#### Scenario: Restaurar un snapshot de un target de base con confirmación

- **GIVEN** un snapshot de un target `mariadb`
- **WHEN** se envía `POST /api/v1/snapshots/{id}/restore` con su `repository_id` y `sobrescribir: true`
- **THEN** la respuesta SHALL ser `200` con `data.execution_id`
- **AND** SHALL publicarse `restore_progress`

#### Scenario: Restaurar un target de base sin confirmación

- **GIVEN** un snapshot de un target `postgresql`
- **WHEN** se envía `POST /api/v1/snapshots/{id}/restore` con su `repository_id` y sin `sobrescribir`
- **THEN** la API SHALL responder `428`
- **AND** NO SHALL lanzarse ninguna ejecución de restauración

## ADDED Requirements

### Requirement: El histórico de restauraciones se lista con filtros y paginación

SHALL existir `GET /api/v1/restorations` que devuelva las ejecuciones de restauración persistidas, ordenadas por fecha de comienzo descendente, con paginación acotada y los filtros `status` y `target_id`, y SHALL seguir la envoltura de `api-shell`. Cada fila SHALL publicar `id`, `snapshot_id`, `target_id`, `target_name`, `target_type`, `status`, `error_message`, `started_at` y `finished_at`, con los identificadores como texto en su forma canónica con guiones y las fechas como cadenas RFC3339. `target_name` y `target_type` SHALL resolverse desde el target referenciado por `target_id`, y SHALL publicarse como `null` solo si `target_id` es `null`.

La ruta SHALL colgar de la mitad protegida del router, como el resto de `/api/v1/*`, y NO SHALL añadirse a la lista blanca de rutas públicas.

El porqué: la ejecución de restauración deja de ser efímera; sin un listado, el registro existiría pero nadie podría verlo, y el panel no tendría de dónde pintar el histórico.

#### Scenario: El listado devuelve las restauraciones

- **GIVEN** dos restauraciones persistidas
- **WHEN** se pide `GET /api/v1/restorations`
- **THEN** SHALL devolver las dos, la más reciente primero
- **AND** cada fila SHALL publicar su `status` y sus fechas

#### Scenario: El filtro por estado acota el listado

- **GIVEN** restauraciones en estados distintos
- **WHEN** se pide `GET /api/v1/restorations?status=failed`
- **THEN** SHALL devolver solo las fallidas

#### Scenario: El listado pagina de forma acotada

- **GIVEN** más restauraciones que el tope de una página
- **WHEN** se pide `GET /api/v1/restorations`
- **THEN** SHALL devolver una página acotada con el total
- **AND** la siguiente página SHALL traer las restantes

#### Scenario: El listado exige sesión cuando el modo la exige

- **GIVEN** el modo de autenticación habilitado
- **WHEN** se pide `GET /api/v1/restorations` sin sesión
- **THEN** SHALL responder como el resto de rutas protegidas
