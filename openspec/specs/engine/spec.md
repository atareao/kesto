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

### Requirement: Una copia que no se da por buena no deja su instantánea en el repositorio

Cuando el motor decide que una ejecución acaba en `failed` **después** de que el archivado haya escrito una instantánea, el motor SHALL retirar esa instantánea del repositorio, de modo que el repositorio NO SHALL quedarse con una instantánea que no corresponde a ninguna ejecución en `success`. La retirada SHALL ser de **mejor esfuerzo**: si retirar la instantánea falla, la ejecución SHALL acabar igualmente en `failed`, con el motivo **original** del volcado, y el fallo de la retirada NO SHALL sustituir ese motivo ni tumbar al worker.

El porqué: la biblioteca de archivado `rustic_core` **silencia** el error del lector y sigue, devolviendo `Ok` y guardando la instantánea **sin el volcado dentro**. El motor ya no hereda ese `Ok` —comprueba la entrega del volcado antes de dar la copia por buena—, pero sin retirar la instantánea queda en el repositorio un residuo de una copia que no llegó a serlo: aparece en el explorador de instantáneas, cuenta como espacio consumido y no se limpia nunca, porque la retención solo corre tras copias exitosas. La retirada es de mejor esfuerzo y no sustituye al motivo porque el fallo que importa —y el que el operador tiene que leer— es el de la copia; convertir «no pude limpiar» en un error que oculta «no pude copiar» sería peor que el residuo.

#### Scenario: Una copia fallida no deja instantánea en el repositorio
- **GIVEN** una ejecución cuyo volcado no se entrega entero después de que el archivado haya escrito una instantánea
- **WHEN** la ejecución acaba en `failed`
- **THEN** el repositorio NO SHALL contener esa instantánea
- **AND** el número de instantáneas del repositorio NO SHALL haber crecido con motivo de esa ejecución

#### Scenario: Un fallo del productor tampoco deja instantánea
- **GIVEN** una ejecución cuyo hilo de extracción falla tras haberse escrito la instantánea
- **WHEN** la ejecución acaba en `failed`
- **THEN** el repositorio NO SHALL contener esa instantánea

#### Scenario: El veredicto de la ejecución no lo cambia la limpieza
- **GIVEN** una copia fallida cuya instantánea no se consigue retirar
- **WHEN** se lee la fila de la ejecución
- **THEN** su estado SHALL ser `failed`
- **AND** `error_message` SHALL llevar el motivo del volcado, NO SHALL llevar el de la retirada
- **AND** el worker del repositorio SHALL seguir atendiendo la siguiente ejecución

### Requirement: La ejecución de restauración se persiste y su ciclo de vida lo escribe el motor

Toda restauración lanzada SHALL tener una fila en `restore_executions` **antes** de contestar el `200`, en estado `pending`, con su `snapshot_id`, su `target_id` y su `started_at`, y ese `id` SHALL ser el `execution_id` que devuelve la respuesta. Al empezar a trabajar SHALL pasar a `running`; al terminar con éxito, a `success` con `finished_at`; al fallar, a `failed` con `error_message` y `finished_at`. La escritura final SHALL ocurrir **también** cuando el trabajo falla, de modo que ninguna restauración se quede en `running` para siempre.

El trabajo de restauración SHALL correr fuera del runtime de Tokio, como el de copia, porque las llamadas de `rustic_core` que leen el repositorio son bloqueantes y la biblioteca construye su propio runtime.

El porqué: una ejecución en `pending` sin paso a `running`, o en `running` sin cierre, miente al operador y ensucia el histórico; y el motivo del fallo es justo lo que el operador necesita para saber si el problema fue la contraseña, el binario o el contenedor.

#### Scenario: La fila existe antes de responder

- **GIVEN** una petición de restauración aceptada
- **WHEN** llega el `200`
- **THEN** SHALL existir una fila en `restore_executions` con ese `execution_id` y estado `pending`

#### Scenario: El ciclo de vida se escribe

- **GIVEN** una restauración que empieza
- **WHEN** el motor la toma
- **THEN** su fila SHALL pasar a `running`
- **AND** al terminar bien SHALL quedar en `success` con `finished_at`

#### Scenario: Un fallo se registra con su motivo

- **GIVEN** una restauración que falla
- **WHEN** el trabajo termina
- **THEN** su fila SHALL quedar en `failed` con `error_message` y `finished_at`
- **AND** NO SHALL quedarse en `running`

### Requirement: El motor restaura según el tipo de target y reserva el destino antes de trabajar

El motor SHALL aceptar la restauración de un target `directory` y de un target de base de datos, y SHALL elegir el camino según el tipo: para `directory`, la escritura a `host_path` con `LocalDestination`; para `mariadb`, `postgresql` y `sqlite`, el flujo inverso de `restauracion` dentro del contenedor del target. NO SHALL enviarse al camino de directorio un target de base, ni al revés.

