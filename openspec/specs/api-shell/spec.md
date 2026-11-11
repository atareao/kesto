# api-shell Specification

## Purpose
La forma de la API HTTP: la envoltura JSON única de las respuestas y de los errores, el modelo de error con su traducción a códigos y los dos *fallbacks* que garantizan que ninguna respuesta salga sin envoltura, la única excepción a esa envoltura —el stream de eventos, que no es JSON— y el endpoint de salud que el contenedor necesita para su `healthcheck`. Fija la envoltura antes de que existan endpoints que la consuman, para que ninguno improvise la suya; y la salud se limita a la versión y al modo de autenticación porque es un endpoint sin sesión y no tiene por qué describir el despliegue.

## Requirements

### Requirement: La salud contesta sin sesión y sin describir el despliegue

`GET /api/v1/health` SHALL contestar `200` sin exigir autenticación, y SHALL limitar su cuerpo a la versión del binario y el modo de autenticación efectivo. NO SHALL incluir la dirección de escucha, ni la cadena de base de datos, ni el emisor OIDC.

Es lo que consulta el `healthcheck` del contenedor, y un health check que exija sesión no sirve para su propósito; pero es también un endpoint sin autenticar, así que no tiene por qué describir el despliegue a quien lo consulte.

La ruta NO SHALL declararse con barra final.

#### Scenario: La salud contesta sin credenciales
- **GIVEN** el router de la aplicación
- **WHEN** se pide `GET /api/v1/health` sin cabecera de sesión
- **THEN** SHALL contestar `200`
- **AND** el cuerpo SHALL traer la versión del binario

#### Scenario: La salud no describe el despliegue
- **GIVEN** una configuración con una dirección de escucha y una cadena de base de datos propias
- **WHEN** se pide `GET /api/v1/health`
- **THEN** el cuerpo NO SHALL contener ni la dirección de escucha ni la cadena de base de datos

#### Scenario: La salud declara el modo de autenticación efectivo
- **GIVEN** una configuración sin ninguna variable `OIDC_*`
- **WHEN** se pide `GET /api/v1/health`
- **THEN** el modo declarado SHALL ser el de desarrollo

#### Scenario: La ruta sin barra final responde
- **WHEN** se pide `GET /api/v1/health`
- **THEN** SHALL contestar `200`

### Requirement: Una ruta desconocida contesta con la misma envoltura

Una petición a una ruta no declarada bajo `/api/v1` SHALL contestar `404` con el cuerpo JSON de error de la API, y NO SHALL contestar con un cuerpo vacío.

#### Scenario: Una ruta que no existe
- **GIVEN** el router de la aplicación
- **WHEN** se pide `GET /api/v1/no-existe`
- **THEN** SHALL contestar `404`
- **AND** el cuerpo SHALL ser JSON con `status` y `message`

### Requirement: Una envoltura única para las respuestas

Toda respuesta de la API SHALL ser JSON con `{status, message, data}` cuando la petición se atendió, y `{status, message}` cuando falló. El campo `status` SHALL duplicar el código HTTP de la respuesta.

#### Scenario: La forma de una respuesta correcta
- **WHEN** se atiende una petición con éxito
- **THEN** el cuerpo SHALL ser JSON con `status`, `message` y `data`
- **AND** `status` SHALL coincidir con el código HTTP

#### Scenario: La forma de una respuesta de error
- **WHEN** una petición falla
- **THEN** el cuerpo SHALL ser JSON con `status` y `message`
- **AND** `status` SHALL coincidir con el código HTTP

#### Scenario: Un método no declarado contesta con la misma envoltura
- **GIVEN** el router de la aplicación
- **WHEN** se pide `POST /api/v1/health`, ruta que solo admite `GET`
- **THEN** SHALL contestar `405`
- **AND** el cuerpo SHALL ser JSON con `status` y `message`

### Requirement: Un error interno no cuenta por qué falló

Una respuesta `5xx` SHALL llevar un mensaje genérico y NO SHALL incluir el texto de la causa. El detalle SHALL registrarse en el log del servidor.

Kesto va a manipular credenciales S3 y cadenas de conexión a bases de datos, y el mensaje de un error de biblioteca es justo donde acaban.

#### Scenario: Un fallo interno no filtra la causa
- **GIVEN** un error interno cuyo texto contiene una credencial
- **WHEN** se convierte en respuesta
- **THEN** el código SHALL ser `500`
- **AND** el cuerpo NO SHALL contener el texto de la causa

#### Scenario: Un error de entrada sí explica qué se esperaba
- **GIVEN** un error de entrada inválida
- **WHEN** se convierte en respuesta
- **THEN** el código SHALL ser `400`
- **AND** el cuerpo SHALL incluir el motivo

### Requirement: El cierre ordenado espera a las peticiones en curso

