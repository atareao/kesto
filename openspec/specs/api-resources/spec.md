# api-resources Specification

## Purpose
La superficie de recursos del §5: las rutas con las que el panel da de alta, consulta, edita y borra las fuentes de respaldo, los repositorios S3 y las tareas programadas, más el listado del histórico de ejecuciones con sus filtros, su paginación acotada y sus métricas, el disparo manual de una tarea, el stream de eventos en vivo y el explorador de snapshots. Es el consumidor de la capa de acceso del §3, así que recibe el pool abierto y migrado al arrancar —lo lleva al estado de la aplicación y no abre una segunda base— y sirve cada fila con tipos propios de la API en vez de las entidades de la base, con los identificadores como texto en su forma canónica con guiones y las fechas como cadenas RFC3339. Valida lo que la base no puede validar sola —la coherencia de los campos de un target con su tipo, la existencia de las referencias de una tarea antes de escribir y la sintaxis del `cron_expression` de una tarea— y devuelve, por cada credencial, un booleano que dice si está configurada en lugar del valor. Y el alta de un repositorio no es un apunte en la base: conecta o inicializa el repositorio de verdad.

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

`target_type = directory` SHALL exigir `host_path` y NO SHALL admitir `container_name`, `db_user`, `db_password`, `db_name` ni `db_path`. `target_type = mariadb` y `target_type = postgresql` SHALL exigir `container_name`, `db_user` y `db_password`, NO SHALL admitir `host_path` ni `db_path`, y SHALL admitir `db_name` como campo opcional, donde informado significa el volcado de **esa sola base** y ausente significa el volcado global —la semántica que fijó `db-schema` y que el motor implementará—. `target_type = sqlite` SHALL exigir `container_name` y `db_path`, y NO SHALL admitir `host_path`, `db_user`, `db_password` ni `db_name`.

El porqué: la tabla del §3 **acepta** las combinaciones incoherentes. Sus columnas son anulables, y su único `CHECK` restringe `target_type`, así que un `directory` con `container_name` y sin `host_path` es una fila que la base guarda tan contenta y que el motor no sabrá usar; el fallo aparecería en la primera copia, como un error que habla de un contenedor que no existe y no de un formulario mal rellenado. La coherencia se exige, por tanto, en el borde HTTP, que es el único sitio donde se conoce la intención de quien escribe.

El alta de una tarea SHALL comprobar que `target_id` y `repository_id` **existen** antes de escribir, y SHALL contestar `400` nombrando el campo que no resuelve, para que el cliente reciba un motivo preciso —«el target indicado no existe»— en vez del error de una restricción, que no distingue si lo que falló fue el target o el repositorio. Esa comprobación NO SHALL sustituir a la clave foránea del §3, que sigue siendo la última línea: entre la comprobación y el `INSERT` cabe otra petición que borre la fila referenciada, y ese hueco lo cubre la base, no el manejador.

Los campos **desconocidos** del cuerpo SHALL rechazarse con `400`. Sin esta regla, una errata en el nombre de un campo —`endpont`— se interpreta como un campo que nadie informa y el valor se pierde en silencio: la petición contesta `201` y el recurso queda escrito sin el dato que quien llamó creía haber mandado. Rechazarlo convierte ese silencio en un error que nombra el campo de más.

#### Scenario: Un directory con container_name se rechaza
- **GIVEN** un alta de target con `target_type` a `directory`, su `host_path` y un `container_name`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `container_name`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un sqlite con sus campos es válido
- **GIVEN** un alta de target con `target_type = sqlite`, `container_name` y `db_path`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `201`
- **AND** la fila escrita SHALL tener el `db_path` indicado