Antes de arrancar el trabajo, el motor SHALL **reservar el destino** —el `host_path` de un directorio, o el `container_name` junto con la base o el `db_path` de un target de base— y SHALL rechazar una segunda restauración sobre un destino ya reservado. La reserva SHALL liberarse al terminar el trabajo, también cuando falla.

#### Scenario: Un target de base va por el camino inverso

- **GIVEN** una restauración de un target `mariadb`
- **WHEN** el motor la ejecuta
- **THEN** SHALL leer el volcado del repositorio e inyectarlo por el `stdin` del contenedor
- **AND** NO SHALL escribir en el `host_path` del sistema de ficheros de Kesto

#### Scenario: La reserva impide dos restauraciones sobre el mismo destino

- **GIVEN** una restauración en curso sobre un destino
- **WHEN** se intenta una segunda sobre ese destino
- **THEN** el motor SHALL rechazarla
- **AND** la reserva SHALL liberarse al terminar la primera

### Requirement: El final de una ejecución se anuncia en el stream después de escribirse en su fila

El trabajador SHALL escribir el estado terminal —y las cifras medidas— en la fila de la ejecución **antes** de publicar el evento final en el stream global, y el evento final SHALL llevar el mismo `status`, `bytes_processed` y `bytes_uploaded` que la fila que acaba de escribirse.

Ese orden es deliberado y lo fija el §5: la fila queda constancia antes del aviso. Su consecuencia para quien observa es que la fila y el stream son **dos caminos con orden no garantizado entre sí**: ver la fila terminal no implica haber recibido ya el evento final. Por eso, quien mida el evento final —una prueba, un cliente— SHALL esperar en el **stream** hasta observar un estado terminal, y NO SHALL dar por hecho que el evento está entregado por el solo hecho de que la fila ya sea terminal.

#### Scenario: La fila terminal precede al aviso
- **GIVEN** una ejecución que termina
- **WHEN** el trabajador cierra la ejecución
- **THEN** SHALL escribir la fila terminal antes de publicar el evento final
- **AND** el evento final SHALL llevar el mismo `status` que la fila

#### Scenario: Las cifras del evento final coinciden con las de la fila
- **GIVEN** una ejecución que termina con cifras medidas
- **WHEN** se compara el evento final con la fila
- **THEN** `bytes_processed` y `bytes_uploaded` SHALL ser los mismos en ambos
- **AND** `percentage` SHALL ser el que corresponde al hecho medido —nulo sin total conocido, un número con él—

#### Scenario: Quien mide el evento final espera en el stream
- **GIVEN** una ejecución que termina
- **WHEN** un observador espera el evento final en el stream, no en la fila
- **THEN** SHALL recibir un evento con `status ∈ {Success, Failed}`
- **AND** NO SHALL quedarse con el último evento `running` por haber leído antes de que el aviso se publicara

### Requirement: La superficie de la API no enlaza el cliente Docker

El binario de la API SHALL compilarse sin `bollard`: la dependencia SHALL ser opcional y activarse solo con la feature `motor` del binario `kesto-motor`. Ningún handler ni estado de la API SHALL poseer `DOCKER_HOST` ni el socket del daemon; SHALL usar en su lugar un `MotorClient` que envía solicitudes tipadas por `KESTO_MOTOR_SOCKET`.

Un fallo, `panic`, `unwrap` explotable o inyección en `argv` en la superficie de la API SHALL NOT entregar el socket del daemon: la petición SHALL NOT llegar a bollard y el proceso motor SHALL seguir vivo. La propiedad SHALL comprobarse en el grafo de dependencias del binario API, no solo por inspección del código fuente.

#### Scenario: Un fallo en un handler no alcanza el daemon
- **GIVEN** un `panic` o un `unwrap` explotable en un handler de la API
- **WHEN** se intenta continuar hasta el daemon desde ese proceso
- **THEN** el binario API SHALL NOT poseer el socket ni `DOCKER_HOST`
- **AND** la petición SHALL NOT llegar al daemon
- **AND** el proceso motor SHALL seguir vivo

#### Scenario: El binario API no contiene bollard en su grafo
- **GIVEN** el árbol de dependencias del workspace con `bollard` marcado como opcional
- **WHEN** se inspecciona el grafo del binario de la API sin la feature `motor`
- **THEN** `bollard` SHALL NOT aparecer en el grafo de ese binario
- **AND** el binario `kesto-motor` SHALL compilarse con la feature `motor`

### Requirement: El motor ejecuta la lista blanca de operaciones del daemon

