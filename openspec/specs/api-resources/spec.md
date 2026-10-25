# api-resources Specification

## Purpose
La superficie de recursos del §5: las rutas con las que el panel da de alta, consulta, edita y borra las fuentes de respaldo, los repositorios S3 y las tareas programadas, más el listado del histórico de ejecuciones con sus filtros, su paginación acotada y sus métricas, el disparo manual de una tarea, el stream de eventos en vivo y el explorador de snapshots. Es el consumidor de la capa de acceso del §3, así que recibe el pool abierto y migrado al arrancar —lo lleva al estado de la aplicación y no abre una segunda base— y sirve cada fila con tipos propios de la API en vez de las entidades de la base, con los identificadores como texto en su forma canónica con guiones y las fechas como cadenas RFC3339. Valida lo que la base no puede validar sola —la coherencia de los campos de un target con su tipo y la existencia de las referencias de una tarea antes de escribir— y devuelve, por cada credencial, un booleano que dice si está configurada en lugar del valor. Y el alta de un repositorio no es un apunte en la base: conecta o inicializa el repositorio de verdad.

## Requirements

### Requirement: El estado de la aplicación lleva el pool y las cuatro capas de acceso

El pool abierto y migrado al arrancar SHALL entregarse al estado de la aplicación —el `AppState`—, y los manejadores SHALL obtener de él las **cuatro** capas de acceso del §3 —repositorios, targets, tareas y ejecuciones—, cada una sobre el mismo pool y la misma clave de cifrado. NO SHALL abrirse una segunda base por consumidor, ni en los manejadores ni en las pruebas que los ejercitan.

El porqué está escrito en la spec de `db`: allí se exige que el pool se abra **una sola vez** y sea el **único** que la aplicación use, y el traspaso al estado se aplazó a propósito a este change, que estrena el primer consumidor. Lo que se añade aquí es lo que hace alcanzable aquel requisito: sin este traspaso, el primer consumidor que quisiera escribir tendría que abrir su propia base, y entonces la regla de `db` se quedaría en el papel.

Un segundo pool sobre el mismo fichero no da la cara en el acto: SQLite lo acepta, WAL tolera varias conexiones y las dos ven las filas ya confirmadas. Lo que rompe es lo de dentro de una escritura —una fila que un manejador acaba de escribir y otro no ve hasta que la transacción cierra—, y el síntoma sería una petición que contesta `404` sobre un recurso que existe. Es un fallo intermitente, que es la peor clase de fallo: no se reproduce en la máquina de quien lo busca.

#### Scenario: El estado lleva el pool y las cuatro capas
- **GIVEN** un estado de la aplicación construido al arrancar
- **WHEN** se consulta el estado
- **THEN** SHALL llevar el pool y las cuatro capas de acceso del §3
- **AND** las cuatro SHALL compartir ese pool y la clave de cifrado del proceso

#### Scenario: Dos peticiones seguidas al mismo router ven lo mismo
- **GIVEN** un router construido sobre ese estado
- **WHEN** una petición da de alta un recurso y la siguiente pide su lectura
- **THEN** la segunda SHALL ver la fila que escribió la primera
- **AND** la lectura NO SHALL contestar `404`

#### Scenario: La escritura fue a la base del proceso y no a una copia
- **GIVEN** un recurso dado de alta por una petición al router, sobre un `DATABASE_URL` conocido
- **WHEN** se lee la fila **con SQL directo** sobre ese mismo fichero, sin pasar por la API
- **THEN** la fila SHALL estar en la tabla
- **AND** SHALL tener los valores que informó la petición

### Requirement: Cada recurso se sirve en cinco rutas, y las tres que el §5 no lista se declaran como añadido al SRS

`/api/v1/targets`, `/api/v1/repositories` y `/api/v1/jobs` SHALL servir cinco rutas cada uno: `GET` —el listado—, `POST` —el alta—, `GET /:id`, `PUT /:id` y `DELETE /:id`.

`GET /:id`, `PUT /:id` y `DELETE /:id` **no están en el §5** y SHALL declararse como **añadido** al SRS, con el motivo escrito, igual que se declararon los cuatro índices que `db-schema` añadió al §3 en vez de colarlos. El motivo es doble. Primero, la capa de acceso ya implementa `leer`, `actualizar` y `borrar` para las cuatro tablas, y hoy **solo las pruebas la ejercitan**: un recurso con una errata no se puede corregir desde el panel, y una tarea que ya no se quiere no se puede quitar, así que la única salida sería editar la base a mano —justo lo que el panel existe para evitar—. Segundo, el panel necesita la lectura de una fila concreta para pintar el formulario de edición, y deducirla filtrando el listado en el cliente sería volver a recorrer la tabla entera para leer una fila.