#### Scenario: Un sqlite sin db_path se rechaza nombrando el campo
- **GIVEN** un alta de target con `target_type = sqlite` y `container_name`, sin `db_path`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400`
- **AND** el mensaje SHALL nombrar el campo `db_path`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un sqlite con db_user se rechaza
- **GIVEN** un alta de target con `target_type = sqlite`, sus `container_name` y `db_path`, y un `db_user`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `db_user`
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

`GET /api/v1/executions` SHALL admitir los filtros `id`, `job_id` y `status`, y la paginación `limit` y `offset`. La respuesta SHALL ser una **envoltura** con `executions` —el array de filas— y `total` —el número de filas que casan con los filtros—, como ya hace el listado de restauraciones.

El filtro `id` SHALL ser opcional y SHALL devolver, como mucho, la fila con ese identificador. Existe para que el servidor MCP pueda resolver una ejecución concreta —la herramienta `get_execution`— sin estrenar una ruta de detalle: el recurso sigue siendo de solo lectura y su superficie no crece. Un `id` con forma inválida SHALL contestar `400`; un `id` válido sin fila SHALL devolver el listado vacío, como cualquier otro filtro que no case.

`limit` SHALL tener `50` como valor por defecto y **`200` de tope**, y un valor fuera de rango —por encima del tope o negativo— o no numérico SHALL contestar `400`. El tope existe porque el histórico es la tabla que más crece de las cuatro —una fila por intento de copia— y el panel pinta una página: sin tope, un `limit` grande materializaría la tabla entera en memoria y ocuparía una de las cinco conexiones del pool mientras el motor quiere escribir. Un valor fuera de rango es una petición mal formada, así que se contesta `400` y no se recorta en silencio.

`total` SHALL contar **todas** las filas que satisfacen los filtros de la petición, no solo las de la página devuelta, y NO SHALL verse afectado por `limit` ni por `offset`. Es lo que permite al panel pintar el número total de elementos y el número de páginas sin una segunda llamada ni una heurística.

El orden SHALL ser `started_at DESC, id`, y el `id` desempata porque las ejecuciones del **mismo segundo** empatan —un lote de tareas disparado por el mismo tick de cron arranca a la vez, y `CURRENT_TIMESTAMP` no tiene más resolución— y el orden tiene que ser **estable** entre dos llamadas: sin desempate, dos peticiones seguidas con el mismo `offset` pueden devolver una fila repetida y saltarse otra, y el panel paginaría de forma visiblemente rota.

Cada fila SHALL llevar sus **métricas** —los dos contadores de bytes y sus dos fechas—, que es el sentido del «y métricas» del §5: **no** hay endpoint de agregados, quien sume los totales es el panel, y para eso necesita los números de cada fila y no solo un resumen que la API no puede acotar por periodo.

Las fechas de cada fila SHALL publicarse como cadena RFC3339, como en el resto de la superficie.

Cada fila SHALL llevar además **`job_name`** (anulable): el nombre de la tarea de esa ejecución, resuelto por el vínculo con `backup_jobs`. SHALL publicarse como `null` cuando la tarea no tenga nombre o la ejecución no case con ninguna tarea, con la misma tolerancia que el resto del listado: un `job_name` a `null` es un nombre desconocido, no un error, y NO SHALL acortar el listado ni ocultar la fila.

#### Scenario: El listado sin filtros devuelve las ejecuciones con sus métricas
- **GIVEN** tres ejecuciones dadas de alta
- **WHEN** se pide el listado sin filtros
- **THEN** SHALL contestar `200` con la envoltura
- **AND** `executions` SHALL traer las tres filas y cada una SHALL traer `bytes_processed`, `bytes_uploaded`, `started_at`, `finished_at` y `job_name`
- **AND** `total` SHALL valer `3`

#### Scenario: Los filtros por job_id y por status acotan el listado
- **GIVEN** ejecuciones de dos tareas distintas, con estados distintos
- **WHEN** se pide el listado con `job_id` y con `status`
- **THEN** cada llamada SHALL devolver solo las filas que cumplen su filtro
- **AND** los dos filtros SHALL poder usarse a la vez

#### Scenario: El total refleja el acotado por los filtros
- **GIVEN** ejecuciones de dos tareas con estados distintos
- **WHEN** se pide el listado con `status=failed`
- **THEN** `total` SHALL ser el número de ejecuciones fallidas, no el de todas

#### Scenario: El total no lo cambian limit ni offset
- **GIVEN** cinco ejecuciones
- **WHEN** se piden dos páginas con `limit=2` y distinto `offset`
- **THEN** las dos respuestas SHALL traer `total` igual a `5`
- **AND** cada una SHALL traer dos filas

#### Scenario: El filtro por id devuelve una sola ejecución
- **GIVEN** ejecuciones de dos tareas distintas
- **WHEN** se pide el listado con el `id` de una de ellas
- **THEN** SHALL devolver solo esa fila
- **AND** SHALL poder combinarse con `job_id` y `status`

#### Scenario: Un id válido sin fila devuelve el listado vacío
- **GIVEN** el router con una sesión válida y un identificador que no existe
- **WHEN** se pide el listado con `id`
- **THEN** SHALL contestar `200` con `executions` vacío
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

### Requirement: El alta y la edición de una tarea validan su expresión cron

`POST /api/v1/jobs` y `PUT /api/v1/jobs/:id` SHALL validar el `cron_expression` informado con la **misma** validación que usa el planificador —un analizador, una gramática, un solo sitio—, SHALL contestar `400` con la envoltura y el motivo cuando la expresión no parsea, y NO SHALL escribir la fila en ese caso: el alta no deja ninguna fila nueva y la edición deja la fila **como estaba**. Una expresión válida de **cinco** campos SHALL aceptarse, y también una de **seis**.

La validación vive en la capability `scheduler`, que es quien conoce la gramática y quien la va a evaluar; aquí se declara lo que la API promete, que es lo que el panel consume. Y el reparto no es una preferencia de arquitectura: si el borde HTTP tuviera su propia idea de qué expresión es válida, lo que la API aceptara y lo que el reloj entendiera podrían discrepar, y discreparían en los bordes —los segundos, los días de la semana—, que es donde aparecen las expresiones raras. Una sola función, invocada desde los dos sitios, hace imposible esa discrepancia.

Las dos rutas y no solo el alta, porque la edición es cómo se arregla una expresión mala y también cómo se podría empeorar sin límite. Y **antes de escribir**, porque una fila inválida que se escribe y luego se rechaza es peor que un rechazo: la fila existe, el panel la lista como programada y la reconciliación del planificador la saltará en cada pasada, llenando el registro de avisos por un error que ya se había detectado en la petición.

Esta validación **no sustituye** a la robustez del planificador: la validación protege de lo que entra por la API y el «se registra, se salta y las demás siguen» del planificador protege de lo que entra por debajo —una fila escrita con `sqlite3`, una base restaurada de una versión anterior—. Las dos capas existen porque protegen de dos caminos distintos, como ya ocurre con la validación de la retención en el borde y el guardarraíl del motor.

El motivo SHALL nombrar el **campo** —`cron_expression`—, como el resto de los `400` de la superficie. La regla de no repetir el valor existe por las credenciales, y aquí el campo es lo que hace falta: quien envió la expresión la tiene delante.

#### Scenario: El alta con una expresión de cinco campos se escribe
- **GIVEN** un alta de tarea con `cron_expression` a `0 3 * * *` y el resto de campos válidos
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `201` con la envoltura
- **AND** la fila SHALL quedar escrita con esa expresión

#### Scenario: El alta con una expresión inválida contesta 400 y no deja fila
- **GIVEN** un alta de tarea con una expresión que el analizador no entiende
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL decir por qué la expresión no vale
- **AND** la tabla de tareas NO SHALL tener ninguna fila nueva

#### Scenario: La edición con una expresión inválida deja la fila como estaba
- **GIVEN** una tarea existente con su expresión válida
- **WHEN** se edita con una expresión que el analizador no entiende
- **THEN** SHALL contestar `400` con la envoltura
- **AND** la fila SHALL quedar con la expresión que tenía

#### Scenario: El motivo nombra el campo
- **GIVEN** un alta de tarea con una expresión inválida
- **WHEN** se lee el cuerpo del `400`
- **THEN** SHALL nombrar el campo `cron_expression`

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

### Requirement: El histórico de restauraciones se lista con filtros y paginación

SHALL existir `GET /api/v1/restorations` que devuelva las ejecuciones de restauración persistidas, ordenadas por fecha de comienzo descendente, con paginación acotada y los filtros `status` y `target_id`, y SHALL seguir la envoltura de `api-shell`. Cada fila SHALL publicar `id`, `snapshot_id`, `target_id`, `target_name`, `target_type`, `status`, `error_message`, `started_at` y `finished_at`, con los identificadores como texto en su forma canónica con guiones y las fechas como cadenas RFC3339. `target_name` y `target_type` SHALL resolverse desde el target referenciado por `target_id`, y SHALL publicarse como `null` solo si `target_id` es `null`.

El **total** de la respuesta SHALL salir de la **misma transacción** que la página: el `COUNT` y la consulta de la página SHALL ejecutarse dentro de un mismo `begin()`/`commit()`, como ya hace el listado de ejecuciones. Sin esa transacción, entre las dos consultas podría entrar una escritura del motor y el `total` diría un número que la página no sostiene. Las dos son lecturas, así que la transacción NO SHALL bloquear a nadie mientras dura.

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

#### Scenario: El total y la página salen de la misma transacción

- **GIVEN** el histórico de restauraciones de una entidad de acceso
- **WHEN** se pide el listado con ventana
- **THEN** el `COUNT` y la consulta de la página SHALL ir dentro de un mismo `begin()`/`commit()`
- **AND** el `total` NO SHALL poder contar un conjunto distinto del que acota la página

### Requirement: El nombre de la tarea es obligatorio y único

`POST /api/v1/jobs` y `PUT /api/v1/jobs/:id` SHALL **exigir** `name`, y la misma regla SHALL valer para las dos rutas —son una sola puerta de escritura, `alta_desde`—. Un cuerpo que no traiga el campo, que lo traiga a `null`, o que lo traiga vacío o en blanco **tras recortar**, SHALL contestar `400` con la envoltura y un mensaje que **nombre `name`** y diga que **falta el nombre**; si supera **120** caracteres SHALL contestar `400` con la envoltura, nombrando `name` y **sin repetir el valor**. El nombre SHALL guardarse **recortado**: los espacios de los extremos no son parte del nombre, y el tope se mide sobre lo recortado.

En el **cable**, y por eso, `name` SHALL seguir siendo `Option<String>` en el struct de entrada —con el `deny_unknown_fields` de siempre—, y el `400` SHALL nacer en el **manejador**, no en el extractor: el extractor de JSON rechaza por su cuenta un cuerpo que no case con el tipo, con un error ajeno a la envoltura del §5 y sin nombrar `name` —y declarar ahí un `String` no obligatorio convertiría la falta del campo en ese rechazo genérico en vez del `400` que nombra el campo—. Con `Option<String>`, `None` y el blanco llegan al manejador, que es quien los traduce a un `400` de la envoltura nombrando `name`; y ese origen es lo que lo hace **distinguible** del `409` del duplicado, que nace al violar la unicidad de la base. Las tres reglas del párrafo anterior —falta, blanco y tope— SHALL salir del mismo sitio que el resto de las validaciones del cuerpo.

Un `name` que ya tenga **otra** tarea SHALL contestar **`409`** con la envoltura y un mensaje que **nombre `name`**, diga que ese nombre ya está en uso y **no repita el valor**. El mensaje del `409` SHALL ser **distinto** del de la falta de nombre: los dos rechazan la misma entrada, pero mandan a hacer cosas distintas —uno a escribir un nombre, el otro a cambiarlo—, y un panel que no los distinga obliga al operador a adivinar cuál de las dos cosas arreglar. Editar una tarea **conservando su propio nombre** NO SHALL considerarse un duplicado: una tarea no choca consigo misma.

La unicidad SHALL estar **en la base** —`UNIQUE(name)`, tal como lo declara `db`—, y el `409` SHALL derivarse de la violación de esa restricción, NO de una lectura previa: comprobar y luego escribir deja la ventana abierta a que dos altas simultáneas pasen las dos la comprobación y una escriba de todos modos. La comparación SHALL ser la de la base, sensible a mayúsculas, de modo que el `409` no prometa más ni menos de lo que la restricción garantiza.

`GET /api/v1/jobs` y `GET /api/v1/jobs/:id` SHALL publicar `name` como **cadena no vacía**, nunca `null`: la columna es `NOT NULL` y las tareas anteriores a la migración llevan el nombre derivado que ella escribió. El nombre NO SHALL omitirse de la vista.

El porqué: el nombre es la **identidad legible** de la tarea —lo que distingue dos tareas sobre el mismo target, donde antes el Histórico usaba el nombre del target como proxy y las confundía—. Anulable, cada pantalla tiene que inventarse un sustituto cuando no lo hay, y esos sustitutos se contradicen entre sí: una deja el hueco en blanco y la otra cae al identificador, que es el de la etiqueta del snapshot. Obligatorio y único, la misma cadena vale en Tareas, en el Histórico y en Snapshots, y el panel no tiene que inventar nada. Que la unicidad viva en la base es lo que la convierte en una garantía y no en una convención; y el `409` es el código que describe el choque, porque la petición **está bien** y contradice lo que ya hay —ni `400`, que diría que el cuerpo está mal escrito, ni `422`, que diría que la operación no se sabe hacer—.

#### Scenario: Un alta sin name contesta 400 nombrando name

- **GIVEN** un cuerpo de alta con target, repositorio, cron y retención válidos, y sin el campo `name`
- **WHEN** se envía
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el mensaje SHALL nombrar el campo `name`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un nombre en blanco contesta 400 nombrando name

- **GIVEN** un alta o una edición de tarea con `name` a una cadena de solo espacios
- **WHEN** se envía
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el mensaje SHALL nombrar el campo `name`
- **AND** NO SHALL guardarse la fila con el nombre en blanco

#### Scenario: El alta guarda el nombre recortado

- **GIVEN** un alta de tarea con `name` con espacios a ambos lados de `copia nocturna`
- **WHEN** se guarda y se lee la tarea
- **THEN** su `name` SHALL ser `copia nocturna`, sin los espacios de los extremos
- **AND** SHALL conservarse el resto de campos de la tarea

#### Scenario: Un nombre de más de 120 caracteres contesta 400 nombrando name

- **GIVEN** un alta o una edición de tarea con un `name` de 121 caracteres
- **WHEN** se envía
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el mensaje SHALL nombrar el campo `name` y NO SHALL repetir el valor

#### Scenario: Un nombre duplicado en el alta contesta 409

- **GIVEN** una tarea ya dada de alta con el nombre `copia nocturna`
- **WHEN** se da de alta otra tarea con ese mismo `name`
- **THEN** SHALL contestar `409` con la envoltura
- **AND** el mensaje SHALL nombrar el campo `name` y decir que ya está en uso
- **AND** NO SHALL repetir el valor
- **AND** NO SHALL escribirse la segunda fila

#### Scenario: Un nombre duplicado en la edición contesta 409

- **GIVEN** dos tareas, `copia nocturna` y `copia diaria`
- **WHEN** se edita la segunda con el nombre de la primera
- **THEN** SHALL contestar `409` con la envoltura
- **AND** la segunda tarea SHALL conservar su nombre anterior

#### Scenario: Editar una tarea conservando su propio nombre no choca

- **GIVEN** una tarea con nombre `copia nocturna`
- **WHEN** se edita —por ejemplo, para cambiar su cron— enviando ese mismo `name`
- **THEN** SHALL contestar `200`
- **AND** su `name` SHALL seguir siendo `copia nocturna`

#### Scenario: Los dos rechazos del nombre se distinguen entre sí

- **GIVEN** un alta sin `name` y un alta con un `name` ya en uso
- **WHEN** se envían las dos
- **THEN** la primera SHALL contestar `400` y la segunda `409`
- **AND** los dos mensajes SHALL ser distintos
- **AND** SHALL poder decidirse con ellos si hay que escribir un nombre o cambiarlo

#### Scenario: La unicidad la garantiza la base

- **GIVEN** una base con las migraciones aplicadas y una tarea con nombre
- **WHEN** se inserta con SQL directo otra fila con ese mismo `name`
- **THEN** la inserción SHALL fallar
- **AND** el `409` del borde HTTP NO SHALL depender de una comprobación previa en Rust

#### Scenario: El listado de tareas publica el nombre de cada tarea

- **GIVEN** tres tareas dadas de alta —las anteriores a la migración incluidas—
- **WHEN** se pide `GET /api/v1/jobs`
- **THEN** cada fila SHALL traer su `name`
- **AND** ninguna SHALL traerlo a `null` ni omitirlo

### Requirement: El estado de salud de un repositorio se comprueba bajo demanda y se sirve en el recurso

La API SHALL ofrecer una comprobación de salud por repositorio y una que comprueba todos: `POST /api/v1/repositories/{id}/health` y `POST /api/v1/repositories/health`. Cada una SHALL ejecutar la comprobación de la capacidad `repository`, **anotar el desenlace en la fila** y devolverlo en la envoltura del §5. La comprobación de todos SHALL recorrer los repositorios existentes y devolver un elemento por cada uno.

El recurso de repositorio que sirve `GET /api/v1/repositories` y `GET /api/v1/repositories/{id}` SHALL incluir el **estado de salud**, la **fecha** de la última comprobación y un **motivo** acotado, derivados de la fila. El estado NO SHALL ser un campo editable: NO SHALL admitirse en el alta ni en la actualización. Un repositorio nunca comprobado SHALL servirse con el estado «desconocido» y la fecha a nulo.

El motivo SHALL estar **saneado**: NO SHALL contener credenciales ni el motivo crudo de la dependencia, solo un texto legible para el operador.

#### Scenario: Comprobar un repositorio persiste y devuelve su estado
- **GIVEN** un repositorio dado de alta
- **WHEN** se pide `POST /api/v1/repositories/{id}/health`
- **THEN** la respuesta SHALL traer el estado resultante en la envoltura del §5
- **AND** el estado SHALL quedar persistido y visible en la lectura posterior del recurso

#### Scenario: El listado sirve el estado sin comprobar nada
- **GIVEN** varios repositorios con estados de salud anotados
- **WHEN** se pide `GET /api/v1/repositories`
- **THEN** cada elemento SHALL traer su estado y su fecha
- **AND** NO SHALL ejecutarse ninguna comprobación contra el destino

#### Scenario: Comprobar todos devuelve un elemento por repositorio
- **GIVEN** tres repositorios dados de alta
- **WHEN** se pide `POST /api/v1/repositories/health`
- **THEN** la respuesta SHALL traer tres elementos
- **AND** cada estado SHALL quedar persistido en su fila

#### Scenario: La salud no se puede escribir por el alta ni la actualización
- **GIVEN** un cuerpo de alta o de actualización que incluye un campo de salud
- **WHEN** se envía
- **THEN** el campo SHALL ignorarse
- **AND** la salud de la fila NO SHALL venir de ese cuerpo

#### Scenario: Un repositorio nunca comprobado se sirve como desconocido
- **GIVEN** un repositorio recién dado de alta
- **WHEN** se pide su lectura
- **THEN** su salud SHALL ser «desconocido»
- **AND** su fecha de comprobación SHALL ser nula

### Requirement: Las rutas del target se aceptan solo dentro de su alcance

El alta y la edición de un target SHALL comprobar el **alcance** de sus rutas, además de su presencia y su coherencia con el tipo. Cuando la comprobación falle, la petición SHALL contestar `400` nombrando el campo y NO SHALL escribirse ninguna fila.

- `host_path`, de un target `directory`, SHALL ser una ruta **absoluta**, **sin `..`** y SHALL quedar **bajo** la raíz de `KESTO_BACKUP_ROOT`, comparando **por componentes** y no por prefijo de cadena: `/backup_sources_x` empieza por `/backup_sources` como texto y **no** está dentro de él.
- `db_path`, de un target `sqlite`, SHALL ser una ruta **absoluta**, sin `..` y sin segmento vacío.

La comprobación del borde SHALL ser **textual** y NO SHALL exigir que la ruta exista: al dar de alta un directorio puede no estar montado todavía, y exigir su existencia convertiría el alta en una comprobación del despliegue. La canonicalización —que resuelve los enlaces simbólicos y es la que de verdad cierra el agujero— la hace el motor, no este borde.

La comprobación SHALL aplicarse por igual al alta y a la edición, porque las dos comparten el mismo punto de validación.

#### Scenario: Un host_path fuera de la raíz se rechaza

- **GIVEN** un alta de target `directory` cuyo `host_path` es `/` y una raíz de copias `/backup_sources`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `host_path`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Una raíz que solo coincide como prefijo no vale

- **GIVEN** un alta de target `directory` cuyo `host_path` es `/backup_sources_x` y una raíz de copias `/backup_sources`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `host_path`

#### Scenario: Un host_path con `..` se rechaza

- **GIVEN** un alta de target `directory` cuyo `host_path` es `/backup_sources/../etc` y una raíz de copias `/backup_sources`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `host_path`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un host_path relativo se rechaza

- **GIVEN** un alta de target `directory` cuyo `host_path` es `backup_sources/var_www`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `host_path`

#### Scenario: Un host_path bajo la raíz se acepta aunque no exista todavía

- **GIVEN** un alta de target `directory` cuyo `host_path` es `/backup_sources/var_www` y ese directorio no existe en el sistema de ficheros
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `201`
- **AND** la fila escrita SHALL tener ese `host_path`

#### Scenario: Un db_path con `..` se rechaza

- **GIVEN** un alta de target `sqlite` cuyo `db_path` es `/datos/../etc/base.db`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `db_path`

#### Scenario: Un db_path relativo se rechaza

- **GIVEN** un alta de target `sqlite` cuyo `db_path` es `datos/base.db`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `db_path`

#### Scenario: La edición aplica la misma regla

- **GIVEN** un target `directory` existente y un `PUT` que le pone un `host_path` fuera de la raíz
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `host_path`
- **AND** la fila NO SHALL cambiar

### Requirement: El alta y la edición de un repositorio validan el `endpoint`

`POST /api/v1/repositories` y `PUT /api/v1/repositories/{id}` SHALL validar el campo `endpoint` **antes** de escribir la fila y antes de hablar con el destino. Cuando la validación falle, la petición SHALL contestar `400` nombrando el campo `endpoint` y NO SHALL escribirse ni modificarse ninguna fila.

El `endpoint` SHALL ser una URL absoluta con esquema y host. SHALL aceptarse el esquema `https`; el esquema `http` SHALL rechazarse salvo que la configuración conceda el permiso explícito —ver la capability `config`—.

Sobre el host, y siempre que sea una dirección literal:

- SHALL rechazarse **siempre**, con permiso o sin él, la dirección de enlace local (`169.254.0.0/16`, `fe80::/10`) —que incluye la de metadatos de la nube, `169.254.169.254`—, la de metadatos IPv6 de AWS (`fd00:ec2::254`) y la no especificada (`0.0.0.0`, `::`);
- SHALL rechazarse **salvo permiso explícito** el loopback (`127.0.0.0/8`, `::1`) y un nombre `localhost` o terminado en `.localhost`;
- SHALL aceptarse el rango privado (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`) y las direcciones públicas.

