# engine Specification

## Purpose
El motor de ejecuciones: quién decide que una copia empieza, dónde corre, en qué orden, qué deja escrito y cómo se entera el panel de lo que está pasando. Cubre la cola y los hilos de trabajo, el ciclo de vida de una ejecución en `job_executions`, el alta de la ejecución desde `POST /api/v1/jobs/:id/run` con sus códigos, el hub de eventos que alimenta el stream SSE y el arranque y la parada ordenada del conjunto. Fuera quedan la extracción del volcado —que es de `extraction`—, el trato con Rustic —de `repository`— y la evaluación del `cron_expression`, que es del change del planificador.

## Requirements

### Requirement: El motor trabaja en hilos propios y nunca dentro del runtime de Tokio

El trabajo de una ejecución SHALL correr en hilos de sistema operativo creados por el motor, y NO SHALL correr en una tarea del runtime de Tokio. La comunicación entre el motor y su trabajo SHALL hacerse por canales, y el acceso a la base desde esos hilos SHALL hacerse entrando explícitamente en el runtime, no dando por hecho que ya se está dentro.

No es una preferencia de estilo: es la única forma que funciona. El backend `opendal` de `rustic_backend` crea su **propio** runtime multi-hilo de Tokio y llama a `runtime().enter()` antes de cada operación. Y `Repository::archive()` es una función **bloqueante** que puede tardar minutos. Un `block_on` sobre un runtime que ya está atendiendo peticiones HTTP detiene el hilo que despacha, y con él el resto de la aplicación. De ahí las dos mitades de la regla: el trabajo va fuera del runtime, y quien vuelve a él para escribir en la base lo hace de forma explícita.

#### Scenario: El trabajo de una ejecución no ocupa un hilo de Tokio
- **GIVEN** una ejecución en curso con un volcado lento
- **WHEN** se atiende una petición a `GET /api/v1/health` desde otra conexión
- **THEN** SHALL contestar sin esperar a que la ejecución termine
- **AND** el latido del motor NO SHALL depender de que el volcado acabe

#### Scenario: Escribir en la base desde el hilo del motor funciona
- **GIVEN** una ejecución que avanza en su propio hilo
- **WHEN** el motor escribe el avance de esa ejecución en `job_executions`
- **THEN** la escritura SHALL completarse
- **AND** una lectura posterior al router SHALL ver el avance

#### Scenario: Un volcado que tarda no bloquea al resto de la API
- **GIVEN** el motor con una ejecución en curso sobre un volcado que no termina
- **WHEN** se piden el listado de targets y el de repositorios
- **THEN** los dos SHALL contestar `200` de inmediato

### Requirement: Una cola y un worker secuencial por repositorio, y ninguno de ellos paraleliza contra el mismo repositorio

Cada repositorio SHALL tener **una** cola y **un** worker, y ese worker SHALL procesar las ejecuciones de ese repositorio **de una en una**, en el orden en que se encolaron. Dos ejecuciones contra repositorios **distintos** NO SHALL esperar la una a la otra.

§2 del SRS lo pide con estas palabras —«un worker secuencial por repositorio S3 para garantizar el aislamiento de *locks*»— y el motivo está medido: `rustic_core` toma un **bloqueo exclusivo del repositorio**, así que dos copias contra el mismo repositorio lanzadas a la vez no corren más: se bloquean entre sí, y el que pierde espera sin que nada lo diga. Un repositorio es el recurso que hay que serializar, no la tarea: dos tareas que escriben en repositorios distintos pueden avanzar a la vez sin tocarse, y serían tiempo perdido si esperaran.

#### Scenario: Dos ejecuciones del mismo repositorio van en serie
- **GIVEN** dos tareas que apuntan al mismo repositorio y que se encolan en ese orden
- **WHEN** el motor las atiende
- **THEN** la segunda NO SHALL empezar hasta que la primera haya terminado
- **AND** el orden SHALL ser el de encolado

#### Scenario: Dos ejecuciones de repositorios distintos van en paralelo
- **GIVEN** dos tareas que apuntan a repositorios distintos
- **WHEN** el motor las atiende
- **THEN** las dos SHALL poder estar en estado `running` a la vez
- **AND** ninguna SHALL esperar a la otra

#### Scenario: Una cola vacía no consume un hilo ocupado
- **GIVEN** un repositorio sin nada encolado
- **WHEN** el motor está ocioso
- **THEN** su worker NO SHALL consumir CPU de forma apreciable
- **AND** encolar una ejecución SHALL despertarlo

### Requirement: El alta de una ejecución contesta 202 con la ejecución en `pending`, y 409 si la tarea ya está corriendo

`POST /api/v1/jobs/:id/run` SHALL contestar `202` con la ejecución recién creada en estado `pending` —con su `id` y su `job_id`, para que el cliente pueda seguirla—, SHALL contestar `404` si la tarea no existe, y SHALL contestar `409` si esa tarea ya tiene una ejecución en estado `pending` o `running`.

