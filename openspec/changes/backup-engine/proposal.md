# Proposal: backup-engine

## Why

`docs/DEFINITION.md` §4 describe el flujo que da sentido al proyecto —extraer el volcado del contenedor de una base, meterlo en un repositorio Rustic sobre S3 y purgar por retención— y **no existe ninguna pieza de él**. Kesto, hoy, es un orquestador de copias que no copia: guarda las cuatro tablas del §3, las sirve por HTTP con la superficie que levantó `api-endpoints`, y **nada escribe jamás una ejecución**. `job_executions` solo la tocan las pruebas de la capa de acceso; `POST /api/v1/jobs/:id/run` no existe; y los **tres** endpoints del §5 que faltan —`POST /api/v1/jobs/:id/run`, `GET /api/v1/events` y `GET /api/v1/snapshots?repository_id=<id>`— faltan precisamente porque dependen de lo que este change construye.

De los cuatro módulos del §7 que siguen vacíos —`docker`, `engine`, `rustic` y `scheduler`— este change llena **tres**: `docker`, `rustic` y `engine`. El cuarto, `scheduler`, corresponde al change del planificador y **no entra aquí**: sin él, una tarea se dispara a mano y no por su `cron_expression`, que es exactamente el alcance que se aprobó.

El change cierra además **dos deudas declaradas** del SRS, las dos por escrito, y estrena una de operación:

1. **Los comandos del §4.A no pueden cumplir lo que el §4.A pretende** (cabo suelto 3). `mariadb-dump --all-databases` y `pg_dumpall` vuelcan el servidor **entero** e ignoran `backup_targets.db_name`. El §3 ya declaró la semántica —`db_name` informado ⇒ esa sola base, `NULL` ⇒ volcado global—, así que lo que faltaba era que el motor cambiara los comandos para que la cumplieran. Ese es el trabajo de este change, y no una interpretación nueva: la deuda la dejó apuntada el propio SRS.
2. **Las credenciales iban por la línea de comandos** (cabo suelto 4). `-p<db_password>` es visible en el listado de procesos del contenedor de la base, que es justo el sitio donde menos conviene. Este change las manda por el **entorno del `exec`** (`MYSQL_PWD` / `PGPASSWORD`), que es el mecanismo que el propio SRS sugería como alternativa.
3. **El `Cargo.lock` deja de resolverse solo.** Es el **primer change con dependencias pesadas** —`rustic_core` 0.13.0, `rustic_backend` 0.7.0 y `bollard` 0.18.1— y el CI corre `cargo test --locked`, así que el árbol hay que resolverlo **una vez con red** y commitear el `Cargo.lock` resultante. No es un efecto colateral: es un entregable, y sin él el CI se cae en el `preflight`. Se resuelve además **una cuarta** que no es nueva en el árbol sino una promoción: drenar el `stream` del `exec` de `bollard` obliga a leer un `Stream`, y `futures-util` —o `tokio-stream`— ya está en el `Cargo.lock` como transitiva (498 paquetes), así que pasa a declararse en vez de aparecer por debajo.

Y hay un desajuste entre el SRS y la biblioteca que hay que **declarar** en vez de disimular. El diagrama del §4.A dibuja la ingesta como un búfer en memoria que `rustic_core` consume como `impl AsyncRead`. La API real de `rustic_core` 0.13 **no acepta un `AsyncRead`**: `Repository::archive()` recibe un `ReadSource`, que es un trait **bloqueante** (`fn open(self) -> RusticResult<Self::Reader>` con `Reader: Read + Send + 'static`). El puente entre el `stream` asíncrono de `bollard` y ese trait bloqueante es trabajo de este change, y su forma —un hilo propio de sistema operativo, un canal síncrono acotado y un lector que lo consume— se decide aquí porque hay una restricción dura: el backend `opendal` de `rustic_backend` **crea su propio runtime multi-hilo de Tokio** y llama a `runtime().enter()`, así que ejecutar `archive()` dentro de un worker de Tokio no es una opción, es un atasco.

## What Changes

**Tres capabilities nuevas.**