Una dirección IPv6 que lleve **embebida** una IPv4 —la forma IPv4-mapeada `::ffff:a.b.c.d` (RFC 4291) y el prefijo NAT64 bien conocido `64:ff9b::/96` (RFC 6052)— SHALL juzgarse por esa IPv4, porque es la dirección que de verdad se alcanza: `https://[::ffff:127.0.0.1]` es loopback y `https://[::ffff:169.254.169.254]` es la dirección de metadatos.

Un `endpoint` que lleve credenciales embebidas (`https://usuario:clave@host`) SHALL rechazarse siempre: acabarían escritas en la fila y en los registros.

El mensaje del `400` SHALL nombrar el campo y NO SHALL repetir el valor recibido.

El porqué del rango privado permitido: el destino natural de Kesto es un almacén S3 autoalojado —MinIO, SeaweedFS— en la red interna, y rechazarlo dejaría el producto inservible para su uso más común. Lo que se cierra es el **loopback** del propio proceso y la **dirección de metadatos**, donde viven las credenciales de instancia de la máquina.

El porqué del borde y no del motor: la edición NO SHALL conectar con el destino, así que una comprobación dentro del motor no la alcanzaría y una fila podría quedar guardada con un `endpoint` inadmisible. El alta SHALL conservar su comprobación de conectividad: la validación del `endpoint` es una puerta previa, no un sustituto, y NO SHALL uniformarse la respuesta del alta según el destino responda o no.

