# FEATURE-005 — backup-engine

> Implementación del change proposal `backup-engine`, en `openspec/changes/archive/2026-10-25-backup-engine/` — **33 requisitos y 107 escenarios** en tres capabilities nuevas (`engine` 9/28, `extraction` 8/25 y `repository` 7/26) más tres deltas (`api-resources` 5/17, `config` 2/5 y `api-shell` 2/6), con su `design.md`. Las specs fusionadas pasan `openspec validate --specs --strict` —**`Totals: 9 passed, 0 failed (9 items)`**— y el change archivado lo comprueba `openspec validate --archived`. `tasks.md`: **82 tareas**.
> Cerrada el 2026-10-25. Rama: `feature/backup-engine`, desde `development`. Plan: `plans/PLAN-005-COMPLETED.md`. Especificación de origen: `docs/DEFINITION.md` §2 (arquitectura), §4 (flujos) y §5 (los tres endpoints que faltaban).
> Diff contra `development`: **dos medidas**, y no coinciden. La cabeza que el CI verificó —`22ad31a`, el run citado más abajo—: **44 ficheros, +27 116 / −886**, en **22 commits**. El árbol que aterriza en `development`: **53 ficheros, +28 306 / −900**, en **24 commits** —de más, porque después vinieron el traslado del change a `openspec/changes/archive/`, las tres specs fusionadas y este documento—. `Cargo.lock`: **302 → 590** paquetes — es el primer change con dependencias pesadas (`bollard` 0.18.1, `rustic_core` 0.13.0, `rustic_backend` 0.7.0 y la promoción de `futures-util`) y el lock es un entregable, porque el CI corre `cargo test --locked`.
> Estado: **cerrado** — **405 pruebas** declaradas: **401** en el binario, de las que **20** van `ignored` porque exigen un motor de contenedores, y **4** de integración en `tests/arranque.rs`. **+191** netas sobre las 214 de `api-endpoints`.
> PR: [#10 — `feature/backup-engine` → `development`](https://github.com/atareao/kesto/pull/10) · CI en verde (run [`40222004103`](https://github.com/atareao/kesto/actions/runs/40222004103)). **Sin release a `main`.**

**Kesto copia.** Hasta este change, el orquestador guardaba las cuatro tablas del §3, las servía por HTTP con la superficie que levantó `api-endpoints`, y **nada escribía jamás una ejecución**: `job_executions` solo la tocaban las pruebas de la capa de acceso, y los tres endpoints del §5 que faltaban faltaban precisamente porque dependían de lo que aquí se construye. Ahora una tarea se dispara a mano, el volcado sale del contenedor de la base **sin que un byte pase por el disco**, entra en un repositorio Rustic sobre S3, la retención se aplica, el panel lo sigue por SSE y los snapshots se exploran desde el repositorio: los tres módulos del §7 que estaban vacíos —`docker`, `rustic` y `engine`— dejan de estarlo.

## Qué es y qué no es

Es el **motor sin planificador**: la extracción del volcado, el trato con Rustic, la orquestación con cola y eventos, las tres rutas del §5 que se habían diferido y el contrato nuevo del alta de un repositorio.

### Fuera del alcance, con destinatario cada uno

- **`src/scheduler` y la evaluación de `cron_expression`** — el planificador. **Consecuencia que hay que declarar en claro: no hay cron.** `cron_expression` se guarda, se sirve y se valida en el alta, pero **nadie la evalúa**, así que un trabajo solo corre cuando alguien llama a `run`. El botón manual y el planificador son dos cosas distintas, y este change solo trae la primera.
- **`ui/` y el `Dockerfile`** — las dos guardas del pipeline siguen saltándose solas, y `just check-all` no puede pasar mientras no exista el panel.
- **La restauración** — `GET /api/v1/snapshots` **lista** instantáneas; no abre una, no enumera su contenido y no devuelve nada a su sitio.
- **Los `keep_*` que el §3 no guarda** — no se añaden columnas: lo que el esquema no guarda, el motor no lo configura, y las otras ocho familias se quedan con el valor por defecto de la biblioteca.
- **La reconexión y el relleno del SSE** — un cliente que llega tarde no recibe lo que se perdió; para eso está `GET /api/v1/executions`.

De los módulos del §7, **solo `scheduler` sigue vacío**: `docker`, `rustic` y `engine` son nuevos, y `api` y `db` crecen. Y siguen sin existir `ui/` y el `Dockerfile`.

## Los cuatro módulos y el orden de entrega

Cada uno con su ciclo RED → GREEN → auditoría independiente → correcciones, y en este orden porque es una cadena de dependencias y no una preferencia: el motor necesita extraer y archivar antes de poder orquestar, y las rutas no tienen nada que servir hasta que el motor acepta trabajo.

| # | Módulo | Contenido | Ficheros |
|---|---|---|---|
| 1 | El transporte de Docker | Conexión por el socket configurado, `create_exec`/`start_exec`/`inspect_exec`, la salida troceada por canal y el código de salida | `src/docker/{mod.rs,cliente.rs}` |
| 2 | Rustic y el puente | El backend S3, los tres desenlaces, `archive`, el `ReadSource` propio, la retención y el listado | `src/rustic/{mod.rs,error.rs,repositorio.rs,origen.rs,retencion.rs,progreso.rs}` |
| 3 | El motor y la extracción | La cola y el trabajador por repositorio, el ciclo de vida, el hub, los comandos por tipo de target y la elección de ruta | `src/engine/{mod.rs,cola.rs,ejecucion.rs,hub.rs,extraccion.rs}` |
| 4 | La capa HTTP y el estado | Las tres rutas, el SSE fuera de la envoltura, el alta con destino, `DOCKER_HOST`, el motor en el `AppState` y el arranque | `src/api/{mod.rs,jobs.rs,repositories.rs,events.rs,snapshots.rs}`, `src/config.rs`, `src/state.rs`, `src/main.rs` |

Tandas medidas en el binario, con el número de `ignored` de cada momento (`N/M` = pasan / ignoradas por exigir un motor de contenedores):

| Módulo | Tandas |
|---|---|
| 1 · Docker | 224/7 → 225/11 |
| 2 · Rustic | 234/11 → 244/11 → 247/11 → 263/11 → 276/11 |
| 3 · Motor | 295/16 → 306/18 → 313/18 → 343/20 |
| 4 · HTTP | 353/20 (+1 de integración) → 376/20 → **401/20** + **4** de integración |

Las veinte `ignored` son las de contenedor, y **están verdes y medidas**: `DOCKER_HOST=unix:///run/user/1000/podman/podman.sock cargo test --locked -- --ignored` contesta `test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 381 filtered out` en **34,40 s** —y ese `381` es justo el cruce que cuadra el `401` del binario que declara la cabecera—. Tampoco son peso muerto: las veinte piden el socket del motor con un `expect`, así que sin motor **habrían fallado** en vez de pasar. **El CI no tiene motor de contenedores**, y esa es la razón de que existan como tales en vez de fingirse.

## Un trabajador secuencial por repositorio

Lo que se serializa es el **repositorio**, no la tarea, y el motivo está medido: `rustic_core` toma un **bloqueo exclusivo del repositorio**, así que dos copias contra el mismo repositorio lanzadas a la vez no corren más —se bloquean entre sí, y la que pierde espera sin que nada lo diga—. Dos tareas que escriben en repositorios distintos, en cambio, no se tocan.

La estructura es un mapa `repository_id → buzón`, y el buzón se crea **perezosamente**, la primera vez que se encola algo para ese repositorio: los repositorios sin trabajo no tienen hilo. El canal es un `sync_channel` de **64** ejecuciones y el encolado es **`try_send`**, nunca `send`: quien encola viene del runtime —una petición HTTP— y bloquear un hilo de Tokio por una cola llena es justo el fallo que este change existe para no cometer. Una cola llena es un `503` con nombre propio, y la fila que se acababa de crear se apunta como fallida antes de devolverlo: una fila `pending` que nadie va a ejecutar miente en el panel para siempre.

El trabajador ocioso se queda **bloqueado en la recepción** del canal: no sondea, no gasta CPU, y encolar lo despierta. Un fallo de una ejecución **no** se lleva al trabajador: el bucle envuelve cada ejecución en `catch_unwind`, y un pánico apunta la fila como `failed` con un motivo propio —el texto del pánico es arbitrario y se queda en el registro— y deja el trabajador listo para la siguiente. Sin esa red, el pánico dejaría la fila en `running` para siempre y la cola de ese repositorio sin nadie que la atienda. Y **el `repository_id` viaja con la ejecución**, sin volverse a resolver al ejecutar —es una corrección de auditoría—: el buzón se elegía con un identificador leído al encolar y la copia usaba otro leído al ejecutar, así que con una tarea editada en medio dos copias podían coincidir en el mismo repositorio sin que nadie las serializara —lo que `rustic_core` no admite—.

## La extracción: del contenedor al repositorio sin pasar por el disco

`ruta()` decide **por dónde** sale la copia y no toca ni el sistema de ficheros ni el motor: un target de base sale por `Ruta::PorContenedor { contenedor, orden }` y uno de directorio por `Ruta::PorFichero { ruta }`.

### El comando, la credencial y el directorio

Es el **cabo suelto 3 del SRS, cerrado**, y lo que se cierra no es una interpretación nueva sino una incoherencia: el §4.A fija `mariadb-dump --all-databases` y `pg_dumpall`, que vuelcan el servidor **entero** e ignoran `backup_targets.db_name`, así que con ellos la columna del §3 era decorativa.

| Tipo | `db_name` nulo | `db_name` informado |
|---|---|---|
| `mariadb` | `mariadb-dump --single-transaction --quick --lock-tables=false -u<db_user> --all-databases` | el mismo comando, nombrando esa base en vez de `--all-databases` |
| `postgresql` | `pg_dumpall -U <db_user>` | `pg_dump -U <db_user> -d <db_name>` |

`pg_dumpall` **no puede** volcar una sola base: para el caso concreto hace falta `pg_dump`, que es otro binario. Y `--single-transaction` con `--lock-tables=false` es lo que permite volcar una base en producción sin bloquearla.

Tres guardas sobre los campos que acaban en la línea de comandos: un campo que el tipo exige y falta, uno informado **y en blanco** —que no nombra nada y cuyo fallo aparecería como «ese contenedor no existe»— y uno cuyo valor **empieza por `-`**, que el cliente leería como una opción y el alcance del volcado cambiaría sin que nada lo note. No hay intérprete de por medio, así que no se cita ni se recorta: se rechaza.

La contraseña viaja en el **entorno** del `exec` —`MYSQL_PWD` para MariaDB y `PGPASSWORD` para PostgreSQL— y **nunca** en el `cmd`: es el **cabo suelto 4 del SRS, cerrado**, porque en un contenedor la línea de comandos es pública para cualquiera con acceso al listado de procesos, que es justo donde más gente la mira. Sin contraseña no se añade variable ninguna, y una contraseña **vacía** tampoco: `MYSQL_PWD=` no significa «sin contraseña», significa «con contraseña vacía».

El `Debug` de la orden publica el comando y los **nombres** de las variables, nunca sus valores. Eso es **necesario y no suficiente**, y está medido: `bollard` registra a `DEBUG` el **cuerpo serializado** de la petición, con el `env` dentro, y el suscriptor instala el puente `log` → `tracing`, así que la contraseña sale impresa en claro con el registro hablador. La exposición se corta **por filtro** —`bollard=warn`, declarado donde arranca el proceso y detrás del filtro que pida quien lance el binario— y el arranque **avisa** si el filtro que llega pide el motor más hablador que eso. Una credencial no se protege solo con tipos: hay que ejercitar el camino real con el registro hablador puesto.

Un target de **directorio no pasa por Docker**: se lee con el origen local de la biblioteca desde su `host_path`, y `host_path` es la ruta **tal como la ve Kesto dentro de su propio contenedor** —el `compose` del §8 monta los directorios del host en `/backup_sources/...` en solo lectura—. Se declara como **añadido al SRS**, porque el §4 solo fija los comandos de las dos bases. Consecuencia útil y medida: un directorio se copia aunque el socket del motor de contenedores esté caído.

### La acotación por inactividad y el puente

El `timeout` del cliente se resuelve **cuando llegan las cabeceras**, así que el cuerpo del `exec` se drena **fuera** de él: no acota el volcado. Y no acotar nada no produce un fallo aislado sino un atasco permanente —la ejecución se queda en `running` y con ella ocupado el trabajador secuencial de su repositorio—. Por eso se acota el **tiempo sin recibir un solo byte**, con `INACTIVIDAD_MAXIMA` de **cinco minutos**: un volcado que avanza tarda lo que tenga que tardar y no puede fallar por tardar, y uno que se calla falla con un motivo que nombra el contenedor y el plazo.

Son **tres** desenlaces, no dos, y confundir los dos últimos es el peor error posible: `Ok(Some(..))` es un trozo, `Ok(None)` es que el proceso terminó —posiblemente sin escribir nada, que es una base vacía— y el plazo vencido es que está colgado. Sin esa distinción, un volcado colgado acabaría en un snapshot vacío dado por bueno. El mismo plazo acota las **dos** esperas del volcado —el `stream` del `exec` y el canal del puente— y es **una** constante porque son la misma pregunta vista desde dos sitios.

El §4.A dibuja además un búfer que la biblioteca consume como `impl AsyncRead`; la API real de `rustic_core` 0.13 pide un trait **bloqueante**. Entre los dos va un puente —hilo productor, canal acotado y un lector—, y su forma está medida, no elegida:

- **El canal es acotado**: `CAPACIDAD_DEL_CANAL` son **128 trozos** de 8 KiB como mucho, es decir **1 MiB** retenido por copia en vuelo. Sin tope, lo retenido pasa a ser una función del tamaño de la base.
- **El final no viaja como mensaje**: medido, un `try_send` del final se encontró el canal **lleno** justo al terminar y el volcado se descartó. Va en una señal atómica que el productor pone después del último byte y que el lector solo da por buena cuando el canal se ha cerrado.
- **La espera del lector tiene plazo**: el canal se cierra cuando muere el **último** emisor, así que un extremo de envío retenido dejaba `archive()` sin volver —medido: hubo que matar la ejecución a los 400 s—. El extremo del productor es un tipo que **no se puede clonar** y declarar el final lo consume.
- **`size()` puede contestar que no sabe**: es lo que hace un volcado por `stdout`, y de ahí que `percentage` sea nulo por construcción y no por comodidad.

El motivo que se guarda en la base —el `stderr` del contenedor— pasa por una red que **quita primero los valores del entorno y recorta después**, en ese orden y no al revés: al revés, una contraseña en el borde del límite se cortaría por dentro y se guardarían sus primeros caracteres, que es publicar media credencial creyendo haberla quitado. El recorte es de **1024 caracteres**, por carácter y no por byte, con `…` para que se lea que está incompleto.

## La condición de éxito no se hereda del archivado

Es el hallazgo más grave del change, y está medido: el archivado de la biblioteca **silencia** el error del lector con un aviso, sigue y devuelve un resultado correcto **guardando la instantánea sin el volcado dentro** —la entrada que falló no llega al árbol—, y en el peor caso medido sin una sola línea de aviso: un volcado que no se generó acaba siendo una copia válida de los datos de ayer. Heredar el éxito de ese `Ok` daría por buena una copia que existe, ocupa sitio y no se puede restaurar, así que la condición la compone el motor con **tres** cosas que sí puede observar, y **ninguna** es el `Ok` del archivado:

1. el volcado se entregó **entero** —la señal del puente—;
2. el proceso de extracción salió con **cero** —por eso el productor solo declara el final si el código es cero, y el puente falla cerrado: cualquier otro código se lee como truncado—;
3. la instantánea **tiene el volcado dentro** (`ficheros >= 1`).

De los dos números que el archivado cuenta, solo **uno** sirve para decidir: `bytes` vale `0` tanto en una copia truncada como en una legítimamente vacía —una base vacía produce un volcado vacío perfectamente bueno—, y `ficheros` sí distingue: medido, el truncado deja `0` y el volcado de longitud cero deja `1` con el nodo presente.

Y una segunda medición, del mismo apartado: el nodo que el puente fabrica para el volcado no lleva tamaño ni fechas —no puede llevarlos—, así que **empareja con el del snapshot anterior**, la biblioteca descarta el lector sin leerlo y el snapshot de hoy apunta al contenido de ayer. Medido restaurando: la segunda copia devolvía los bytes de la primera. El archivado desactiva el emparejamiento con el padre —que es lo que la propia biblioteca hace para su entrada estándar—, y perder esa deduplicación no es una pérdida real: un origen de flujo no puede probar que no ha cambiado sin leerlo.

## La retención, en tres operaciones

Los tres `keep_*` del esquema **no** son parámetros del `prune`: son reglas de retención, y confundir las dos cosas escribe un motor que purga datos que nunca dejaron de estar referenciados.

1. **Decidir**: las instantáneas se leen **del repositorio** y se agrupan por host, etiqueta y rutas —la retención es de la tarea, y dos tareas que compartan repositorio no tienen por qué decidir la una sobre las de la otra—; las reglas producen la lista de identificadores a retirar.
2. **Retirar**: la operación de borrado de instantáneas, y solo si la lista no está vacía.
3. **Purgar**: el plan se calcula **antes** de ejecutarlo y se **mira** —el informe lleva los bytes que el plan dice que va a reclamar—. No es «bytes liberados»: la biblioteca marca los paquetes y los conserva un tiempo, así que el espacio se recupera en una purga posterior.

Dos reglas más: **una retención que falla no cambia el resultado de la copia** —la copia ya está hecha y es correcta, y lo que hay que contar es que la copia está y la retención no—, y **las tres reglas a cero se rechazan**, en el motor y en el alta y la edición de una tarea. Un cero suelto es una instrucción legítima; las tres a la vez no son una retención que conserva de menos: son la destrucción de todo el historial del repositorio, **incluida la copia recién hecha**. El §3 pone `DEFAULT 7` en esas columnas pero **sin cota**, así que el valor entra por la API y entra a mano con `sqlite3`; las dos capas existen porque protegen de dos caminos.

### La semántica de la biblioteca, que es lo que hay que saber

«Una por día» es lo que se espera de `keep_daily` y **no es toda la verdad**, y esto es el hallazgo más caro del change. La biblioteca ordena las instantáneas de la **más nueva a la más vieja** y conserva cuando `!has_next || last.is_none() || !check_fun(sn, last)`, con `equal_day` comparando año y **día del año**. La derivación, con doce copias del mismo día y `keep_daily = 7`:

- la **más nueva** entra por `last.is_none()` y consume un punto del contador (7 → 6);
- las **de en medio** se saltan, porque `equal_day` es cierto con la anterior y la condición no se cumple;
- y la **más vieja** entra por `!has_next` —es la última del recorrido— mientras queden puntos (6 → 5).

Resultado medido en un repositorio real: **dos instantáneas**, la primera y la última. Con `keep_daily = 1` queda **una sola**, porque cuando el recorrido llega a la más vieja el contador ya está a cero. **No es un defecto de Kesto, es la biblioteca**, y ese es también el motivo de que las pruebas del módulo usen `keep_daily = 1` y no cubran el caso. La consecuencia para el operador: con copias del mismo día, una retención diaria conserva la primera y la última, no «la más reciente de cada día».

## El motor: el ciclo de vida y los eventos

Cuatro estados —`pending`, `running`, `success`, `failed`— y los fija el `CHECK` del §3, no este change. Lo que añade el motor es **quién** los escribe y **cuándo**: es el **único** componente que escribe en `job_executions`, y **ningún camino de error deja la fila en `running`**: cada uno escribe `failed` con su motivo y su `finished_at`. Una ejecución que se queda en `running` para siempre es la peor salida posible, porque el panel la pinta como «en marcha» y nadie se entera de que murió.

El alta la crea en `pending` con una **guarda atómica** de una sola sentencia (`INSERT … SELECT … WHERE NOT EXISTS … RETURNING`): con la forma «leer y luego insertar», medido, pasaban **tres de seis** disparos simultáneos sobre la misma tarea. El `error_message` se trunca y pasa por la misma red de secretos que el motivo del contenedor —el campo se muestra en el panel y se guarda en la base, y las dos cosas son formas de publicar una credencial—.

### Un solo stream global, con el payload literal del §5

`GET /api/v1/events` es **un** stream global —no uno por ejecución— con `execution_progress` como único nombre de evento. Sus siete campos, con el tipo con el que viajan:

| Campo | Tipo | Qué es |
|---|---|---|
| `execution_id` | texto (UUID canónico) | la ejecución del aviso |
| `job_id` | texto (UUID canónico) | la tarea a la que pertenece |
| `status` | texto | el estado de la fila, del `CHECK` del §3 |
| `bytes_processed` | entero | los bytes que han **salido** de la extracción |
| `bytes_uploaded` | entero | los bytes que el archivador ha leído, según su gancho |
| `percentage` | número o `null` | solo con total conocido y mayor que cero |
| `current_action` | texto | qué está haciendo ahora mismo, **en español** |

`bytes_processed` y `bytes_uploaded` son dos cifras distintas medidas en dos sitios distintos y **ninguna de las dos cuenta los bytes que han llegado a S3**: medido en el archivador de la biblioteca, su contador de bytes se incrementa **fuera** del `if` de la deduplicación, así que cuenta bytes **leídos**. Presentarlas como «subido a S3» sería describir un hecho que el motor no observa.

`percentage` es **nulo cuando el total no se conoce**, y no se inventa: medido, la biblioteca **no llama nunca** a `set_length` en la barra de bytes de un origen de flujo, así que un volcado de base no tiene denominador y su porcentaje es nulo por construcción; un directorio sí declara su total. Un `total == 0` también da nulo: un `0 %` o un `100 %` inventados serían una barra pintada sobre una cifra falsa. El valor se acota a 100 —el total es un pre-escaneo y lo contado son bytes leídos, así que un fichero que crece durante la copia puede pasarse—.

El cable vive en la capa HTTP y **no** en el hub: el hub reparte un tipo —con el `status` como enumerado, que es lo que garantiza que son los cuatro del `CHECK`— y traducirlo a las líneas del protocolo es cosa del manejador. El stream **no manda latido**, y es una decisión: una línea de comentario sería una tercera clase de línea que el contrato no declara. Se suscribe **al atender la petición**, así que no promete historia —para eso está `/executions`— y un cliente que se queda atrás recibe el aviso de cuántos eventos perdió, se registra y **sigue leyendo**: cerrarle la conexión lo castigaría dos veces, y avisarle con un evento propio sería inventarse el cable.

### El reparto: capacidad, descarte y cierre

El hub es un `broadcast` con capacidad **256** eventos, y la elección resuelve las dos mitades del requisito sin código propio: **tiene tope**, así que no crece —un hub sin tope es una fuga de memoria con forma de funcionalidad— y **descarta** lo que no cabe en vez de bloquear, así que un cliente lento pierde eventos y **no frena al motor**. Publicar nunca falla y nunca bloquea: sin suscriptores se descarta, y después del cierre es un no-op silencioso. El emisor es **uno solo**, dentro de un `Arc<Mutex<Option<…>>>`, y el `Arc` es la mitad del arreglo del apagado: el estado de una ejecución en curso guarda un clon del hub, así que con un emisor por clon, cerrar desde uno dejaría el otro en pie y el canal seguiría abierto. El cierre es **idempotente** y no entra en pánico si el candado quedó envenenado.

**El reparto se cierra durante el apagado ordenado, y antes de que el servidor empiece a drenar clientes.** Sin eso el apagado no termina nunca: el cierre del servidor no resuelve hasta que todas las conexiones lo hacen, y el cuerpo de un `text/event-stream` no tiene final propio —su única salida es el cierre del canal—. Medido con una espera de parada de 5 s: sin cliente el proceso muere en 2 s; con un cliente conectado seguía vivo pasados 20 s, y lo mataba el `SIGKILL` del período de gracia con el hilo de trabajo a mitad de archivar. El arreglo **no** añade una vía de terminación nueva: cierra el canal, que es la que ya existía.

**El límite que esto no cubre, y que hay que declarar**: un cliente que además **deje de leer** —el socket lleno, el portátil cerrado— puede retrasar el cierre, porque su búfer de escritura no se puede vaciar y esa conexión no termina. Eso no está en manos del hub: lo que sí lo está es cerrar el canal, y es lo que se hace. Un cliente **conectado al stream que lee** deja de recibir y su conexión termina, y el proceso acaba dentro del plazo.

## Las tres rutas nuevas

| Ruta | Códigos | Qué hace |
|---|---|---|
| `POST /api/v1/jobs/:id/run` | `202` / `409` / `404` / `400` / `503` | encola y contesta con la ejecución nacida |
| `GET /api/v1/events` | `200` `text/event-stream` | el stream global, la única respuesta sin envoltura |
| `GET /api/v1/snapshots?repository_id=<id>` | `200` / `400` / `404` | el explorador, leído **del repositorio** |

`202` y no `201`: la ejecución **no está hecha**, está aceptada. `409` y no un segundo `202`: encolar dos veces la misma tarea no acelera nada, la pone a esperar a la primera y deja al panel con dos barras para un solo trabajo. Y una tarea con `enabled = 0` **sí** se dispara a mano: `enabled` gobierna al planificador, que todavía no existe. Las tres cuelgan de la mitad protegida y **la lista blanca no crece**: son superficie de negocio —el disparo escribe, y el stream y el explorador publican el estado del despliegue—. La excepción del SSE es de **cuerpo** y no de sitio: la envoltura en este repositorio no es una capa del router sino lo que produce un manejador, así que el stream se la salta por devolver un `Sse`, y **no alcanza a los errores**: un `401` antes de abrir el stream sí lleva la envoltura.

### El explorador de instantáneas, y el extractor propio

La lista sale **del repositorio** y no del histórico de ejecuciones, y esa es la mitad que lo hace útil: la base sabe qué copias se intentaron y el almacenamiento sabe qué copias están —una instantánea que no está en el histórico es una copia hecha con otra herramienta, y una ejecución sin instantánea es una copia que se dio por buena y no está—. Deduplicar la segunda lista de la primera escondería ese caso.

La ventana es de **200** —la misma que la del histórico, y por el mismo motivo: listar es la lectura más cara de la API— y la respuesta lleva `truncated`. La ventana se aplica a los **identificadores antes de resolverlos**, que es lo único que evita recorrer el repositorio entero, y el precio hay que saberlo: el identificador de una instantánea es el nombre de un fichero y **no lleva fecha**, así que el subconjunto devuelto **no** tiene por qué ser el de las más recientes. `truncated` significa «el listado se ha recortado» y **no** promete antigüedad; lo que sí se garantiza es que lo devuelto viene **ordenado de la más reciente a la más antigua**.

Los parámetros de consulta pasan por el `Query` propio de `src/api/extract.rs`, que no publica el texto del intérprete: el rechazo por defecto de `axum` describe el fallo **con el texto del intérprete, que ecoa el valor del campo**, y esa es la fuga que `api-endpoints` cerró. El explorador añade su grano: `repository_id` es un `Option` y **no** un campo obligatorio, porque con el campo obligatorio el rechazo lo produce el extractor y su motivo no dice **cuál** falta; mirándolo a mano, el `400` puede **nombrar el parámetro**. Y `deny_unknown_fields`, para que un `?repositori_id=` no se ignore en silencio. Ni el nombre ni el valor del campo que sobra viajan al cliente.

## La puesta en marcha de un repositorio y sus tres desenlaces

El módulo de Rustic recibe los dos secretos **ya descifrados** por la capa de acceso y **no** conoce la clave maestra: tenerla sería el segundo sitio del programa capaz de descifrar credenciales, que es justo lo que el cifrado en reposo quiere evitar. El repositorio se abre **una vez por ejecución**: reutilizarlo entre dos copias desactiva la deduplicación por completo —el índice en memoria no ve el árbol que la copia anterior acaba de escribir—, así que un repositorio de larga vida no ahorra trabajo, lo esconde.

Ponerlo en marcha tiene **tres** desenlaces, y se distinguen **preguntándole al destino**, no leyendo el texto de un error: `open()` señala «no hay fichero de configuración» con el **mismo** tipo de error con el que `init()` señala «ya hay uno», así que por tipo es imposible y por texto sería atarse a una cadena de una dependencia. Se conserva el backend y se le pregunta por los ficheros de configuración: **ninguno** ⇒ se inicializa, **con ficheros** ⇒ se abre, **error** ⇒ el destino no se alcanza. Los tres no se arreglan igual —«no existe, lo he creado», «existe y he conectado» y «existe y no abre, revisa la passphrase»—.

`POST /api/v1/repositories` **habla con el destino antes de escribir la fila** y dice cuál de los dos ocurrió en `data.outcome` (`initialized` o `connected`). Si el destino no se alcanza o la passphrase no abre un repositorio que ya existe, **no queda fila**: es más barato no escribir que limpiar, y una fila que describe un destino inalcanzable hace que el panel lo liste como disponible y que el fallo aparezca en la primera copia, cuando ya nadie lo relaciona con el alta. `PUT` **no** conecta ni inicializa: solo el alta habla con el destino. Y el código lo elige la culpa: la credencial que no abre un repositorio existente es un `400` —el formulario se puede corregir—, un campo vacío es un `400` que **nombra el campo**, y lo demás es el `502` de «el repositorio no ha respondido».

## El arranque y la parada

El orden es **base abierta y migrada → motor montado → listener**, y no es decorativo: los trabajadores escriben en `job_executions` desde sus propios hilos, así que necesitan las capas construidas sobre el esquema ya aplicado. Un socket del motor de contenedores **inalcanzable no impide arrancar**: el arranque lo comprueba **para avisar** y el fallo aparece al ejecutar una copia. Es deliberado —el panel tiene que poder levantarse precisamente para enseñar que el socket no responde, y un proceso que se niega a arrancar deja al operador sin la herramienta con la que diagnosticarlo—. La parada, con `SIGTERM` o `SIGINT`, deja de aceptar conexiones, **cierra el reparto de eventos**, espera de forma **acotada** a los trabajadores y los une. La espera por defecto es de **5 s** —el período de gracia de un `docker stop` son 10, y la espera tiene que caber dentro con margen— y es **una** para todos los trabajadores, no una por cabeza; lo que no termine dentro del plazo se **registra** y el proceso acaba igualmente. La parada del motor va **después** del servidor, y va en un hilo de bloqueo porque es una espera síncrona: llamarla en el hilo del runtime dejaría paradas las tareas que ese hilo tuviera por delante. El proceso sale con código `0`.

## La verificación

### Las cuatro puertas

`just check` —`cargo fmt --all -- --check` → `cargo clippy --all-targets -- -D warnings` → `cargo test --locked`— en verde, con **0 fallos**, y `just doc` (`RUSTDOCFLAGS="-D warnings"`) limpio. `just check-all` **no** se puede ejecutar y no es un fallo: el job del panel se salta solo mientras no exista `ui/`.

Reparto de las **405** declaradas —**401** en el binario y **4** de integración—, medido fichero a fichero: **295** viven en los **30** ficheros que este change crea o toca, y **110** en **doce** que no toca.

| Fichero | Pruebas | Fichero | Pruebas |
|---|---|---|---|
| `src/engine/extraccion.rs` | 34 | `src/engine/mod.rs` | 27 |
| `src/rustic/repositorio.rs` | 24 | `src/rustic/origen.rs` | 23 |
| `src/api/repositories.rs` | 19 | `src/docker/cliente.rs` | 17 |
| `src/api/jobs.rs` | 16 | `src/config.rs` | 15 |
| `src/api/mod.rs` | 13 | `src/db/entities/execution.rs` | 11 |
| `src/api/targets.rs` | 11 | `src/error.rs` | 9 |
| `src/engine/hub.rs` | 8 | `src/api/executions.rs` | 8 |
| `src/docker/mod.rs` | 7 | `src/rustic/retencion.rs` | 6 |
| `src/main.rs` | 6 | `src/state.rs` | 5 |
| `src/api/snapshots.rs` | 5 | `src/api/secretos.rs` | 5 |
| `src/api/particion.rs` | 5 | `tests/arranque.rs` | 4 |
| `src/api/health.rs` | 4 | `src/api/events.rs` | 4 |
| `src/rustic/progreso.rs` | 3 | `src/engine/ejecucion.rs` | 3 |
| `src/engine/cola.rs` | 2 | `src/test_support.rs` | 1 |
| `src/rustic/mod.rs` | 0 | `src/rustic/error.rs` | 0 |

De las 401 del binario, **20** van `ignored` —las de contenedor, con su motivo escrito en cada `#[ignore]`— y **381** se ejecutan; las veinte están verdes contra el Podman real con `cargo test -- --ignored`. Y las **110** que quedan viven en **doce** ficheros que este change **no** toca —el mayor, `src/auth/oidc.rs` con **24**—: `src/db/entities/repository.rs` 15, `src/db/crypto.rs` 14, `src/db/mod.rs` 12, `src/api/auth.rs` 12, `src/auth/session.rs` 9, `src/db/migrate.rs` 7, `src/db/entities/target.rs` 6, `src/secret.rs` 3, `src/db/entities/job.rs` 3, `src/auth/middleware.rs` 3 y `src/auth/mod.rs` 2. **295 + 110 = 405** ✓.

### Las ocho comprobaciones de operador

Contra el binario de verdad, con el socket de Podman, un contenedor de base de datos y un S3 de pruebas, y con `sqlite3` sobre el fichero:

| # | Qué se comprobó | Lo medido |
|---|---|---|
| 4.1 | Arranque y parada ordenada | `/api/v1/health` → `200` con `auth_mode: "disabled"`; parada ordenada en **~500 ms** con **las dos marcas** del registro (`señal de cierre recibida` y `motor parado`); migraciones embebidas `init` e `indexes` aplicadas |
| 4.2 | Runtime de contenedores caído | Arranca con un `DOCKER_HOST` muerto, **avisa** al arrancar, y el fallo aparece **en el momento de copiar**: la ejecución queda `failed` con motivo y `finished_at` |
| 4.3 | Copia real de MariaDB | Copia de MariaDB de verdad en **15 s**; con `curl -N` se ven los **siete campos** del §5 recorriendo `pending → running → success`; la instantánea aparece en el listado (`files: 1`, `bytes: 2147`, `paths: ["target-operador-43.sql"]`) |
| 4.4 | Copia de directorio | `percentage` **no nulo** (sube de 0.0023 a 100) y **cero líneas de contenedor** en el registro |
| 4.5 | Consistencia de las ejecuciones | Tres filas, **dos `success` y una `failed`**, **cero `running`** y **cero filas terminales sin `finished_at`** |
| 4.6 | Sin ficheros temporales | Un volcado de **131 464 487 bytes** copiado con **cero ficheros regulares abiertos** en cinco muestreos; sin ficheros nuevos en `/tmp`, `/var/tmp` ni `/dev/shm` |
| 4.7 | Higiene de procesos | `pkill -x kesto` → **0 procesos** |
| 4.8 | Despacho bajo carga | **12 de 12**, de 9 a 10 s cada una, con el despacho contestando siempre **`202`** |

La 4.3 es la que cierra el círculo: la secuencia de eventos que se leyó por el stream trae los siete campos, con `bytes_processed` y `bytes_uploaded` avanzando juntos —los dos cuentan lo leído del `exec`— y `percentage` nulo, que es lo que le corresponde a un volcado de flujo. La 4.4 es su mitad complementaria: en un directorio el porcentaje **sí** existe y en el registro **no aparece una sola línea** del motor de contenedores, que es la prueba de que ese camino no pasa por Docker. Y una comprobación de la capa de repositorios, también en vivo: **los dos desenlaces de `POST /api/v1/repositories`**, `initialized` contra un cubo vacío y `connected` contra un repositorio que ya existía, con la respuesta diciendo cuál de los dos ocurrió.

### Cómo se repite

El servidor S3 de pruebas es **SeaweedFS**, y no es una preferencia: `rclone serve s3` **no sirve** —mapea los marcadores de directorio a ficheros de 0 bytes y contesta `500` a lo que el cliente necesita— y MinIO ya no es descargable sin credenciales. Contra SeaweedFS los tres desenlaces del repositorio y la copia entera funcionan. El resto del montaje: el socket de Podman en `DOCKER_HOST`, un contenedor de base de datos con su cliente de volcado dentro, un `DATABASE_URL` propio —el defecto del §8 apunta al volumen del `compose` y en una máquina de desarrollo ese directorio no existe—, `KESTO_MASTER_KEY` siempre, y ninguna de las tres `OIDC_*` para quedarse en modo de desarrollo. La parada se mide con `SIGTERM` y el reloj, y el stream con `curl -N`.

## Las auditorías

Cuatro módulos auditados por revisores **independientes** y de **solo lectura**, cada uno antes de empezar el siguiente, y **cada hallazgo cerró con su prueba**.

### Módulos 1 y 2 — 2 hallazgos en el transporte, 7 en el puente y 10 en el repositorio

- **La contraseña salía impresa en claro.** `bollard` registra a `DEBUG` el cuerpo serializado de la petición, con el `env` del `exec` dentro, y la directiva que lo cortaba vivía **solo en las recetas del `justfile`**: un `RUST_LOG=debug`, que es una forma perfectamente normal de arrancar, publicaba `MYSQL_PWD`. Ahora la directiva la compone **el proceso**, detrás de lo que pida quien arranca, y se avisa cuando el filtro pide el motor más hablador que eso.
- **El `timeout` de la biblioteca no acotaba el drenaje**: se resuelve al llegar las cabeceras, así que un volcado colgado dejaba la ejecución en `running` y el trabajador de su repositorio ocupado para siempre. De ahí la acotación por **inactividad**.
- **Una promesa falsa en el puente**: un comentario afirmaba que el error del lector acaba en `failed`, y medido en el archivador de la biblioteca **no** —filtra el error con un aviso y sigue—. Para poder comprobarlo, la señal de la entrega completa se hizo alcanzable desde el motor. Y el progreso llegaba mezclado: un contador de la biblioteca avanza 1 por instantánea, así que `bytes_uploaded` habría salido falso sin delator.
- **El más serio era de pérdida de datos, y no estaba en el código sino en su ausencia**: con las tres columnas de retención a cero, las reglas son válidas para la biblioteca y **no conservan nada**, así que se retiraba y se purgaba todo, incluida la copia recién hecha. Además, el camino de S3 —que es donde entran los secretos— era el único sin prueba, y una errata medida en vivo lo confirma: `OpenDALBackend::new("opendal:s3")` **no funciona** —la ruta se parte por el primer `:` y le pide a `opendal` un servicio llamado «opendal»—; el esquema es `"s3"`.

### Módulo 3 — 6 hallazgos en la extracción, y 2 altos en el motor

Los dos altos los reprodujo el auditor **por mutación**, y el primero es una trampa que el CI no habría cazado: con el backend de S3, llamar a Rustic desde dentro de un runtime de Tokio **entra en pánico**, y la restricción estaba escrita solo en los ficheros del motor —que no son los que iban a listar instantáneas ni a poner en marcha un repositorio en el alta—. Se cerró midiendo: `spawn_blocking` es seguro para Rustic —el hilo no está despachando tareas— y `block_in_place` **sí** entra en pánico sobre un runtime `current_thread`, que es el que monta `#[tokio::test]` por defecto. El segundo dejaba una ejecución **mintiendo para siempre**: si fallaba marcar la fila en curso se registraba y se volvía sin escribir nada, así que la fila se quedaba en `pending` —y la guarda del `409` cuenta como en curso toda fila `pending` o `running`—, el disparo de esa tarea pasaba a dar `409` para siempre y el panel la pintaba «en cola» indefinidamente.

### Módulo 4 — 1 bloqueante, y 6 más en la auditoría final

El bloqueante: **con un cliente conectado al stream, el cierre ordenado no llegaba nunca**, y lo mataba el `SIGKILL` del período de gracia con el hilo de trabajo a mitad de archivar. El arreglo es el del apartado del reparto —el emisor único y el cierre dentro del futuro de apagado—, y lleva **dos** pruebas y no una: las dos unitarias caen si se sustituye el `Arc` por un emisor por clon, y la de integración del binario **sigue verde** porque su escenario no tiene copia en vuelo. De los seis restantes, tres se cerraron **rompiendo lo que vigilan y revirtiendo**: el orden del explorador —lo único que el contrato promete sobre un subconjunto que admite no ser el de las más recientes— no tenía prueba, y invertir la ordenación dejaba las cinco en verde; `truncated: false` no se probaba nunca, y un `true` cableado pasaba la suite entera; y el evento de los dos clientes no estaba atado a su ejecución, así que un `status` inventado pasaba. Los otros tres: un rustdoc que prometía una aserción inexistente, un test con un nombre que hablaba de S3 donde el fichero entero dice §3, y `DOCKER_HOST`, que filtraba el vacío exacto pero **no recortaba** al revés que su hermana `RUST_LOG` —`DOCKER_HOST=" "` viajaba como ruta de socket—.

### Una carrera del arnés, medida y no supuesta

La prueba del ciclo de vida afirmaba `pending` sobre la tabla justo detrás del encolado, y ahí el trabajador es **otro hilo**: marca `running` en cuanto la ejecución entra en su buzón. La prueba perdía **2 corridas de 8**, y con el protocolo de carga que reproducía el fallo —el test en solitario y 16 bucles de CPU en paralelo— **4 de 40** antes y **0 de 40** después. No se tocó una línea de producción: el motor hacía lo que tenía que hacer y el que mentía era el arnés.

### La carrera que cazó la CI

La prueba de integración `un_sigterm_acaba_con_el_proceso_aunque_haya_un_cliente_en_el_stream` —la que ejercita el cierre ordenado con un cliente conectado al stream— pasó **cuatro veces** en esta máquina de 16 CPUs y **cayó en la CI**, que corre con dos:

```
test un_sigterm_acaba_con_el_proceso_aunque_haya_un_cliente_en_el_stream ... FAILED
panicked at tests/arranque.rs:840:33:
no se pudo abrir la conexión con 127.0.0.1:38153: Connection refused (os error 111)
test result: FAILED. 3 passed; 1 failed; ... finished in 0.12s
```

`arrancar` —el arnés— espera la marca `ARRANCADO` del registro, y esa marca la emite `main` **una línea antes** de que `serve` ate el puerto: lo dice el rustdoc de `esperar_la_salud` en el propio `tests/arranque.rs`, y es lo que razona la prueba hermana. La prueba nueva se justificaba diciendo que un `200` con `text/event-stream` demuestra que el servidor atiende, y eso es verdad **una vez conectado**: lo que caía era el `connect`, que el ayudante hace **una sola vez y sin plazo**. Murió a los **0,12 s**, no a los 10 s del plazo de la parada.

El arreglo (commit `22ad31a`) es esperar a la **boca** del servidor con `esperar_la_salud` antes de abrir el stream, como ya hacía la prueba hermana. Y lo que cierra la ventana **no** es un margen de tiempo —que es justo lo que fallaba, porque no se puede dimensionar para toda máquina— sino una **precondición establecida**: una vez que una petición HTTP ha respondido, el listener está atado y lo sigue estando hasta el apagado. La respuesta de la salud **no** se afirma: lo que la prueba necesita de esa llamada es que el servidor escuche, y el `200` con `text/event-stream` lo afirma ya el propio stream.

Y lo que la resolvió fue **reproducirla, no volver a correrla**: se midió en los dos sentidos y **con el mismo binario mutado**, con **500 ms de sueño** inyectados al principio de `serve` —después de la marca y antes de atar el puerto— para ensanchar de microsegundos a medio segundo una ventana que en 16 CPUs es inobservable. Con esa mutación puesta, la prueba **sin** el arreglo cae con el mismo mensaje y la misma línea que la CI (**0,04 s**), y **con** el arreglo pasa tardando **0,56 s** —tolera de verdad un hijo que ata tarde—. La mutación quedó revertida y demostrada (`git diff -- src/main.rs` = 0 líneas). De ahí lo que queda escrito, sin más: **un verde en local no era prueba de nada sobre esa prueba**.

## Deuda pendiente y límites aceptados

- **O-1: `GET /api/v1/snapshots` puede crear el repositorio** en el destino —un `GET` con efecto de escritura—. Se **declara** en el rustdoc de la ruta y no se arregla aquí: distinguir «abre» de «crea» pide una variante de la apertura que no inicialice, y eso es reabrir un `src/rustic` ya auditado. El efecto ocurriría igual —el alta y la primera copia crean ese repositorio—, así que lo que no puede es ocurrir **en silencio**.
- **El alta contra un S3 inalcanzable no está acotada**: tarda **~50 s**, los cinco reintentos de la biblioteca. No se le pone plazo porque un plazo no distingue muerto de lento y no cancela el trabajo que ya corre con los dos secretos dentro.
- **`INACTIVIDAD_MAXIMA` no es inyectable**: es una constante del módulo y no entra por los ajustes del motor, así que no se puede acortar en una prueba sin falsear el reloj. El plazo del motor de contenedores y el de la parada **sí** son ajustes.
- **El arnés de integración deja del orden de 2 675 directorios `kesto-prueba-*`** en el directorio temporal, y crece con cada corrida de la suite: deuda de limpieza.
- **Un cliente que deja de leer puede retrasar la parada.** El motor cierra el canal —que es lo que está en su mano—; vaciar el búfer de escritura de ese cliente, no.
- **El `compose` del §8 copiado literal no arranca**: le falta `KESTO_MASTER_KEY`, que es obligatoria siempre. Y tal y como está —sin ninguna de las tres `OIDC_*`— arrancaría en modo `Disabled`, con la API **abierta** en el 8080: es modo de desarrollo, no un despliegue expuesto.
- **La semántica de retención de la biblioteca** conserva la más nueva y la más vieja del grupo mientras queden puntos del contador, así que doce copias del mismo día con `keep_daily = 7` dejan **dos** instantáneas. No es un defecto de Kesto, es la biblioteca, y está contada arriba con su derivación.
- **N-3**: con el reparto ya cerrado, una petición que llegue **durante** el apagado recibe `200` con cuerpo vacío en vez de un `503` con la envoltura. Se **declara** y no se cambia: es una rama casi inalcanzable —durante el apagado el servidor ya no acepta conexiones— y el cliente del stream reintenta igual.
- **N-4**: que el futuro de apagado cierre el reparto es una **invariante documentada, no tipada**. Se declara porque su riesgo **ya está cubierto por prueba**: si alguien deja de cerrarlo, cae el test de integración del binario.
- **`db_name` y §4.A**: la divergencia **está cerrada** por este change —los comandos honran la columna, ver la tabla de arriba—, así que lo que queda no es deuda sino consecuencia: un target con `db_name` informado y un cliente de volcado que no entienda la base nombrada fallará en la extracción, no en el esquema.
- **Un directorio no tiene señal de entrega**: la condición de éxito de esa copia es que el archivado devuelva `Ok`, y el archivado silencia con un aviso el fallo de lectura de un fichero concreto y sigue, así que una copia de directorio puede no llevar un fichero que se volvió ilegible a mitad. Para un volcado lo caza la señal del puente; para un directorio no hay señal equivalente.
- **La cifra del enunciado de la tarea 0.3 no se sostiene** y se declara en vez de taparse: pide «el árbol resuelto, **498 paquetes**» —la cuenta que dejó el *spike*— y el `Cargo.lock` entregado tiene **590** sobre los **302** de `development`. Lo que la tarea exige de verdad —las tres dependencias con sus versiones exactas, las features sin `rest` ni `rclone`, la promoción de `futures-util` con su nota, el `Cargo.lock` versionado y una sola copia de `rustic_core`— está y está verificado, así que la tarea va tildada: la cifra es del plan y no del entregable, y el plan no se toca porque registra lo que se planificó.
- **Seis decisiones se tomaron como estaban propuestas y nadie confirmó después**, y se documentan como están, sin reabrirlas: `current_action` **en español** frente al ejemplo en inglés del §5; `bytes_processed` y `bytes_uploaded` **no** cuentan los bytes que llegan al destino; `PUT /repositories/:id` **no** conecta ni inicializa; la ventana de `/snapshots` es **200**; **el CI no tiene contenedores**, así que todo lo que exige un runtime se verifica a mano; y el rechazo de una retención con los tres contadores a cero ocurre **en la API**, además de en el motor.
- **No hay cron, y es lo que más se nota**: `cron_expression` se guarda y se sirve y nadie la evalúa, así que hoy un trabajo solo corre cuando alguien llama a `run`. Llega con el planificador, junto a `src/scheduler`, `ui/` y el `Dockerfile`.

Este documento es el hermano de `plans/PLAN-005-COMPLETED.md` y continúa a `docs/FEATURE-004-api-endpoints.md`.