- **`extraction`** — sacar el volcado del contenedor. `docker exec` por `bollard` sobre el socket configurado, `attach_stdout` y `attach_stderr` sin TTY, credenciales por el **entorno del `exec`**, y el `stdout` reconducido a un lector en memoria que se le entrega a Rustic. Sin ficheros temporales y sin tocar el disco: ni un byte del volcado se escribe en ningún sitio que no sea el repositorio. El comando depende del tipo de target y `db_name` decide el alcance, con los comandos corregidos de la deuda 3. Un target de tipo `directory` **no pasa por Docker**.
- **`repository`** — hablar con Rustic. Construir el repositorio desde la fila de `repositories`, con sus tres secretos descifrados; **conectar** si ya existe e **inicializar** si no, distinguiendo los dos casos sin leer mensajes de error; archivar el volcado con la procedencia grabada en el snapshot; aplicar la retención en sus tres operaciones —decidir qué snapshots se retiran según los tres `keep_*` del §3, retirarlos y reclamar los datos huérfanos con un plan de purga calculado antes de ejecutarlo—; y listar los snapshots del repositorio.
- **`engine`** — orquestar. Una **cola y un worker secuencial por repositorio** (§2 lo exige y el motivo es un lock exclusivo del repositorio en `rustic_core`: paralelizar dos copias contra el mismo repositorio las bloquea entre sí), el ciclo de vida de una ejecución escrito en `job_executions`, el **hub de eventos** que alimenta el SSE, y el arranque y la parada ordenada de todo ello.

**Tres capabilities que se modifican.**

- **`api-resources`** — entran las **tres rutas del §5** que faltaban: `POST /api/v1/jobs/:id/run`, `GET /api/v1/events` y `GET /api/v1/snapshots?repository_id=<id>`. Y `POST /api/v1/repositories` gana su contrato de verdad: el §5 dice «Inicializar o conectar repositorio S3/Rustic», y hasta ahora el alta se limitaba a escribir la fila. A partir de este change, un alta **conecta** con el repositorio y, si no existe, lo **inicializa**.
- **`config`** — entra `DOCKER_HOST`, que es la variable que `bollard` lee para saber a qué socket apuntar. En producción el socket está en la ruta de siempre; en desarrollo, la máquina de este entorno no tiene Docker, tiene Podman con una API compatible en otro socket, y esa ruta se configura sin tocar el binario.
- **`api-shell`** — entra la excepción que el SSE obliga a declarar: `GET /api/v1/events` es la **única** respuesta de la API que **no** viaja con la envoltura JSON. Un `text/event-stream` con envoltura no es un `text/event-stream`.

## Decisiones tomadas al abrir el change

1. **Alcance: el motor sin el planificador.** `docker` + `rustic` + `engine` y los tres endpoints. `src/scheduler` queda fuera, con el change del planificador.
2. **Las credenciales del volcado viajan por el entorno del `exec`**, no por la línea de comandos (`MYSQL_PWD` / `PGPASSWORD`). Cierra el cabo suelto 4.
3. **`GET /api/v1/snapshots?repository_id=<id>` entra en este change**, porque es Rustic quien los lista y Rustic llega aquí. No tiene sentido dejarlo para después.
4. **`POST /api/v1/repositories` conecta y, si el repositorio no existe, lo inicializa.** El §5 ya lo decía —«Inicializar o conectar»— y el change lo convierte en contrato.
5. **`host_path` es la ruta tal como la ve Kesto dentro de su propio contenedor** (por ejemplo `/backup_sources/var_www`), no la ruta del host. El §4 no dice qué significa y las dos lecturas son defendibles, pero solo una tiene los datos: para usar la ruta del host, Kesto tendría que conocer un mapeo host→contenedor que nadie le ha dado. Montar el directorio del host en el contenedor de Kesto es trabajo del `compose` del §8, que ya monta `/backup_sources/...` en solo lectura.
6. **Un target `directory` no pasa por Docker.** El §4 solo fija los comandos de las dos bases; para un directorio no hay proceso que ejecutar: se lee con el `LocalSource` de `rustic_core` desde la ruta del punto 5. Se declara como **añadido al SRS**, porque el §4 no describe ese flujo.
7. **`percentage` es nulo cuando el total no se conoce.** Medido: `StdinSource::size()` devuelve `Ok(None)` y `LocalSource::size()` devuelve `Ok(Some(total))`. Es decir, para un volcado de base **no hay denominador** —el `exec` no anuncia cuánto va a pesar el dump— y para un directorio sí. Inventar un total daría un porcentaje falso; anular el campo obliga al panel a pintar «subiendo…» sin barra, que es lo honesto.
8. **`POST /api/v1/jobs/:id/run` contesta `202`** con la ejecución recién creada en estado `pending`, y **`409`** si la tarea ya tiene una ejecución `pending` o `running`. Una tarea con `enabled = 0` **sí** se puede disparar a mano: `enabled` gobierna el planificador, no el botón.
9. **Solo tres de las once familias `keep_*` de Rustic tienen columna en el §3** (`keep_daily`, `keep_weekly`, `keep_monthly`). El resto se queda con el valor por defecto de la biblioteca, y eso se declara en vez de dejarlo implícito. Y los tres que sí la tienen **no** son parámetros del `prune`: son reglas de retención, que deciden qué snapshots se retiran; el `prune` es lo que después reclama el espacio que esos snapshots ocupaban.
10. **`status` viaja en inglés y `current_action` en español.** Los valores de `status` (`pending`, `running`, `success`, `failed`) los fija el `CHECK` del §3 y no son texto de pantalla; `current_action` sí lo es, y el panel es español.