`PUT /:id` SHALL ser **reemplazo completo** de los campos mutables, y `id` y `created_at` NO SHALL entrar en él: el identificador no se reescribe —cambiarlo por el cuerpo sería crear otra fila donde ya había una y dejar huérfanas las referencias— y `created_at` es la fecha de nacimiento de la fila, no un campo editable, así que reescribirla cambiaría el orden de los listados. La única excepción a la sustitución completa son las credenciales, y la fija un requisito posterior de esta capability: un secreto **ausente** conserva el valor almacenado.

El listado SHALL contestar `200` con un **array** en `data`, y el listado de una tabla sin filas SHALL contestar `200` con un **array vacío**. NO SHALL contestar `404`, que diría que la colección no existe, y NO SHALL contestar `data` nulo, que obligaría al panel a distinguir dos formas de vacío —la colección sin filas y la colección ausente— cuando solo hay una.

#### Scenario: Cada una de las cinco rutas contesta lo suyo
- **GIVEN** el router con una sesión válida
- **WHEN** se recorren las cinco rutas de un recurso —el listado, el alta, la lectura por identificador, la actualización y el borrado—
- **THEN** cada una SHALL contestar el código que le corresponde con la envoltura
- **AND** las cinco SHALL existir, sin contestar `404` ninguna por no estar declarada

#### Scenario: El listado contesta 200 con un array en data
- **GIVEN** dos recursos dados de alta
- **WHEN** se pide el listado
- **THEN** SHALL contestar `200`
- **AND** `data` SHALL ser un array con los dos

#### Scenario: Una tabla vacía lista un array vacío
- **GIVEN** una base migrada sin ninguna fila en la tabla
- **WHEN** se pide el listado
- **THEN** SHALL contestar `200`
- **AND** `data` SHALL ser un array vacío
- **AND** NO SHALL contestar `404` ni `data` nulo

### Requirement: Las respuestas llevan tipos propios de la API y nunca las entidades de la base

Las respuestas SHALL serializarse con tipos declarados en `src/api/`, NO SHALL serializarse las entidades de `src/db/entities/`, y los identificadores SHALL viajar como **texto** en su forma canónica con guiones, de modo que `Uuid::parse_str` los acepte.

Los motivos están medidos y se escriben porque ninguno se deduce leyendo el código:

- `SecretString` **no implementa `Serialize`**: deriva `Clone` y tiene `Debug` y `Display` redactados a mano, pero no hay impl de serialización, así que una respuesta que llevara un secreto **no compila**. Eso es una red, no una convención: la regla «un secreto no vuelve al cliente» deja de depender de que alguien se acuerde y pasa a ser un fallo de compilación. Serializar las entidades de `src/db/entities/` exigiría añadir esa impl, y añadirla es justo lo que convierte cada `GET` en una fuga.
- `uuid` está declarado con la feature `v4` y **sin `serde`**, así que un `Uuid` **no se puede serializar** —y por eso los identificadores viajan como texto, que además es lo que quiere el panel y lo que lee cualquiera con `curl`—. La trampa del §3 no puede reaparecer en el cable: `sqlx` escribe un `Uuid` como BLOB de 16 bytes, y por eso la capa de acceso guarda y lee la forma con guiones; si el tipo de la respuesta fuera la entidad tal cual, ese BLOB tendría un segundo camino para salir.

Los nombres de los campos SHALL ser los del §3, para que el formulario del panel, el cuerpo de la petición y la columna de la tabla se llamen igual y no haya tres vocabularios para un dato.

La excepción son los booleanos por credencial del requisito «Un secreto no vuelve al cliente y la edición conserva el que no se envía»: `has_secret_access_key` y `has_passphrase` **no son campos del §3** —no hay ninguna columna con ese nombre, y el cuerpo del alta informa `secret_access_key`— y son un **añadido a este contrato**, como lo son las tres rutas de `/:id`. Su nombre antepone `has_` al de la columna para que se lean como un sí/no **sobre** ella —«el `secret_access_key` de la tabla, dicho como sí/no»— y no como un segundo vocabulario para el mismo dato.

#### Scenario: El alta devuelve los campos con los nombres del §3
- **GIVEN** un alta de repositorio atendida
- **WHEN** se lee el cuerpo de la respuesta
- **THEN** los campos SHALL llamarse como las columnas del §3 —`name`, `endpoint`, `bucket`, `region`, `access_key_id`—
- **AND** NO SHALL aparecer ningún campo que no exista en la tabla, salvo el booleano por credencial que exige el requisito «Un secreto no vuelve al cliente y la edición conserva el que no se envía»