La validación NO SHALL resolver el nombre del host: un `endpoint` con nombre se acepta por su forma, así que un nombre que resuelva por DNS a una dirección rechazada NO SHALL detectarse en esta versión.

#### Scenario: Un endpoint por `http` se rechaza sin permiso
- **GIVEN** un alta cuyo `endpoint` es `http://almacen.example.test:9000` y sin permiso declarado
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un endpoint a loopback se rechaza sin permiso
- **GIVEN** un alta cuyo `endpoint` es `http://127.0.0.1:9000` y sin permiso declarado
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`

#### Scenario: La dirección de metadatos se rechaza incluso con permiso
- **GIVEN** un alta cuyo `endpoint` es `https://169.254.169.254` y el permiso declarado
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`

#### Scenario: Una IPv4 embebida en una IPv6 se juzga por la IPv4
- **GIVEN** un alta cuyo `endpoint` es `https://[::ffff:127.0.0.1]:9000` y sin permiso declarado
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`
- **AND** lo mismo SHALL cumplirse para la forma NAT64 `https://[64:ff9b::7f00:1]:9000`
- **AND** con el permiso declarado, `https://[::ffff:127.0.0.1]:9000` SHALL admitirse como se admite `127.0.0.1`

#### Scenario: Con permiso, un endpoint local y en claro se acepta
- **GIVEN** un alta cuyo `endpoint` es `http://127.0.0.1:9000` y el permiso declarado
- **WHEN** se atiende la petición
- **THEN** la petición SHALL pasar la validación del `endpoint`
- **AND** su desenlace SHALL depender de la conectividad, como en cualquier alta

