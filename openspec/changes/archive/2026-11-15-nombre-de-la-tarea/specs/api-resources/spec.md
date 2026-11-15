## MODIFIED Requirements

### Requirement: El listado de ejecuciones lleva filtros, paginación acotada y orden estable

`GET /api/v1/executions` SHALL admitir los filtros `job_id` y `status`, y la paginación `limit` y `offset`.

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

### Requirement: El listado de snapshots declara, por instantánea, el target del que procede

`GET /api/v1/snapshots?repository_id=<id>` SHALL añadir a **cada** elemento de `data` tres campos nuevos: `target_id`, `target_name` y `target_type`, resueltos por el mismo vínculo que usa la restauración —`snapshot → job_executions → backup_jobs → backup_targets`— y acotados al `repository_id` que se pide. Los tres SHALL ser anulables y SHALL publicarse como `null` cuando la instantánea no case con ninguna ejecución, tarea ni target de ese repositorio: una copia hecha por otra herramienta, o cuya fila en `job_executions` se borró en cascada al borrar su tarea. `null` SHALL ser un valor del contrato, no un hueco: un elemento con `target_id` a `null` es un elemento sin target conocido, y NO SHALL confundirse con un error del listado ni impedir que se listen las demás instantáneas.

El listado SHALL ser tolerante a la ausencia del vínculo —una consulta por elemento o una única unión externa, es indiferente—, y NO SHALL fallar, ni acortar el listado, ni ocultar la instantánea huérfana por el hecho de que no tenga target.

El porqué: el panel necesita saber **sobre qué target** se va a restaurar antes de confirmarlo, y si ese target es restaurable, y hoy el listado no lo dice. Sin estos tres campos, la confirmación no puede nombrar el destino —que es justo lo que evita escribir en el sitio equivocado— y la acción no puede saber si debe ofrecerse o explicar su limitación; y como el vínculo con el target puede faltar por causas legítimas, el contrato tiene que decir qué significa su ausencia en vez de dejar que cada cliente la adivine.

Cada elemento de `data` SHALL llevar además **`job_name`** (anulable): el nombre de la tarea de origen, resuelto por el vínculo `snapshot → job_executions → backup_jobs`, y publicado como `null` cuando la instantánea no case con ninguna tarea o la tarea no tenga nombre. La `label` de la instantánea NO SHALL tocarse: SHALL seguir siendo el identificador de la tarea, y los campos que el listado ya publicaba —`id`, `time`, `paths`, `label`, `files`, `bytes`, junto al target— SHALL seguir como están.

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

#### Scenario: Cada instantánea declara el nombre de su tarea o null

- **GIVEN** una instantánea cuya tarea tiene nombre, y otra del mismo repositorio cuya tarea no lo tiene
- **WHEN** se pide `GET /api/v1/snapshots?repository_id=<id>`
- **THEN** el elemento de la primera SHALL traer `job_name` con el nombre de su tarea
- **AND** el de la segunda SHALL traer `job_name` a `null`
- **AND** el campo `label` de ambas SHALL seguir siendo el identificador de la tarea, sin cambios

## ADDED Requirements

### Requirement: La tarea lleva un nombre legible

`POST /api/v1/jobs` y `PUT /api/v1/jobs/:id` SHALL aceptar un campo opcional `name` (texto). El valor SHALL recortarse; si queda vacío o en blanco SHALL guardarse como `null`; si supera **120** caracteres SHALL rechazarse con `400` y la envoltura, **nombrando `name`**.

`GET /api/v1/jobs` y `GET /api/v1/jobs/:id` SHALL publicar `name` (anulable) por tarea.

El nombre NO SHALL ser único ni obligatorio. El porqué: sin nombre el panel cae al identificador de la tarea, y el nombre es lo que distingue dos tareas sobre el mismo target —hoy el Histórico usa el nombre del target como proxy y confunde las dos—. Exigir unicidad obligaría a inventar sufijos para lo que es una etiqueta para el operador, y hacerlo obligatorio rompería las tareas que ya existen sin nombre.

#### Scenario: El alta guarda el nombre recortado
- **GIVEN** un alta de tarea con `name` con espacios a ambos lados de `copia nocturna`
- **WHEN** se guarda y se lee la tarea
- **THEN** su `name` SHALL ser `copia nocturna`, sin los espacios de los extremos
- **AND** SHALL conservarse el resto de campos de la tarea

#### Scenario: Un nombre en blanco se guarda como null
- **GIVEN** un alta de tarea con `name` a una cadena de solo espacios
- **WHEN** se guarda y se lee la tarea
- **THEN** su `name` SHALL ser `null`
- **AND** NO SHALL guardarse una cadena vacía

#### Scenario: Un nombre de más de 120 caracteres contesta 400 nombrando name
- **GIVEN** un alta o una edición de tarea con un `name` de 121 caracteres
- **WHEN** se envía
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el mensaje SHALL nombrar el campo `name`

#### Scenario: El listado de tareas publica el nombre anulable
- **GIVEN** una tarea con nombre y otra sin él
- **WHEN** se pide `GET /api/v1/jobs`
- **THEN** la primera fila SHALL traer su `name`
- **AND** la segunda SHALL traer `name` a `null`