#### Scenario: El identificador llega como cadena con guiones
- **GIVEN** un recurso dado de alta, con su identificador generado por la capa de acceso
- **WHEN** se lee el `id` del cuerpo de la respuesta
- **THEN** SHALL ser una cadena con los guiones de la forma canónica
- **AND** `Uuid::parse_str` SHALL aceptarla sin error

### Requirement: Los códigos del recurso: 201 al crear, 200 al leer, actualizar y borrar, y 404 cuando la fila no existe

`POST` SHALL contestar `201` con el recurso creado, y ese cuerpo SHALL traer el `id` que **generó la capa de acceso**: quien llama no lo informa, así que si la respuesta no lo devolviera no habría forma de leer, editar ni borrar esa fila recién creada sin salir a buscarla en el listado. El reparto viene de la spec de `db` —el identificador se genera en la capa de acceso, no en la base, porque SQLite no tiene `uuid()` y un valor por defecto en el DDL obligaría a interpolarlo—.

`GET`, `GET /:id`, `PUT /:id` y `DELETE /:id` SHALL contestar `200` con la envoltura, y `GET /:id`, `PUT /:id` y `DELETE /:id` sobre una fila que no existe SHALL contestar `404`. Ese `404` es posible porque la lectura de la capa de acceso devuelve un resultado que **distingue «no está» de «no se pudo leer»** —lo exige la spec de `db`—: confundir los dos casos daría un `404` sobre una base rota y un `500` sobre un recurso que simplemente no está.

Y una decisión que hay que explicar porque va contra la costumbre: **`DELETE` contesta `200` con `data` nulo y NO `204`**. El contrato de `api-shell` exige que **toda** respuesta de la API sea JSON con `status` y `message`, y un `204` no tiene cuerpo: sería la única respuesta capaz de romper la envoltura que la API se impone, y el panel tendría una rama especial —«si el código es 204, no leas el cuerpo»— para un solo endpoint.

El `201` NO SHALL acompañarse de cabecera `Location`: el cliente recibe el recurso en el cuerpo, y una cabecera que apunte a una ruta que solo existe si se cumplen las condiciones anteriores sería un segundo contrato que nadie mantiene.

#### Scenario: El alta contesta 201 y su id sirve para el GET siguiente
- **GIVEN** una petición de alta con un cuerpo válido
- **WHEN** se atiende
- **THEN** SHALL contestar `201`
- **AND** el cuerpo SHALL traer el recurso creado con su `id`
- **AND** un `GET /:id` con ese `id` SHALL contestar `200` con la misma fila

#### Scenario: Un identificador que no existe contesta 404
- **GIVEN** una base migrada sin la fila pedida
- **WHEN** se pide `GET /:id` con un identificador bien formado que no existe
- **THEN** SHALL contestar `404`
- **AND** el cuerpo SHALL ser JSON con `status` y `message`

#### Scenario: El DELETE contesta 200 con data nulo y la lectura posterior 404
- **GIVEN** una fila dada de alta
- **WHEN** se pide `DELETE /:id`
- **THEN** SHALL contestar `200` con `data` nulo
- **AND** NO SHALL contestar `204`
- **AND** el `GET /:id` posterior SHALL contestar `404`

#### Scenario: El PUT sobre una fila que no existe contesta 404
- **GIVEN** una base migrada sin la fila pedida
- **WHEN** se pide `PUT /:id` con un cuerpo válido
- **THEN** SHALL contestar `404`
- **AND** NO SHALL escribirse ninguna fila

### Requirement: Un identificador de ruta que no es un UUID contesta 400 con el motivo

Un `:id` de ruta que no parsea como UUID SHALL contestar `400` con el motivo, y NO SHALL contestar `500` ni `404`.

El porqué: un identificador mal formado es una **petición** mal formada, no un recurso ausente. Un `500` mandaría a quien pregunta a buscar el fallo en el servidor —y a quien lea el registro de la API, a buscar una excepción que no existe—, y un `404` le diría que el recurso no está cuando lo que está mal es el identificador que escribió: el cliente que recibiera un `404` por una errata de teclado podría llegar a concluir que su recurso se ha borrado.

El mensaje SHALL nombrar el parámetro de ruta, no el valor: un identificador no es una credencial, pero la regla del resto de la API —nombrar el campo, nunca el valor— se aplica igual para que el trato sea uno solo y no haya que decidir caso a caso cuál se puede repetir.

#### Scenario: Un identificador mal formado contesta 400 con el motivo
- **GIVEN** el router con una sesión válida
- **WHEN** se pide `GET /api/v1/targets/no-es-un-uuid`
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el mensaje SHALL explicar que el identificador no es válido
- **AND** NO SHALL contestar `500` ni `404`