`202` y no `201`: la ejecución **no está hecha**, está aceptada. Un `201` diría que el recurso está creado y disponible, y lo que está es encolado. `409` y no un segundo `202`: encolar dos veces la misma tarea contra el mismo repositorio no acelera nada, solo pone a la segunda esperando a la primera y deja al panel con dos barras para un solo trabajo. Es un conflicto con el estado actual del recurso, que es exactamente lo que `409` significa.

Una tarea con `enabled = 0` SÍ SHALL poder dispararse a mano: `enabled` decide si el planificador la dispara por su `cron_expression`, y el disparo manual es otra cosa. Una tarea desactivada con un botón que la ejecuta igual es más útil que un botón que se niega sin explicar que el problema está en el planificador, que ni siquiera existe todavía.

#### Scenario: El alta encola y contesta 202
- **GIVEN** una tarea sin ninguna ejecución en curso
- **WHEN** se pide `POST /api/v1/jobs/:id/run`
- **THEN** SHALL contestar `202`
- **AND** el cuerpo SHALL traer el `id` de la ejecución y su `job_id`
- **AND** la ejecución SHALL estar en estado `pending` en `job_executions`

#### Scenario: Una tarea ya en marcha contesta 409
- **GIVEN** una tarea con una ejecución en estado `running`
- **WHEN** se pide `POST /api/v1/jobs/:id/run` otra vez
- **THEN** SHALL contestar `409` con la envoltura
- **AND** NO SHALL crear una segunda ejecución

#### Scenario: Una tarea que no existe contesta 404
- **GIVEN** un identificador que no corresponde a ninguna tarea
- **WHEN** se pide `POST /api/v1/jobs/:id/run`
- **THEN** SHALL contestar `404` con la envoltura
- **AND** NO SHALL crear ninguna ejecución

#### Scenario: Una tarea desactivada se puede disparar a mano
- **GIVEN** una tarea con `enabled = 0`
- **WHEN** se pide `POST /api/v1/jobs/:id/run`
- **THEN** SHALL contestar `202`
- **AND** SHALL crear la ejecución

### Requirement: Una ejecución recorre `pending`, `running`, `success` o `failed`, y el motor es el único que la escribe

El motor SHALL ser el **único** componente que escribe en `job_executions`. Una ejecución SHALL nacer en `pending`, SHALL pasar a `running` cuando su worker la toma, y SHALL acabar en `success` o en `failed`, nunca en otro estado y nunca dejando de ser una de esas cuatro. Al acabar SHALL quedar escrito `finished_at`, y si la ejecución falla SHALL quedar escrito un `error_message` con el motivo.

Los cuatro estados no los elige este change: los fija el `CHECK` del §3, y la spec de `db` ya exige que la columna acepte esos cuatro y solo esos cuatro. Lo que añade este requisito es **quién** los escribe y **cuándo**: hasta ahora la tabla solo la tocaban las pruebas, y a partir de aquí un fallo del motor deja de ser un fallo silencioso y queda como una fila que dice qué pasó. Una ejecución que se queda en `running` para siempre es la peor de las salidas posibles, porque el panel la pinta como «en marcha» y nadie se entera de que murió.

El `error_message` es texto que viene de fuera del proceso —del `stderr` de un `exec`, de un error de la biblioteca—, así que SHALL truncarse a una longitud acotada y NO SHALL llevar credenciales: es un campo que el panel muestra y que se guarda en la base, y las dos son formas de publicar un secreto.

#### Scenario: El ciclo de vida completo queda escrito
- **GIVEN** una tarea que se dispara y cuyo volcado termina bien
- **WHEN** la ejecución acaba
- **THEN** SHALL haber pasado por `pending` y por `running`
- **AND** SHALL acabar en `success` con `finished_at` informado

#### Scenario: Una ejecución que falla queda en failed con su motivo
- **GIVEN** un volcado que el contenedor rechaza con un mensaje en `stderr`
- **WHEN** la ejecución acaba
- **THEN** SHALL acabar en `failed`
- **AND** `error_message` SHALL llevar el motivo
- **AND** `finished_at` SHALL estar informado

#### Scenario: El mensaje de error no lleva la credencial
- **GIVEN** un fallo del `exec` en el que el propio mensaje del contenedor repite la contraseña
- **WHEN** se lee la fila de la ejecución
- **THEN** `error_message` NO SHALL contener esa contraseña

#### Scenario: Ninguna ejecución se queda en running
- **GIVEN** una ejecución cuyo volcado falla a mitad
- **WHEN** se lee su fila al acabar
- **THEN** su estado NO SHALL ser `running`
- **AND** `finished_at` SHALL estar informado