#### Scenario: El rango privado se acepta por `https`
- **GIVEN** un alta cuyo `endpoint` es `https://192.168.1.10:9000`
- **WHEN** se atiende la petición
- **THEN** la petición SHALL pasar la validación del `endpoint`

#### Scenario: Un esquema que no es `http` ni `https` no vale
- **GIVEN** un alta cuyo `endpoint` es `ftp://almacen.example.test`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`

#### Scenario: Un valor que no es una URL absoluta no vale
- **GIVEN** un alta cuyo `endpoint` es `almacen.example.test:9000`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`

#### Scenario: Un endpoint con credenciales embebidas no vale
- **GIVEN** un alta cuyo `endpoint` es `https://usuario:clave@almacen.example.test`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`
- **AND** el cuerpo NO SHALL contener la credencial

#### Scenario: La edición aplica la misma regla
- **GIVEN** un repositorio existente y un `PUT` que le pone un `endpoint` por `http` sin permiso declarado
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`
- **AND** la fila NO SHALL cambiar

#### Scenario: El alta conserva su comprobación de conectividad
- **GIVEN** un alta con un `endpoint` válido contra un destino que no responde
- **WHEN** se atiende la petición
- **THEN** SHALL contestar el error de destino inalcanzable con la envoltura
- **AND** NO SHALL quedar fila en la base