La aplicación SHALL cerrar el servidor cuando la señal de cierre se resuelva, y SHALL dar salida a las peticiones en curso antes de terminar, para que el `restart: always` del §8 no corte una respuesta a medias.

La señal de cierre SHALL ser inyectable, de modo que el mecanismo sea comprobable sin levantar un proceso ni enviarle señales.

#### Scenario: El servidor se apaga cuando la señal de cierre se resuelve
- **GIVEN** el servidor escuchando con una señal de cierre inyectada
- **WHEN** la señal se resuelve
- **THEN** el servidor SHALL dejar de aceptar conexiones
- **AND** la función de arranque SHALL devolver

#### Scenario: Las señales del sistema se atienden
- **GIVEN** el binario en ejecución
- **WHEN** recibe `SIGTERM`
- **THEN** SHALL cerrar el servidor de forma ordenada
- **AND** el proceso SHALL terminar con éxito

### Requirement: El router se parte en público y protegido

El router SHALL partirse en dos mitades: una **pública** con la salud y las dos rutas del flujo de entrada —`/api/v1/auth/login` y `/api/v1/auth/callback`—, y una **protegida** con el resto de la superficie de negocio, a la que el *gate* de sesión SHALL colgarse con `route_layer` para que no alcance a las rutas públicas.

La partición NO SHALL construirse con el middleware sobre el router entero más una lista de excepciones: añadir una ruta pública obligaría a una excepción más, y olvidarla la dejaría detrás del *gate* sin que nada lo delate. La salud y el flujo de entrada son la lista blanca.

Con la autenticación apagada NO SHALL montarse la mitad protegida: la superficie queda pública, que es el modo de desarrollo, y las rutas de `/api/v1/auth/*` NO SHALL declararse.

Una ruta **no declarada** bajo `/api/v1` NO SHALL pasar por el *gate*: SHALL seguir contestando el `404` de la envoltura sin exigir sesión. Es lo medido —`route_layer` no envuelve el *fallback* que resuelve las rutas no declaradas—, y **es correcto**: un `404` no revela ningún recurso ni ningún método, así que no hay nada que proteger detrás. NO SHALL forzarse el *gate* sobre ese `404`. Queda escrito con escenario propio para que nadie lo lea mañana como un agujero y lo «arregle» envolviendo el *fallback* a mano: eso acoplaría las dos mitades y rompería la partición que este requisito fija.

#### Scenario: Las rutas públicas no pasan por el *gate*
- **GIVEN** el router en modo `Oidc`
- **WHEN** se pide `GET /api/v1/health` sin cookie
- **THEN** SHALL contestar `200`
- **AND** NO SHALL contestar `401`

#### Scenario: La superficie de negocio cuelga del router protegido
- **GIVEN** el router en modo `Oidc` con una ruta de negocio declarada en la mitad protegida
- **WHEN** se pide sin cookie
- **THEN** SHALL contestar `401` con la envoltura

#### Scenario: En modo desarrollo no hay mitad protegida
- **GIVEN** una configuración sin ninguna variable `OIDC_*`
- **WHEN** se construye el router
- **THEN** la superficie SHALL ser pública
- **AND** NO SHALL aplicarse ningún `401` por falta de sesión

#### Scenario: Una ruta no declarada no pasa por el *gate*
- **GIVEN** el router en modo `Oidc`
- **WHEN** se pide una ruta no declarada bajo `/api/v1` sin cookie
- **THEN** SHALL contestar `404` con la envoltura
- **AND** NO SHALL contestar `401`

### Requirement: Una petición sin sesión válida contesta con la misma envoltura

Una petición a la superficie protegida sin cookie de sesión válida SHALL contestar `401` con el mismo cuerpo JSON de error que el `404` y el `405` que ya existen —`{status, message}`— y NO SHALL contestar con un cuerpo vacío, ni con HTML, ni con una redirección.

El `status` del cuerpo SHALL duplicar el código de la cabecera, como en el resto de la envoltura.

En la mitad protegida el *gate* SHALL decidir **antes** que el enrutado por método: `method_not_allowed_fallback` SHALL declararse **antes** de colgar la capa con `route_layer`, de modo que la capa envuelva también ese *fallback*. Así, un **método no declarado** sobre una ruta protegida contesta `401` sin sesión y `405` —el de la envoltura— con sesión.

El motivo SHALL quedar escrito, porque no se deduce leyendo el router y la medición lo confirma: `method_not_allowed_fallback` **sustituye** el *fallback* del `MethodRouter`, y solo lo hace si el *fallback* es el de por defecto, así que declararlo **después** del `merge` reemplaza un *fallback* que ya estaba envuelto por otro que nunca vio la capa. Ese `405` sale **sin haber exigido sesión**, revelando a quien no ha entrado qué métodos existen. **El orden de las dos líneas de construcción del router es semántico, no cosmético**: invertirlas no rompe la compilación ni falla ninguna prueba de ruta válida; solo abre ese agujero.