## Non-goals

- **El planificador.** `src/scheduler` sigue vacío: `cron_expression` se guarda y se sirve, pero nada lo evalúa. Un job no se dispara solo en este change.
- **El `Dockerfile` y la UI.** `ui/` y `Dockerfile` siguen sin existir. Las dos guardas del pipeline siguen haciendo su trabajo.
- **La restauración.** Nada devuelve los datos a su sitio. `GET /snapshots` **lista** snapshots; no abre uno, no enumera su contenido y no restaura nada.
- **El `backup_targets.target_type = 'directory'` más allá de leerlo.** Se copia el directorio, y ya.
- **Los `keep_*` que no tienen columna.** No se añaden columnas al §3 en este change: lo que el esquema no guarda, el motor no lo configura.
- **La reconexión y el relleno del SSE.** Un cliente que se conecta tarde no recibe lo que se perdió; para eso está `GET /api/v1/executions`.

## Impact

- **Rutas nuevas:** `POST /api/v1/jobs/:id/run`, `GET /api/v1/events`, `GET /api/v1/snapshots`.
- **Contrato modificado:** `POST /api/v1/repositories` pasa de escribir una fila a escribir una fila **y** conectar o inicializar el repositorio.
- **Módulos nuevos:** `src/docker`, `src/rustic`, `src/engine` (los tres del §7 que el SRS contemplaba y que hasta ahora estaban vacíos). Dentro de `src/engine` vivirá también la extracción, porque la extracción existe para alimentar al motor; si al implementarla pide su propio fichero —por tamaño, no por gusto— se separará dentro del mismo módulo.
- **Ficheros tocados:** `Cargo.toml` y `Cargo.lock` (tres dependencias nuevas y su árbol, más la promoción de `futures-util` o `tokio-stream`, que ya estaban dentro como transitivas), `src/main.rs` (arranque y parada del motor), `src/state.rs` (el motor en el estado), `src/api/mod.rs` (las tres rutas y la excepción del SSE), `src/config.rs` (`DOCKER_HOST`), `src/test_support.rs` (el arnés del motor).
- **Especificaciones tocadas:** tres nuevas (`engine`, `extraction`, `repository`) y tres deltas (`api-resources`, `config`, `api-shell`).
- **Riesgo principal, ya cerrado:** que un `ReadSource` propio **no** fuera implementable desde fuera del crate, porque `rustic_core` declara `pub(crate) mod backend`. Un *spike* de compilación lo ha desmentido: un tipo propio implementa el trait, se pasa a `archive()` y el crate compila en verde con las tres dependencias dentro. Y el spike ha cazado **dos errores de este mismo change que la lectura del código no había visto**, los dos ya corregidos en la spec y en las tareas: el listado de snapshots **no** se puede pedir por `SnapshotFile` —no implementa el trait que `list` exige, porque no es `Copy`— sino por identificador; y los tres `keep_*` **no** son parámetros del `prune` sino reglas de retención, así que la retención tiene tres operaciones —decidir, retirar y purgar— y no dos.
- **La parada ordenada cambia de forma:** hoy espera a las peticiones en curso; con el motor, tiene además que dejar de aceptar trabajo, esperar a la ejecución en vuelo y unir los hilos. Es una ejecución que puede durar minutos, así que la espera es acotada y lo que se pase de ahí se registra en vez de esperarse indefinidamente.