### Requirement: Los campos de texto se recortan y tienen una cota por campo

Las rutas de alta y edición de `/repositories` y `/targets` SHALL **recortar** los espacios de los extremos de cada campo de texto antes de guardarlo y de medirlo, y SHALL rechazar con `400` y la envoltura, **nombrando el campo** y **sin repetir el valor**, un campo que tras recortar supere su cota.

Las cotas SHALL ser: `name` 120 caracteres; `container_name` 128; `db_user`, `db_name` y `bucket` 63; `region` 64; `access_key_id` 128; `endpoint` 2048; `db_path` 1024; `host_path` 4096.

Un campo **obligatorio** que tras recortar quede vacío SHALL contestar `400` nombrando el campo —es la misma regla que la falta del campo—. Un campo **opcional** (por ejemplo `db_name`) que se informe vacío o de solo espacios SHALL guardarse como `NULL` en su columna, que es lo que el §3 significa para «ausente»; NO SHALL guardarse la cadena vacía.

Las cotas SHALL medirse en **caracteres** y no en bytes: contar bytes rechazaría un nombre con acentos que el formulario deja escribir. Los campos de **secreto** (`secret_access_key`, `passphrase`, `db_password`) NO SHALL tener cota de campo.

El porqué: la tabla del §3 no acota sus columnas `TEXT`, así que sin esta regla un `bucket` de miles de caracteres o un `name` de solo espacios entran en la base y reaparecen más tarde —en un listado, en una etiqueta de snapshot o en una orden del motor—. Y el recorte es lo que hace que el tope se mida sobre lo que de verdad se guarda: los espacios de los extremos no son parte del valor.