### Requirement: Un solo stream SSE global, con el payload literal del §5 y `execution_progress` como único evento

`GET /api/v1/events` SHALL ser un único stream **global** —uno que sirve a todos los clientes y no uno por ejecución— y SHALL emitir eventos con el nombre `execution_progress` y un `data` con exactamente los siete campos del §5: `execution_id`, `job_id`, `status`, `bytes_processed`, `bytes_uploaded`, `percentage` y `current_action`. NO SHALL inventarse otro nombre de evento ni otro juego de campos, y NO SHALL haber un stream por ejecución.

El §5 fija el mensaje con un ejemplo literal, y una API que emite un evento distinto del que su propia especificación publica es una API que nadie puede consumir sin leer el código. El panel se escribe contra ese payload.

`bytes_processed` SHALL ser el número de bytes que el motor ha **leído del `exec`** y `bytes_uploaded` SHALL ser el número de bytes que el **archivador ha procesado** según su gancho de progreso. Son dos cifras distintas medidas en dos sitios distintos, y así SHALL declararse: ninguna de las dos es un recuento de los bytes que han llegado a S3, y presentarlas como si lo fueran sería describir un hecho que el motor no observa.

#### Scenario: El evento llega con el payload del §5
- **GIVEN** un cliente conectado a `GET /api/v1/events`
- **WHEN** una ejecución avanza
- **THEN** SHALL recibir un evento llamado `execution_progress`
- **AND** su `data` SHALL traer los siete campos del §5

#### Scenario: Dos clientes ven el mismo evento
- **GIVEN** dos clientes conectados a la vez
- **WHEN** una ejecución avanza
- **THEN** los dos SHALL recibir el evento
- **AND** ninguno SHALL recibir un evento que el otro no reciba

#### Scenario: El status del evento sigue al de la fila
- **GIVEN** una ejecución que pasa de `pending` a `running` y a `success`
- **WHEN** se recogen los eventos de las tres transiciones
- **THEN** el `status` de cada evento SHALL coincidir con el estado de la fila en ese momento

### Requirement: `percentage` es nulo cuando el total no se conoce, y no se inventa

El campo `percentage` del evento SHALL ser **nulo** cuando el motor no conoce el tamaño total de lo que está copiando, y SHALL ser un número cuando lo conoce. NO SHALL informarse un porcentaje calculado sobre un total supuesto.

La causa está medida en la biblioteca, no supuesta: `StdinSource::size()` —el origen de un volcado por `stdout`— devuelve `Ok(None)`, y `LocalSource::size()` —el de un directorio— devuelve `Ok(Some(total))`. Nadie sabe cuánto va a pesar un `mariadb-dump` hasta que termina. Un campo que dijera `40` sin denominador sería una cifra inventada pintada como una barra de progreso, y el panel la mostraría con la misma autoridad que una de verdad.

#### Scenario: Un volcado sin total informa porcentaje nulo
- **GIVEN** una ejecución cuyo origen es el `stdout` de un `exec`
- **WHEN** se emite un evento de avance
- **THEN** `percentage` SHALL ser nulo
- **AND** `bytes_processed` SHALL seguir informado

#### Scenario: Un directorio con total informa porcentaje
- **GIVEN** una ejecución cuyo origen es un directorio local
- **WHEN** se emite un evento de avance
- **THEN** `percentage` SHALL ser un número entre `0` y `100`

### Requirement: El hub de eventos no crece sin límite y un cliente lento no frena al motor

El hub SHALL tener una capacidad acotada y SHALL descartar lo que no quepa, en vez de crecer. Un cliente que se queda atrás NO SHALL bloquear al motor ni ralentizar una ejecución, y el motor NO SHALL esperar a que un cliente lea.

Un hub sin tope es una fuga de memoria con forma de funcionalidad: el panel abre una pestaña, alguien cierra el portátil, y el motor sigue guardando eventos para un lector que ya no está. Y un hub que bloquea cuando el búfer se llena convierte a un cliente lento en un cliente que **para las copias**, que es el peor trueque posible: lo que hay que proteger es la copia, no el progreso. Un cliente que se pierde eventos no se queda a ciegas, porque `GET /api/v1/executions` sigue contando la verdad; lo que no puede es frenar a nadie.

#### Scenario: Un cliente que no lee no detiene la ejecución
- **GIVEN** un cliente conectado a `GET /api/v1/events` que no consume nada de lo que recibe
- **WHEN** una ejecución avanza y termina
- **THEN** la ejecución SHALL acabar en `success`
- **AND** SHALL hacerlo sin esperar al cliente

#### Scenario: El hub no acumula sin tope
- **GIVEN** una ejecución muy habladora y ningún cliente leyendo
- **WHEN** se emiten más eventos de los que caben en el hub
- **THEN** el hub NO SHALL crecer por encima de su capacidad
- **AND** la ejecución NO SHALL fallar por ello