### Requirement: Los campos del target son coherentes con su tipo, y las referencias existen antes de escribir

`target_type = directory` SHALL exigir `host_path` y NO SHALL admitir `container_name`, `db_user`, `db_password` ni `db_name`. `target_type = mariadb` y `target_type = postgresql` SHALL exigir `container_name`, `db_user` y `db_password`, NO SHALL admitir `host_path`, y SHALL admitir `db_name` como campo opcional, donde informado significa el volcado de **esa sola base** y ausente significa el volcado global —la semántica que fijó `db-schema` y que el motor implementará—.

El porqué: la tabla del §3 **acepta** las dos combinaciones incoherentes. Sus columnas son anulables, y su único `CHECK` restringe `target_type`, así que un `directory` con `container_name` y sin `host_path` es una fila que la base guarda tan contenta y que el motor no sabrá usar; el fallo aparecería en la primera copia, como un error que habla de un contenedor que no existe y no de un formulario mal rellenado. La coherencia se exige, por tanto, en el borde HTTP, que es el único sitio donde se conoce la intención de quien escribe.

El alta de una tarea SHALL comprobar que `target_id` y `repository_id` **existen** antes de escribir, y SHALL contestar `400` nombrando el campo que no resuelve, para que el cliente reciba un motivo preciso —«el target indicado no existe»— en vez del error de una restricción, que no distingue si lo que falló fue el target o el repositorio. Esa comprobación NO SHALL sustituir a la clave foránea del §3, que sigue siendo la última línea: entre la comprobación y el `INSERT` cabe otra petición que borre la fila referenciada, y ese hueco lo cubre la base, no el manejador.

Los campos **desconocidos** del cuerpo SHALL rechazarse con `400`. Sin esta regla, una errata en el nombre de un campo —`endpont`— se interpreta como un campo que nadie informa y el valor se pierde en silencio: la petición contesta `201` y el recurso queda escrito sin el dato que quien llamó creía haber mandado. Rechazarlo convierte ese silencio en un error que nombra el campo de más.

#### Scenario: Un directory con container_name se rechaza
- **GIVEN** un alta de target con `target_type` a `directory`, su `host_path` y un `container_name`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `container_name`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un mariadb sin db_password se rechaza nombrando el campo
- **GIVEN** un alta de target con `target_type` a `mariadb`, su `container_name` y su `db_user`, y sin `db_password`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400`
- **AND** el mensaje SHALL nombrar el campo `db_password`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un db_name ausente es NULL en la columna
- **GIVEN** un alta de target de base de datos que no informa `db_name`
- **WHEN** se lee la fila y se mira la columna `db_name` con SQL directo
- **THEN** el valor almacenado SHALL ser `NULL`
- **AND** NO SHALL ser la cadena vacía

#### Scenario: Una tarea con un target_id inexistente no escribe nada
- **GIVEN** un alta de tarea cuyo `target_id` no existe
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `target_id`
- **AND** la tabla de tareas NO SHALL tener ninguna fila nueva

#### Scenario: Un campo de más en el cuerpo se rechaza
- **GIVEN** un alta con un cuerpo válido al que se le añade un campo que no existe en el §3
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400`
- **AND** NO SHALL escribirse ninguna fila

### Requirement: Los mensajes de error nombran el campo y nunca su valor

El mensaje de un `400` de estas rutas SHALL identificar el **campo** que no cumple, y NO SHALL incluir el **valor** recibido.

El motivo: los cuerpos de estas peticiones llevan credenciales —`secret_access_key`, `passphrase`, `db_password`—, y un mensaje que repita el valor lo pone en el cuerpo de la respuesta y, con él, en cualquier registro intermedio: el del proxy, el del navegador, el del `journalctl` del contenedor. Quien depura un alta que falla necesita saber **qué campo** estaba mal; el valor ya lo tiene delante quien lo envió, así que repetirlo no añade información a quien pregunta y se la añade a quien no debería tenerla.

La regla no distingue entre campos de secreto y el resto, y es a propósito: una excepción para los campos que no son sensibles se aplica mal el día que un campo deja de serlo, y decidir caso a caso es exactamente lo que se olvida.