#### Scenario: Un método no declarado sin sesión no contesta `405`
- **GIVEN** el router en modo `Oidc` y una ruta protegida que solo declara `GET`
- **WHEN** se pide esa ruta con un método no declarado y sin cookie
- **THEN** SHALL contestar `401` con la envoltura
- **AND** NO SHALL contestar `405`

#### Scenario: Un método no declarado con sesión contesta `405`
- **GIVEN** el router en modo `Oidc`, una ruta protegida que solo declara `GET` y una cookie de sesión válida
- **WHEN** se pide esa ruta con un método no declarado
- **THEN** SHALL contestar `405` con la envoltura
- **AND** el cuerpo SHALL ser JSON con `status` y `message`

#### Scenario: El `401` trae la envoltura
- **GIVEN** el router en modo `Oidc`
- **WHEN** se pide una ruta protegida sin cookie
- **THEN** el cuerpo SHALL ser JSON con `status` y `message`
- **AND** `status` SHALL ser `401`

#### Scenario: El `401` no es una redirección ni un cuerpo vacío
- **GIVEN** una petición sin cookie a una ruta protegida
- **WHEN** se atiende
- **THEN** el cuerpo NO SHALL estar vacío
- **AND** la respuesta NO SHALL ser `3xx` ni traer cabecera `Location`

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

### Requirement: El stream de eventos es la única respuesta que no lleva la envoltura

`GET /api/v1/events` SHALL contestar `text/event-stream` y NO SHALL envolver su cuerpo en la envoltura JSON, que NO SHALL aplicarse a ningún otro punto de la API. La envoltura SHALL seguir rigiendo todas las demás respuestas, incluidas las de error del propio stream antes de abrirse.

Un `text/event-stream` con la envoltura dentro de un `data:` no es un `text/event-stream`: el cliente que lo lee espera líneas `event:` y `data:` y recibiría un objeto JSON que tendría que desenvolver, con lo que el formato dejaría de ser el que el §5 publica. Es una excepción y hay que declararla precisamente para que siga siendo **una**: si se dejara como un caso particular sin escribirlo, el día que aparezca una segunda ruta que quiera saltarse la envoltura nadie sabría si está permitido.

La excepción NO SHALL extenderse a los errores: una petición al stream sin sesión en modo OIDC contesta `401` **con** la envoltura, porque ahí sí es una respuesta HTTP normal. La envoltura se cae cuando el stream ya está abierto, no antes.

#### Scenario: El stream no lleva la envoltura
- **GIVEN** una sesión válida y una ejecución en curso
- **WHEN** se lee el cuerpo de `GET /api/v1/events`
- **THEN** SHALL estar formado por líneas `event:` y `data:`
- **AND** NO SHALL haber una clave `data` con la envoltura dentro

#### Scenario: Un error antes de abrir el stream sí lleva la envoltura
- **GIVEN** el modo OIDC con la capa puesta y una petición al stream sin sesión
- **WHEN** se atiende
- **THEN** SHALL contestar `401`
- **AND** el cuerpo SHALL llevar la envoltura

#### Scenario: Las demás respuestas siguen envueltas
- **GIVEN** una petición al listado de recursos y otra a una ruta desconocida
- **WHEN** se atienden
- **THEN** las dos SHALL llevar la envoltura
- **AND** la única excepción SHALL ser el stream ya abierto

### Requirement: El stream se declara aparte en el router y no se le aplican las capas que envuelven

La ruta del stream SHALL declararse de forma que NO reciba las capas que añaden o transforman la envoltura, y SHALL seguir recibiendo la capa de sesión y la de traza. El *fallback* de ruta desconocida SHALL seguir contestando con la envoltura.

El requisito anterior fija **qué** sale por el cable; este fija **cómo** se consigue en el router, porque la forma evidente de añadirlo —colgarlo de la mitad protegida como las demás rutas— es la que menos cuesta y la que rompe el formato si alguna capa toca el cuerpo. Las dos capas que sí tienen que llegarle son la de sesión, que es lo que lo protege, y la de traza, que es lo que permite ver en el registro que alguien se conectó. Y el `fallback` no cambia: una ruta que no existe sigue siendo un `404` con envoltura.

#### Scenario: El stream no pasa por las capas que envuelven
- **GIVEN** el router montado
- **WHEN** se pide `GET /api/v1/events` con sesión válida
- **THEN** el cuerpo SHALL llegar en formato de eventos
- **AND** NO SHALL haberse envuelto

#### Scenario: La sesión sí llega al stream
- **GIVEN** el modo OIDC con la capa puesta
- **WHEN** se pide el stream sin sesión
- **THEN** SHALL contestar `401`

#### Scenario: Una ruta desconocida sigue contestando con la envoltura
- **GIVEN** una petición a una ruta que no existe
- **WHEN** se atiende
- **THEN** SHALL contestar `404` con la envoltura
