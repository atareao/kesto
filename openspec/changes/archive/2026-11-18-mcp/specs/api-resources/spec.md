# api-resources Specification

## MODIFIED Requirements

### Requirement: El listado de ejecuciones lleva filtros, paginación acotada y orden estable

`GET /api/v1/executions` SHALL admitir los filtros `id`, `job_id` y `status`, y la paginación `limit` y `offset`.

El filtro `id` SHALL ser opcional y SHALL devolver, como mucho, la fila con ese identificador. Existe para que el servidor MCP pueda resolver una ejecución concreta —la herramienta `get_execution`— sin estrenar una ruta de detalle: el recurso sigue siendo de solo lectura y su superficie no crece. Un `id` con forma inválida SHALL contestar `400`; un `id` válido sin fila SHALL devolver el listado vacío, como cualquier otro filtro que no case.

`limit` SHALL tener `50` como valor por defecto y **`200` de tope**, y un valor fuera de rango —por encima del tope o negativo— o no numérico SHALL contestar `400`. El tope existe porque el histórico es la tabla que más crece de las cuatro —una fila por intento de copia— y el panel pinta una página: sin tope, un `limit` grande materializaría la tabla entera en memoria y ocuparía una de las cinco conexiones del pool mientras el motor quiere escribir. Un valor fuera de rango es una petición mal formada, así que se contesta `400` y no se recorta en silencio: recortar dejaría al cliente creyendo que ha recibido lo que pidió.

El orden SHALL ser `started_at DESC, id`, y el `id` desempata porque las ejecuciones del **mismo segundo** empatan —un lote de tareas disparado por el mismo tick de cron arranca a la vez, y `CURRENT_TIMESTAMP` no tiene más resolución— y el orden tiene que ser **estable** entre dos llamadas: sin desempate, dos peticiones seguidas con el mismo `offset` pueden devolver una fila repetida y saltarse otra, y el panel paginaría de forma visiblemente rota.

Cada fila SHALL llevar sus **métricas** —los dos contadores de bytes y sus dos fechas—, que es el sentido del «y métricas» del §5: **no** hay endpoint de agregados, quien sume los totales es el panel, y para eso necesita los números de cada fila y no solo un resumen que la API no puede acotar por periodo.

Las fechas de cada fila SHALL publicarse como cadena RFC3339, como en el resto de la superficie.

Cada fila SHALL llevar además **`job_name`** (anulable): el nombre de la tarea de esa ejecución, resuelto por el vínculo con `backup_jobs`. SHALL publicarse como `null` cuando la tarea no tenga nombre o la ejecución no case con ninguna tarea, con la misma tolerancia que el resto del listado: un `job_name` a `null` es un nombre desconocido, no un error, y NO SHALL acortar el listado ni ocultar la fila.

#### Scenario: El listado sin filtros devuelve las ejecuciones con sus métricas
- **GIVEN** tres ejecuciones dadas de alta
- **WHEN** se pide el listado sin filtros
- **THEN** SHALL contestar `200` con las tres filas
- **AND** cada fila SHALL traer `bytes_processed`, `bytes_uploaded`, `started_at`, `finished_at` y `job_name`

#### Scenario: Los filtros por job_id y por status acotan el listado
- **GIVEN** ejecuciones de dos tareas distintas, con estados distintos
- **WHEN** se pide el listado con `job_id` y con `status`
- **THEN** cada llamada SHALL devolver solo las filas que cumplen su filtro
- **AND** los dos filtros SHALL poder usarse a la vez

#### Scenario: El filtro por id devuelve una sola ejecución
- **GIVEN** ejecuciones de dos tareas distintas
- **WHEN** se pide el listado con el `id` de una de ellas
- **THEN** SHALL devolver solo esa fila
- **AND** SHALL poder combinarse con `job_id` y `status`

#### Scenario: Un id válido sin fila devuelve el listado vacío
- **GIVEN** el router con una sesión válida y un identificador que no existe
- **WHEN** se pide el listado con `id`
- **THEN** SHALL contestar `200` con una lista vacía
- **AND** NO SHALL contestar `404`

#### Scenario: Un id con forma inválida contesta 400
- **GIVEN** el router con una sesión válida
- **WHEN** se pide el listado con `id=no-es-un-uuid`
- **THEN** SHALL contestar `400` con la envoltura

#### Scenario: limit acota y offset avanza
- **GIVEN** un histórico con más de una fila
- **WHEN** se pide el listado con `limit=1` y después con `limit=1&offset=1`
- **THEN** cada llamada SHALL devolver una fila
- **AND** la segunda fila SHALL ser distinta de la primera

#### Scenario: Un limit fuera de rango o no numérico contesta 400
- **GIVEN** el router con una sesión válida
- **WHEN** se pide el listado con `limit=201` y con `limit=abc`
- **THEN** las dos SHALL contestar `400` con la envoltura
- **AND** NO SHALL devolverse un listado recortado en silencio

#### Scenario: Dos llamadas seguidas dan el mismo orden con filas empatadas
- **GIVEN** dos ejecuciones con el mismo `started_at`
- **WHEN** se piden dos listados seguidos
- **THEN** el orden de las filas SHALL ser el mismo en las dos respuestas
- **AND** el desempate SHALL ser el identificador

#### Scenario: Las fechas de una fila de ejecución son cadenas RFC3339
- **GIVEN** una ejecución con su `started_at` y su `finished_at`
- **WHEN** se lee el listado
- **THEN** las dos SHALL llegar como cadenas
- **AND** SHALL poder releerse con `OffsetDateTime::parse(…, &Rfc3339)`