#### Scenario: El 400 nombra el campo y no filtra el valor
- **GIVEN** un alta con una credencial de valor distinguible que incumple lo exigido —un secreto vacío o un valor con el tipo equivocado—
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400`
- **AND** el cuerpo SHALL contener el **nombre** del campo
- **AND** el cuerpo NO SHALL contener el valor de la credencial

### Requirement: Un secreto no vuelve al cliente y la edición conserva el que no se envía

La respuesta de un repositorio NO SHALL traer `secret_access_key` ni `passphrase`, y la de un target NO SHALL traer `db_password`. En su lugar, cada credencial SHALL representarse por un **booleano** que diga si está configurada.

El booleano y no el valor porque el panel no necesita el valor para pintar el formulario: necesita saber si hay credencial guardada para mostrar el campo como configurado y no obligar a reescribirla en cada edición. Devolver el valor convertiría cada `GET` del listado en una fuga —una respuesta que viaja al navegador y queda en el historial y en las herramientas de desarrollo—, y la red de tipo que describe un requisito anterior de esta capability existe precisamente para que eso no se pueda escribir por descuido.

En un `PUT`, un campo de secreto **ausente** SHALL conservar el valor almacenado: el manejador lee la fila y reutiliza el secreto que ya estaba. Y uno **vacío** SHALL rechazarse con `400` nombrando el campo.

El porqué de las dos mitades: la capa de acceso ya rechaza el vacío al escribir —`DbError::SecretoVacio`—, así que sin la primera regla un formulario de edición que no rellena la contraseña la sustituiría por nada, y el daño aparecería en la siguiente copia; y sin la segunda, ese mismo formulario pasaría el valor vacío y recibiría el error de la base, que al cliente le llega como `500` genérico. La segunda mitad convierte ese `500` en un `400` que dice qué campo falta. Y **ausente** y **vacío** NO SHALL tratarse igual: lo primero es «no lo toques» y lo segundo es «bórralo», y el borrado de una credencial no es una operación que este change ofrezca.

#### Scenario: El alta y su lectura no traen secretos y sí el booleano
- **GIVEN** un repositorio dado de alta con su `secret_access_key` y su `passphrase`, y un target con su `db_password`
- **WHEN** se leen por la API
- **THEN** el cuerpo NO SHALL contener ninguno de los tres valores
- **AND** SHALL traer un booleano por credencial con el valor de que está configurada

#### Scenario: Un PUT sin el secreto conserva el almacenado
- **GIVEN** un repositorio con su `passphrase` almacenada
- **WHEN** se actualiza con un cuerpo que no informa `passphrase` y cambia otros campos
- **THEN** la `passphrase` almacenada SHALL seguir siendo **la misma** que había
- **AND** los campos informados SHALL haber cambiado

#### Scenario: Un PUT con el secreto vacío se rechaza
- **GIVEN** un repositorio con su `passphrase` almacenada
- **WHEN** se actualiza con `passphrase` a cadena vacía
- **THEN** SHALL contestar `400`
- **AND** el mensaje SHALL nombrar el campo
- **AND** la `passphrase` almacenada NO SHALL haber cambiado

#### Scenario: Ningún campo de secreto aparece en ninguna respuesta
- **GIVEN** un alta, una lectura, una actualización y un borrado sobre recursos con credenciales
- **WHEN** se revisan los cuerpos de todas las respuestas
- **THEN** ninguno SHALL contener `secret_access_key`, `passphrase` ni `db_password`
- **AND** ninguno SHALL contener el valor de las credenciales dadas de alta

### Requirement: El listado de ejecuciones lleva filtros, paginación acotada y orden estable

`GET /api/v1/executions` SHALL admitir los filtros `job_id` y `status`, y la paginación `limit` y `offset`.

`limit` SHALL tener `50` como valor por defecto y **`200` de tope**, y un valor fuera de rango —por encima del tope o negativo— o no numérico SHALL contestar `400`. El tope existe porque el histórico es la tabla que más crece de las cuatro —una fila por intento de copia— y el panel pinta una página: sin tope, un `limit` grande materializaría la tabla entera en memoria y ocuparía una de las cinco conexiones del pool mientras el motor quiere escribir. Un valor fuera de rango es una petición mal formada, así que se contesta `400` y no se recorta en silencio: recortar dejaría al cliente creyendo que ha recibido lo que pidió.

El orden SHALL ser `started_at DESC, id`, y el `id` desempata porque las ejecuciones del **mismo segundo** empatan —un lote de tareas disparado por el mismo tick de cron arranca a la vez, y `CURRENT_TIMESTAMP` no tiene más resolución— y el orden tiene que ser **estable** entre dos llamadas: sin desempate, dos peticiones seguidas con el mismo `offset` pueden devolver una fila repetida y saltarse otra, y el panel paginaría de forma visiblemente rota.

Cada fila SHALL llevar sus **métricas** —los dos contadores de bytes y sus dos fechas—, que es el sentido del «y métricas» del §5: **no** hay endpoint de agregados, quien sume los totales es el panel, y para eso necesita los números de cada fila y no solo un resumen que la API no puede acotar por periodo.

Las fechas de cada fila SHALL publicarse como cadena RFC3339, como en el resto de la superficie.

#### Scenario: El listado sin filtros devuelve las ejecuciones con sus métricas
- **GIVEN** tres ejecuciones dadas de alta
- **WHEN** se pide el listado sin filtros
- **THEN** SHALL contestar `200` con las tres filas
- **AND** cada fila SHALL traer `bytes_processed`, `bytes_uploaded`, `started_at` y `finished_at`

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

### Requirement: Las fechas del cable son RFC3339 y no la serialización por defecto de time

Las fechas SHALL publicarse como **cadena** en RFC3339 y en UTC, y NO SHALL publicarse con el `Serialize` que `time` trae tal cual.

El porqué está medido: en `time` 0.3.55, `serde-human-readable` es una feature **aparte** de `serde` —el `Cargo.toml` declara `serde = ["dep:serde_core", …]` y `serde-human-readable = ["serde","formatting","parsing"]`—, y este crate declara `time` con `["formatting","parsing","macros","serde"]`, **sin** `serde-human-readable`. Así que el `Serialize` de `time` no produce una cadena: el panel recibiría algo que no puede pintar y que `Date` de JavaScript no entiende.

El change formatea las fechas por su cuenta con `OffsetDateTime::format` y `Rfc3339` —la feature `formatting` sí está— en vez de encender la feature que falta. Encenderla cambiaría el formato del cable de forma implícita, y el formato del cable es parte del contrato: el panel lo va a leer y un cambio de feature en el `Cargo.toml` no tiene por qué cambiarlo. Formatear explícitamente deja el formato escrito en el sitio donde se emite.

El desplazamiento SHALL ser cero: la base guarda UTC, como fija la spec de `db`, y dejar el desplazamiento que tuviera el proceso haría que la misma fila se publicara distinta según la zona horaria del contenedor.

#### Scenario: Una fecha del cable se puede releer y da el mismo instante
- **GIVEN** una fecha publicada por cualquiera de las rutas de recursos
- **WHEN** se vuelve a leer con `OffsetDateTime::parse(…, &Rfc3339)`
- **THEN** SHALL dar el mismo instante que la fecha almacenada
- **AND** su valor SHALL ser una cadena
- **AND** NO SHALL ser una secuencia de números

### Requirement: Las rutas nuevas cuelgan de la mitad protegida sin tocar la partición

Las rutas de esta capability NO SHALL declararse en la mitad pública del router y NO SHALL añadirse excepciones a la lista blanca de `api-shell`.

El porqué es la forma de la partición: la lista blanca deja solo la salud y las dos rutas del flujo de entrada del lado público, y todo lo demás queda protegido **por construcción**, así que una ruta nueva está detrás del *gate* sin que nadie tenga que acordarse. Añadir una excepción para una ruta de recursos invertiría eso —habría que acordarse de **no** añadirla— y abriría un recurso de negocio a quien no ha entrado, que es exactamente lo que la lista blanca existe para impedir.

Este change NO SHALL invertir el orden de construcción del router ni tocar el `method_not_allowed_fallback` de la mitad protegida. Ese orden es semántico y ya está documentado en `api-shell`: declarar el *fallback* de métodos **después** de colgar la capa hace que un método no declarado conteste `405` **sin haber exigido sesión**, revelando a quien no ha entrado qué métodos existen. Las rutas de recursos multiplican la superficie donde eso se nota —cinco rutas por recurso—, así que la partición se comprueba sobre ellas, no solo sobre el flujo de entrada.

#### Scenario: Sin cookie, cada ruta nueva contesta 401
- **GIVEN** el router en modo `Oidc` y sin cookie de sesión
- **WHEN** se pide cada una de las rutas de los tres recursos, de lectura y de escritura
- **THEN** todas SHALL contestar `401` con la envoltura
- **AND** ninguna SHALL contestar `404`

#### Scenario: Con la autenticación apagada las rutas nuevas contestan sin sesión
- **GIVEN** una configuración sin ninguna variable `OIDC_*`
- **WHEN** se pide una ruta de recursos sin sesión
- **THEN** SHALL atenderla, porque en ese modo no se monta ninguna capa
- **AND** NO SHALL contestar `401`

#### Scenario: Un método no declarado sin sesión contesta 401 y no 405
- **GIVEN** el router en modo `Oidc` y una ruta de recursos sin cookie
- **WHEN** se pide esa ruta con un método que no está declarado
- **THEN** SHALL contestar `401` con la envoltura
- **AND** NO SHALL contestar `405`

### Requirement: El alta de un repositorio conecta con él, y lo inicializa si todavía no existe

`POST /api/v1/repositories` SHALL, además de escribir la fila, poner el repositorio en marcha: SHALL conectar con el destino descrito por el cuerpo y, si ese repositorio no existe todavía, SHALL inicializarlo. La respuesta SHALL decir cuál de los dos desenlaces ocurrió. Si el destino no se alcanza o la credencial no abre un repositorio que ya existe, el alta SHALL fallar y NO SHALL dejar fila en la base.

El §5 describe la ruta con cuatro palabras —«Inicializar o conectar repositorio S3/Rustic»— y hasta ahora solo se hacía la mitad de atrás: se guardaba una fila que describía un repositorio con el que nadie había hablado. El resultado es un alta que contesta `201` sobre un bucket que no existe, una `passphrase` con una errata o un endpoint mal escrito, y el error aparece mucho después, al ejecutar la primera copia, cuando ya nadie lo relaciona con el alta. Hablar con el destino en el momento del alta es lo que convierte esos tres errores en un `400` o un `502` con motivo, en la misma petición en la que se cometieron.

Que no se deje fila es la otra mitad y no es un detalle de estilo: una fila que describe un repositorio inalcanzable hace que el panel lo liste como disponible, que una tarea pueda apuntarle y que el fallo se manifieste como una ejecución fallida en vez de como un alta rechazada. Es más barato no escribir que limpiar.

#### Scenario: Un alta contra un destino que ya tiene repositorio conecta
- **GIVEN** un destino que ya contiene un repositorio abierto con esa passphrase
- **WHEN** se da de alta
- **THEN** SHALL contestar `201` con la envoltura
- **AND** la respuesta SHALL decir que se ha conectado
- **AND** SHALL quedar la fila en la base

#### Scenario: Un alta contra un destino vacío lo inicializa
- **GIVEN** un destino alcanzable sin ningún repositorio
- **WHEN** se da de alta
- **THEN** SHALL contestar `201` con la envoltura
- **AND** la respuesta SHALL decir que se ha inicializado
- **AND** el destino SHALL quedar con un repositorio

#### Scenario: Un alta contra un destino inalcanzable no deja fila
- **GIVEN** un body con un endpoint que no responde
- **WHEN** se da de alta
- **THEN** SHALL contestar con un error y la envoltura
- **AND** NO SHALL quedar fila en la base
- **AND** el error NO SHALL publicar la clave secreta

#### Scenario: Un alta cuya passphrase no abre el repositorio existente no deja fila
- **GIVEN** un destino que ya contiene un repositorio y un cuerpo con otra passphrase
- **WHEN** se da de alta
- **THEN** SHALL contestar con un error y la envoltura
- **AND** NO SHALL quedar fila en la base
- **AND** el destino NO SHALL quedar modificado

### Requirement: El disparo manual de una tarea encola y contesta 202, y 409 si ya está corriendo

`POST /api/v1/jobs/:id/run` SHALL contestar `202` con la ejecución creada en estado `pending`, SHALL contestar `404` si la tarea no existe, y SHALL contestar `409` si esa tarea ya tiene una ejecución en curso. La ruta SHALL colgar de la mitad protegida, como las demás de recursos.

El comportamiento vive en la capability `engine`, que es quien conoce la cola y el ciclo de vida; aquí se declara lo que la API promete, que es lo que el panel consume. Los tres códigos están elegidos en el requisito del motor y el porqué está escrito allí.

#### Scenario: El disparo contesta 202 con la ejecución
- **GIVEN** una tarea sin ejecución en curso
- **WHEN** se pide `POST /api/v1/jobs/:id/run`
- **THEN** SHALL contestar `202` con la envoltura
- **AND** `data` SHALL traer el `id` de la ejecución y su `job_id`

#### Scenario: Un identificador que no es un UUID contesta 400
- **GIVEN** una ruta cuyo identificador de tarea no es un UUID
- **WHEN** se pide el disparo
- **THEN** SHALL contestar `400` con el motivo

#### Scenario: Sin sesión no se dispara nada
- **GIVEN** el modo OIDC con la capa puesta y una petición sin sesión
- **WHEN** se pide el disparo
- **THEN** SHALL contestar `401` con la envoltura
- **AND** NO SHALL crearse ninguna ejecución

### Requirement: El stream de eventos es de la tarea que se pide, y el listado de snapshots es del repositorio que se pide

`GET /api/v1/events` SHALL contestar `200` con `text/event-stream` y NO SHALL exigir ningún parámetro. `GET /api/v1/snapshots` SHALL exigir el parámetro `repository_id`, SHALL contestar `400` si falta o no es un UUID, y SHALL contestar `404` si el repositorio no existe.

Las dos rutas leen de sitios distintos —el hub de eventos, que está en memoria, y el repositorio, que está en el almacenamiento—, pero las dos tienen la misma forma de fallar de la que ya se ocupó `api-resources`: un parámetro que falta o que no se puede interpretar es un `400` con el motivo, y un recurso que no existe es un `404`. Que el stream no lleve parámetros es deliberado y lo fija el §5: es un stream **global**, no uno por ejecución, así que no hay nada que filtrar aquí.

#### Scenario: El stream contesta su tipo de contenido
- **GIVEN** una sesión válida
- **WHEN** se pide `GET /api/v1/events`
- **THEN** SHALL contestar `200`
- **AND** el tipo de contenido SHALL ser `text/event-stream`

#### Scenario: El listado de snapshots exige su parámetro
- **GIVEN** una petición a `GET /api/v1/snapshots` sin `repository_id`
- **WHEN** se atiende
- **THEN** SHALL contestar `400` nombrando el parámetro

#### Scenario: Un repository_id que no es un UUID contesta 400
- **GIVEN** una petición con `repository_id=no-es-un-uuid`
- **WHEN** se atiende
- **THEN** SHALL contestar `400` con el motivo

#### Scenario: Un repositorio que no existe contesta 404
- **GIVEN** una petición con un `repository_id` bien formado que no corresponde a ninguna fila
- **WHEN** se atiende
- **THEN** SHALL contestar `404` con la envoltura

### Requirement: Las tres rutas nuevas cuelgan de la mitad protegida y ninguna escapa de la capa

`POST /api/v1/jobs/:id/run`, `GET /api/v1/events` y `GET /api/v1/snapshots` SHALL colgar de la mitad protegida del router, SHALL exigir sesión cuando el modo de autenticación la exige, y NO SHALL añadirse a la lista blanca de rutas públicas.

Las tres son superficie de negocio: el disparo escribe, y el stream y el explorador publican datos del despliegue —qué se está copiando y qué hay en el repositorio—. Dejarlas públicas por comodidad del panel sería publicar el estado del sistema a quien pregunte, y el stream lo publicaría además en tiempo real. La regla ya está declarada en el requisito de la partición; lo que hace falta aquí es que las tres rutas nuevas no se cuelen.

#### Scenario: Las tres rutas exigen sesión en modo OIDC
- **GIVEN** el modo OIDC con la capa puesta
- **WHEN** se piden las tres rutas sin sesión
- **THEN** las tres SHALL contestar `401` con la envoltura

#### Scenario: El stream no se sirve sin sesión
- **GIVEN** el modo OIDC con la capa puesta y una petición sin sesión a `GET /api/v1/events`
- **THEN** SHALL contestar `401` y NO SHALL quedar un stream abierto

#### Scenario: La lista blanca no ha crecido
- **GIVEN** la lista de rutas que no exigen sesión
- **WHEN** se revisa
- **THEN** NO SHALL contener ninguna de las tres rutas nuevas

### Requirement: El alta y la edición de una tarea rechazan una retención que no conservaría nada

`POST /api/v1/jobs` y `PUT /api/v1/jobs/:id` SHALL rechazar con `400` y con la envoltura una tarea cuyas **tres** reglas de retención sean cero, nombrando las tres columnas. Un cero suelto acompañado de otra regla informada NO SHALL rechazarse.

No es una validación de cortesía: con las tres a cero no hay ninguna regla que conserve nada, así que la retención no conserva de menos, **borra todo** el historial del repositorio en la siguiente copia —incluida la que se acaba de hacer—. El esquema del §3 pone un valor por defecto a esas tres columnas pero **no una cota**, así que el valor entra por la API y entra a mano; rechazarlo aquí es lo que evita que el error se cometa sin querer. El motor tiene además su propio guardarraíl, porque una fila escrita directamente en la base no pasa por aquí: las dos capas existen porque protegen de dos caminos distintos.

#### Scenario: Una tarea con las tres reglas a cero se rechaza al darla de alta
- **GIVEN** un cuerpo con las tres reglas de retención a cero
- **WHEN** se da de alta la tarea
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar las tres columnas
- **AND** NO SHALL quedar fila en la base

#### Scenario: Una tarea con las tres reglas a cero se rechaza al editarla
- **GIVEN** una tarea existente y un cuerpo que pone sus tres reglas a cero
- **WHEN** se edita
- **THEN** SHALL contestar `400` con la envoltura
- **AND** la fila SHALL quedar como estaba

#### Scenario: Un cero suelto se acepta
- **GIVEN** un cuerpo con una regla a cero y otra informada
- **WHEN** se da de alta la tarea
- **THEN** SHALL contestar `201` con la envoltura
- **AND** SHALL quedar la fila con esos valores
