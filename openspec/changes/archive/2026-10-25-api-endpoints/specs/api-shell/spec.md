# api-shell Specification (delta)

## ADDED Requirements

### Requirement: Un error de la base causado por la petición contesta 4xx, y el resto sigue contestando 5xx

Un fallo de la base cuyo origen es **la petición** SHALL contestar `4xx` con la envoltura, y NO SHALL contestar `500`. En concreto: un secreto **vacío** SHALL contestar `400` **nombrando el campo**, y una **clave foránea violada** SHALL contestar `409 Conflict`.

El porqué de cada código:

- El `400` del secreto vacío: la petición se puede atender, y lo que trae mal es un campo. La causa ya existe y es tipada —`DbError::SecretoVacio { campo }`—, y su mensaje nombra el campo y nunca el valor, así que se puede publicar tal cual.
- El `409`: la petición está **bien formada** y lo que ocurre es que contradice una relación que existe —la tarea apunta a un target o a un repositorio que ya no está—, así que un `400` diría que está mal escrita, y no lo está. `409` es el código que dice «tu petición es válida y el estado del servidor no la admite ahora mismo».

**Todo lo demás** SHALL seguir contestando `500` con el mensaje genérico que ya fija el requisito «Un error interno no cuenta por qué falló» de esta capability, y con la causa en el registro: un `DbError::Consulta` con un `no such table`, un fichero ilegible o una migración que no se aplicó no son culpa de quien pregunta. Este requisito no relaja aquel: le escribe la **frontera**, que hasta ahora no estaba dicha porque ningún manejador tocaba la base.

El mensaje del `4xx` SHALL llevar el motivo. El del `409` SHALL decir que la operación **contradice una referencia existente**, y NO SHALL llevar el texto de la sentencia, ni el nombre de la restricción, ni el de una columna o una tabla. Y NO SHALL prometer más de lo que se sabe: `backup_jobs` tiene **dos** claves foráneas —`target_id` y `repository_id`— y SQLite contesta `FOREIGN KEY constraint failed` sin nombrar cuál falló —el `constraint()` de `sqlx` no la trae para este caso—, así que un mensaje que dijera «el target indicado no existe» estaría **adivinando entre dos** y mandaría a corregir un campo que puede estar bien. Lo que hay que poder hacer con un `409` no es reparar la petición a ciegas, sino **enterarse de que ocurrió** —y para eso está el registro, que recibe la causa entera—: recuerda que el caso solo se alcanza por una carrera, porque el manejador valida antes de escribir.

Y hay que dejar escrito que **el `409` es una red de seguridad, no el camino normal**: el manejador de un alta de tarea valida que las referencias existen antes de escribir, así que solo se llega al `409` si algo cambia **entre la comprobación y el `INSERT`** —otra petición borró la fila referenciada, o el `ON DELETE CASCADE` de un borrado en paralelo se llevó por delante lo que se iba a referenciar—. Si el `409` aparece en un caso de uso normal, la comprobación previa del manejador no está haciendo su trabajo, y eso es un fallo que hay que ver en el registro, no un comportamiento aceptable.

**La causa no se pierde** por contestar `4xx`: `sqlx::Error::Database` conserva `is_foreign_key_violation()`, que es lo que permite distinguir la violación de clave foránea de cualquier otra consulta fallida, y el `DbError::Consulta` lleva la sentencia **por tipo**, `&'static str`, así que por construcción no puede arrastrar el valor de una columna —ni el de una credencial— hasta el mensaje que va al registro.

#### Scenario: Un secreto vacío contesta 400 con el campo nombrado
- **GIVEN** un alta cuyo secreto es la cadena vacía
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el mensaje SHALL nombrar el campo del secreto
- **AND** NO SHALL contestar `500`

#### Scenario: Una clave foránea violada contesta 409 sin publicar la sentencia
- **GIVEN** una tarea cuyo target se borró entre la comprobación del manejador y la escritura
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `409` con `status` y `message`
- **AND** el cuerpo NO SHALL contener el texto de la sentencia ni el nombre de la restricción
- **AND** el motivo SHALL quedar en el registro del servidor
- **AND** el mensaje NO SHALL atribuir el fallo a un campo concreto de la petición