Todas las operaciones de bollard SHALL ejecutarse en el proceso `kesto-motor`, y solo como respuesta a una de las seis solicitudes permitidas por `motor-ipc`: `Comprobar`, `CrearContenedor`, `IniciarContenedor`, `CrearExec`, `InspeccionarExec` e `IniciarExec`. La API SHALL NOT enviar HTTP crudo ni pedir una operación fuera de esa lista.

Las solicitudes legítimas SHALL conservar el comportamiento actual: la etiqueta `io.kesto.backup-source=true` y las rutas confinadas se comprueban en el motor antes de cualquier `exec`, y la respuesta vuelve a la API como un tipo de `RespuestaMotor` sin credenciales ni rutas de socket.

#### Scenario: Un `exec`, `inspect`, `create` o `start` legítimo se ejecuta en el motor
- **GIVEN** solicitudes legítimas de la lista blanca enviadas por la API
- **WHEN** llegan al proceso motor con el UID autorizado
- **THEN** el motor SHALL ejecutarlas contra el daemon con el contrato actual
- **AND** la comprobación de la etiqueta y de las rutas confinadas SHALL ocurrir en el motor antes de cualquier `exec`

#### Scenario: La API no puede pedir una operación arbitraria
- **GIVEN** un handler de la API comprometido
- **WHEN** intenta pedir al motor una operación fuera de la lista blanca
- **THEN** el motor SHALL responder con un error tipado
- **AND** NO SHALL emitir ninguna llamada al daemon

### Requirement: Los argumentos de un `exec` llegan como argv literal

`Orden` SHALL construirse en el proceso motor y sus argumentos SHALL pasarse a bollard como una secuencia de cadenas literales. Un valor con metacomandos, punto y coma, sustitución de shell o saltos de línea SHALL permanecer dentro de un único argumento; NO SHALL interpretarse como shell ni como metacomando del cliente.

#### Scenario: Un argumento con metacomandos no se interpreta
- **GIVEN** un nombre de base o ruta con `;`, `$()` o un salto de línea
- **WHEN** el motor construye la `Orden` y crea el `exec`
- **THEN** el valor SHALL viajar como un argumento literal del argv
- **AND** NO SHALL ejecutarse ningún metacomando ni interpretarse como shell

### Requirement: El motor detecta cambios antes de archivar un volcado

El motor DEBE calcular, antes de archivar el volcado de una base de datos, un *manifest de
cambios* que identifique de forma estable el contenido de la BD. El manifest se compone de
la estructura (PostgreSQL: `pg_dump --schema-only`; MySQL/MariaDB: `mysqldump --no-data`) y
de un checksum de los datos por tabla (PostgreSQL: checksum de datos; MySQL/MariaDB:
`CHECKSUM TABLE`).

- Si el manifest coincide con el del último volcado archivado, el motor NO DEBE subir bytes
  de nuevo y DEBE crear una snapshot «sin cambios».
- Si el manifest difiere, el motor DEBE subir el volcado completo y etiquetarlo con
  `kesto.change_manifest=<hash>`.
- Si el manifest no se puede calcular, el motor DEBE degradar al comportamiento actual
  (subida completa forzada, sin etiqueta) en lugar de abortar el backup.

El motor NO DEBE escribir el volcado a disco ni a `/tmp`; el streaming en memoria se
mantiene.

#### Scenario: Mismo manifest produce una snapshot sin cambios

- **GIVEN** una base de datos cuyo contenido y esquema no han cambiado desde el último
  volcado archivado, con manifest `M`
- **WHEN** el motor archiva un nuevo volcado y el manifest calculado vuelve a ser `M`
- **THEN** el motor NO sube bytes del volcado de nuevo
- **AND** crea una snapshot «sin cambios» etiquetada con `kesto.change_manifest=M`
- **AND** restaurar esa snapshot devuelve el contenido del último volcado real con manifest `M`

#### Scenario: Manifest distinto produce una snapshot etiquetada con el nuevo manifest

- **GIVEN** una base de datos cuyo contenido ha cambiado desde el último volcado, con nuevo
  manifest `M2` distinto del anterior `M`
- **WHEN** el motor archiva el volcado y detecta `M2`
- **THEN** el motor sube el volcado completo
- **AND** la snapshot queda etiquetada con `kesto.change_manifest=M2`
- **AND** restaurar esa snapshot devuelve el contenido del volcado correspondiente a `M2`

#### Scenario: Fallo en la detección degrada a subida completa

- **GIVEN** una base de datos en la que el cálculo del manifest falla
- **WHEN** el motor intenta archivar un volcado
- **THEN** el motor sube el volcado completo forzando la escritura (sin dedup)
- **AND** la snapshot no lleva etiqueta `kesto.change_manifest`
- **AND** el backup finaliza sin abortar

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