#### Scenario: Un name de solo espacios se rechaza
- **GIVEN** un alta de target con `name` a una cadena de solo espacios
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar el campo `name`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un name por encima de la cota se rechaza sin repetir el valor
- **GIVEN** un alta de repositorio con un `name` de 121 caracteres distinguible
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400`
- **AND** el motivo SHALL nombrar el campo `name`
- **AND** el cuerpo NO SHALL contener el valor recibido

#### Scenario: Un campo se guarda recortado
- **GIVEN** un alta válida con `container_name` con espacios a ambos lados
- **WHEN** se guarda y se lee la fila
- **THEN** el valor almacenado SHALL ser el recortado, sin los espacios de los extremos
- **AND** SHALL conservarse el resto de campos

#### Scenario: Un db_name en blanco se guarda como NULL
- **GIVEN** un alta de target de base de datos con `db_name` a una cadena de solo espacios
- **WHEN** se escribe la fila y se mira la columna `db_name` con SQL directo
- **THEN** el valor almacenado SHALL ser `NULL`
- **AND** NO SHALL ser la cadena vacía ni la cadena en blanco

#### Scenario: Un bucket por encima de su cota se rechaza
- **GIVEN** un alta de repositorio con un `bucket` de 64 caracteres
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar el campo `bucket`

### Requirement: El nombre de un contenedor se valida contra el juego de caracteres de Docker

`container_name`, cuando se informe en un alta o una edición de target, SHALL cumplir el juego de caracteres con el que Docker nombra y localiza contenedores —una primera letra alfanumérica seguida de letras, dígitos, `_`, `.` o `-` (`[A-Za-z0-9][A-Za-z0-9_.-]*`)—. Un valor que no lo cumpla SHALL contestar `400` con la envoltura, **nombrando `container_name`** y **sin repetir el valor**.

El porqué: el cliente de la API de Docker (bollard) interpola el nombre **sin codificar** en la ruta de la petición (`/containers/{container_name}/exec`). Un nombre con `/`, `..` o `?` no da un error de nombre: mueve la petición a otro punto de la API de Docker. El juego de caracteres de Docker no admite ninguno de esos caracteres, así que exigirlo cierra la inyección de ruta y, de paso, hace que el nombre que se escribe sea uno que un contenedor puede tener de verdad. Cubre también los identificadores —largos o cortos—, que son hexadecimales.

#### Scenario: Un container_name con una barra se rechaza
- **GIVEN** un alta de target `sqlite` con `container_name` a `../../v1.40/containers/x` y un `db_path` válido
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar el campo `container_name`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un container_name válido se acepta
- **GIVEN** un alta de target `mariadb` con `container_name` a `contenedor_de-la.base-01` y el resto de campos válidos
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `201` con la envoltura

### Requirement: El alta y la edición de una tarea rechazan una retención negativa que no sea -1

`POST /api/v1/jobs` y `PUT /api/v1/jobs/:id` SHALL aceptar, en cada una de las tres reglas `keep_daily`, `keep_weekly` y `keep_monthly`, un valor `>= 0` o el valor `-1` —que es la forma que tiene la biblioteca de retención de decir «conservar todas»—. SHALL rechazar con `400` y con la envoltura, nombrando la columna, cualquier valor menor que `-1` o mayor que el entero de 32 bits que la biblioteca usa, sin escribir la fila.

El porqué: el motor (`src/rustic/retencion.rs`) lee `-1` como «conservar todo» y rechaza cualquier otro negativo; el borde tiene que rechazar **lo mismo**, para que un valor que el motor no sabría leer se pare en el formulario en vez de aparecer como un fallo de la primera copia. La regla no sustituye al guardarraíl del motor —una fila escrita por debajo de la API no pasa por aquí—, y las dos capas existen porque protegen de dos caminos distintos. El `-1` se conserva porque hoy es la forma legítima de pedir «conservar todo» y rechazarlo rompería configuraciones válidas.

#### Scenario: Una regla negativa distinta de -1 se rechaza
- **GIVEN** un alta de tarea con `keep_daily` a `-5` y el resto de campos válidos
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar la columna `keep_daily`
- **AND** NO SHALL quedar fila en la base

#### Scenario: El -1 se acepta como «conservar todo»
- **GIVEN** un alta de tarea con `keep_daily` a `-1` y las otras dos reglas informadas
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `201` con la envoltura
- **AND** la fila SHALL quedar escrita con `keep_daily` a `-1`

#### Scenario: Una regla por encima del entero de la biblioteca se rechaza
- **GIVEN** un alta de tarea con `keep_weekly` a un valor mayor que el mayor entero de 32 bits
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar la columna `keep_weekly`