#### Scenario: Una consulta que falla por otra causa contesta 500 genérico
- **GIVEN** un fallo de base que no tiene su origen en la petición —una tabla que no existe, un fichero ilegible—
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `500` con el mensaje genérico de esta capability
- **AND** la causa SHALL quedar en el registro del servidor
- **AND** el cuerpo NO SHALL contener el texto de la causa

#### Scenario: El 4xx no filtra ni una credencial ni la ruta del fichero de la base
- **GIVEN** un fallo de base traducido a `400` o a `409`
- **WHEN** se lee el cuerpo de la respuesta
- **THEN** NO SHALL contener el valor de ninguna credencial
- **AND** NO SHALL contener la ruta del fichero de la base

### Requirement: Un cuerpo o una consulta que no se pueden interpretar contestan 400 con la envoltura, y su texto no sale del proceso

Un cuerpo que no es JSON válido, o cuyos tipos no encajan con los del contrato, SHALL contestar `400` con el **mismo cuerpo de error** que el resto de la API, y NO SHALL contestar con el texto plano que produce el extractor de axum por defecto. El texto del intérprete NO SHALL aparecer **ni en el cuerpo ni en el registro**.

El porqué está medido, y es de seguridad, no de estilo: el texto de un rechazo de `serde` **puede ecoar el valor del campo**. Un cuerpo como `{"keep_daily": "AKIAIOSFODNN7EXAMPLE"}` produce `invalid type: string "AKIAIOSFODNN7EXAMPLE", expected i64 at line 1 column 42`, y la credencial viaja dentro del mensaje. Reenviarlo la pondría en el cuerpo de la respuesta, y registrarlo la pondría en el log del contenedor; y basta con que un campo se llame `keep_daily` y quien llame pegue la credencial en la casilla equivocada para que eso ocurra por accidente. Los cuerpos de esta API llevan tres secretos, así que el accidente no es hipotético.

De ahí la doble regla: la respuesta pierde el texto del intérprete y se queda con la envoltura —que ya está definida para el `400` y que el panel sabe leer—, y el registro no lo recibe tampoco. Lo que sí se registra es **que** una petición no se pudo interpretar, sin el texto: es lo que hace falta para saber que hay un cliente mandando algo raro, y no lo que no puede estar en un log.

Lo mismo vale para los **parámetros de consulta**: un `?limit=abc` SHALL contestar `400` con la envoltura y no el texto del rechazo, por la misma razón —el valor del parámetro también entra en el mensaje del intérprete— y por una segunda: quien pide un listado no recibe ayuda alguna con un error de `serde`, mientras que un mensaje que diga que `limit` debe ser un número entre 1 y 200 le dice qué corregir.

#### Scenario: Un cuerpo que no es JSON contesta 400 con la envoltura
- **GIVEN** una petición con un cuerpo que no es JSON válido
- **WHEN** se atiende
- **THEN** SHALL contestar `400`
- **AND** el cuerpo SHALL ser JSON con `status` y `message`
- **AND** NO SHALL ser texto plano

#### Scenario: El rechazo de un cuerpo no publica la credencial en ninguna parte
- **GIVEN** `RUST_LOG=trace` y una petición con un campo de tipo equivocado que contiene una credencial distinguible
- **WHEN** se atiende y se revisan la respuesta y la salida del registro
- **THEN** SHALL contestar `400`
- **AND** la credencial NO SHALL aparecer en el cuerpo de la respuesta
- **AND** la credencial NO SHALL aparecer en el registro capturado

#### Scenario: Un parámetro de consulta mal formado contesta 400 con la envoltura
- **GIVEN** el router con una sesión válida
- **WHEN** se pide un listado con `?limit=abc`
- **THEN** SHALL contestar `400`
- **AND** el cuerpo SHALL ser JSON con `status` y `message`
- **AND** NO SHALL aparecer el texto del rechazo del intérprete