#### Scenario: El histórico sigue contando la verdad sin SSE
- **GIVEN** un cliente que se perdió eventos por ir lento
- **WHEN** consulta `GET /api/v1/executions`
- **THEN** SHALL ver el estado final de la ejecución

### Requirement: El arranque monta el motor y el cierre ordenado lo para

El arranque SHALL montar el motor **después** de abrir y migrar la base y **antes** de empezar a escuchar, y el cierre ordenado SHALL dejar de aceptar trabajo nuevo, SHALL cerrar el reparto de eventos —de forma que un cliente conectado al stream **que lea** deje de recibir y su conexión termine—, SHALL esperar de forma **acotada** a la ejecución en vuelo, y SHALL unir sus hilos. Lo que no termine dentro de esa espera SHALL registrarse como tal, y el proceso SHALL acabar igualmente.

El orden importa por lo mismo que en `api-endpoints`: un motor arrancado contra una base sin migrar escribe en tablas que no existen. Y la parada acotada importa porque una copia puede durar minutos: esperarla sin límite convierte un `docker stop` en una espera indefinida que el orquestador acaba resolviendo con un `SIGKILL`, que es justo lo que la parada ordenada existía para evitar. La spec de `api-shell` ya exige que el cierre espere a las peticiones en curso; esto es lo mismo aplicado a lo que tarda mucho más. Y el reparto se cierra **antes** de esperar a las conexiones, que es lo que no se deduce del resto: el cuerpo de un `text/event-stream` no tiene final propio —su única salida es el cierre del canal—, y el cierre del servidor no resuelve hasta que todas las conexiones lo hacen, así que con un cliente conectado el proceso se quedaría vivo hasta que el período de gracia del orquestador lo matara con `SIGKILL`, con el hilo de trabajo a mitad de archivar.

Que un socket de Docker inalcanzable **no** impida arrancar es deliberado y va en la misma dirección: el panel tiene que poder levantarse para enseñar que el socket no responde. Un proceso que se niega a arrancar por un socket caído deja al operador sin la herramienta con la que diagnosticarlo.

#### Scenario: El motor arranca detrás de la base y delante del listener
- **GIVEN** un arranque con la base abierta y migrada
- **WHEN** el proceso empieza a escuchar
- **THEN** el motor SHALL estar montado
- **AND** SHALL poder aceptar trabajo desde la primera petición

#### Scenario: La parada deja de aceptar trabajo y espera lo acotado
- **GIVEN** el proceso con una ejecución en curso
- **WHEN** llega `SIGTERM`
- **THEN** NO SHALL aceptarse trabajo nuevo
- **AND** el proceso SHALL acabar tras la espera acotada, aunque la ejecución no termine

#### Scenario: El cierre ordenado termina con un cliente conectado al stream
- **GIVEN** el proceso con un cliente conectado a `GET /api/v1/events`
- **WHEN** llega `SIGTERM`
- **THEN** el stream SHALL cerrarse
- **AND** el proceso SHALL acabar dentro del plazo acotado, pese a la conexión abierta
- **AND** SHALL hacerlo por su cuenta, sin que el período de gracia del orquestador tenga que matarlo con `SIGKILL`

#### Scenario: Un socket de Docker inalcanzable no impide arrancar
- **GIVEN** un arranque en el que el socket configurado no responde
- **WHEN** el proceso arranca
- **THEN** SHALL arrancar y escuchar
- **AND** SHALL dejar constancia de que el socket no responde
- **AND** el fallo SHALL aparecer al ejecutar una copia, no al arrancar

### Requirement: Un fallo de una ejecución no tumba al worker ni al proceso

Un fallo de una ejecución SHALL quedar registrado en su fila y SHALL dejar al worker **listo** para la siguiente. NO SHALL propagarse como un pánico que mate el hilo ni el proceso.

Un motor en el que el primer volcado fallido deja el repositorio clavado es un motor que hay que reiniciar cada vez que una base no responde, y en un orquestador de copias lo normal es que algo falle. La ejecución que falla se apunta como `failed` con su motivo y el worker vuelve a su cola: el fallo se cuenta, no se hereda.

#### Scenario: Tras una ejecución fallida, el worker sigue vivo
- **GIVEN** una ejecución que falla
- **WHEN** se encola la siguiente ejecución del mismo repositorio
- **THEN** SHALL arrancar sin reiniciar el proceso
- **AND** SHALL poder acabar en `success`

#### Scenario: Una tarea que no resuelve no detiene el motor
- **GIVEN** una ejecución cuyo target apunta a un contenedor que no existe
- **WHEN** el motor la atiende
- **THEN** SHALL acabar en `failed` con el motivo
- **AND** el motor SHALL seguir aceptando trabajo
