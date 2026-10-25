# PLAN-005 — `backup-engine`

> **Estado:** en curso (change proposal aprobado el 2026-10-25; implementación TDD por arrancar).
> **Rama:** `feature/backup-engine`, desde `development`.
> **Change de OpenSpec:** `openspec/changes/backup-engine/` — **28 requisitos y 89 escenarios** (`engine` 9/27, `extraction` 6/19 y `repository` 5/17, las tres capabilities **nuevas**; `api-resources` 4/14, `config` 2/5 y `api-shell` 2/6, que crecen), más `design.md`. `openspec validate backup-engine --strict` válido. `tasks.md`: **76 tareas** (15 de preparación, 35 de RED, 5 de GREEN, 7 de REFACTOR, 8 de operador y 6 de cierre).
> **Especificación de origen:** `docs/DEFINITION.md` §2 (arquitectura de componentes), §4 (flujos de extracción y retención) y §5 (los tres endpoints que faltaban). §6, §7 y §8 se respetan sin entrar.
> **Cambio anterior:** `api-endpoints` (`PLAN-004-COMPLETED.md`, `docs/FEATURE-004-api-endpoints.md`, 214 tests, fusionado en `development` como `932747c`).

## Objetivo

Que Kesto **copie**. Hoy el orquestador guarda las cuatro tablas del §3, las sirve por HTTP con la superficie que levantó `api-endpoints`, y **nada escribe jamás una ejecución**: `job_executions` solo la tocan las pruebas de la capa de acceso. Al terminar, una tarea se dispara a mano, el volcado sale del contenedor de la base sin tocar disco, entra en un repositorio Rustic sobre S3, la retención se aplica, el panel lo sigue por SSE y los snapshots se exploran desde el repositorio.

De los cuatro módulos del §7 que siguen vacíos, este change llena **tres**: `docker`, `rustic` y `engine`.

## Alcance

**Dentro:**

1. **La extracción del volcado**: `docker exec` por `bollard` sobre el socket configurado, con `attach_stdout` y `attach_stderr` sin TTY, **credenciales por el entorno del `exec`** (`MYSQL_PWD` / `PGPASSWORD`) y el `stdout` reconducido a un lector en memoria. Sin ficheros temporales: ni un byte del volcado pasa por el disco.
2. **Los comandos del §4.A, corregidos**: `mariadb-dump --all-databases` / `pg_dumpall` pasan a respetar `db_name` —informado ⇒ esa sola base, `NULL` ⇒ volcado global—, que es lo que el §3 ya declara. Cierra el cabo suelto 3.
3. **Un target `directory` sin Docker**: se lee con el `LocalSource` de `rustic_core` desde el `host_path`, declarado como añadido al SRS porque el §4 solo fija los comandos de las dos bases.
4. **El trato con Rustic**: el backend S3, la construcción del repositorio desde la fila con sus secretos ya descifrados, la **discriminación de los tres desenlaces** de la puesta en marcha, `archive`, la retención en tres operaciones y el listado de snapshots.
5. **El motor**: una **cola y un worker secuencial por repositorio** (§2, por el bloqueo exclusivo del repositorio), el ciclo de vida de una ejecución en `job_executions`, el hub de eventos y el arranque y la parada acotada.
6. **Las tres rutas del §5 que faltaban**: `POST /api/v1/jobs/:id/run` (`202` / `409` / `404` / `400`), `GET /api/v1/events` (SSE global, la **única** respuesta sin envoltura) y `GET /api/v1/snapshots?repository_id=<id>` (ventana de 200).
7. **El contrato nuevo del alta de repositorio**: `POST /api/v1/repositories` conecta con el destino y, si no existe, lo **inicializa**; si el destino no se alcanza o la passphrase no abre, **no deja fila**.
8. **`DOCKER_HOST`** en la configuración, con la lectura inyectada como el resto de variables.

**Fuera:** `src/scheduler` y la evaluación de `cron_expression` (el change del planificador); `ui/` y `Dockerfile`; la restauración de snapshots; los `keep_*` que el §3 no guarda; la reconexión y el relleno del SSE; el `backup_targets.target_type = 'directory'` más allá de leerlo.

## Módulos

El change se reparte en **cuatro** módulos entregables, cada uno con su ciclo RED → GREEN → auditoría independiente → correcciones:

| # | Módulo | Contenido | Ficheros |
|---|---|---|---|
| 1 | El transporte de Docker | El cliente, la conexión por `DOCKER_HOST` con la versión negociada o fijada, `create_exec` / `start_exec` / `inspect_exec`, la captura del `stdout` y del `stderr` y el código de salida. No sabe nada de volcados. | `src/docker/{mod.rs,cliente.rs}` |
| 2 | Rustic y el puente | El backend S3 con `OpenDALBackend`, `RepositoryBackends`, `Repository::new_with_progress`, los tres desenlaces por `list(FileType::Config)`, `archive` con el `ReadSource` propio, la retención en tres operaciones, el listado por `SnapshotId` y el gancho de progreso. | `src/rustic/{mod.rs,repositorio.rs,origen.rs,retencion.rs,progreso.rs}` |
| 3 | El motor y la extracción | La cola y el worker por repositorio en hilos propios, el ciclo de vida en `job_executions`, el hub de eventos, los comandos por tipo de target, la elección de ruta —Docker o `LocalSource`—, el puente con el runtime y el arranque y la parada. | `src/engine/{mod.rs,cola.rs,ejecucion.rs,hub.rs,extraccion.rs}` |
| 4 | La capa HTTP y el estado | Las tres rutas, el SSE fuera de la envoltura, el alta del repositorio con su contrato nuevo, `DOCKER_HOST` en la configuración, el motor en el `AppState` y el orden del arranque y la parada. | `src/api/{mod.rs,jobs.rs,repositories.rs,events.rs,snapshots.rs}`, `src/config.rs`, `src/state.rs`, `src/main.rs`, `src/test_support.rs` |

El **orden de entrega** es 1 → 2 → 3 → 4, y es una cadena de dependencias, no una preferencia: el motor necesita extraer (1) y archivar (2) antes de poder orquestar (3), y las rutas del 4 no tienen nada que servir hasta que el 3 acepte trabajo. Ningún módulo se empieza sin los tests del anterior en verde.

## Decisiones tomadas

### El alcance es el motor, no el planificador

`docker` + `rustic` + `engine` y los tres endpoints del §5. Sin ninguna de las tres `OIDC_*` el modo es `Disabled` y no se monta capa, así que el disparo manual y el SSE son la única forma de ejercitar el motor sin proveedor de identidad. El planificador llega después y su módulo (`src/scheduler`) sigue vacío.

### Las credenciales van por el entorno del `exec`

`MYSQL_PWD` / `PGPASSWORD` en vez de `-p<contraseña>`. En un contenedor la línea de comandos es pública para cualquiera con acceso al listado de procesos, y el `-p<contraseña>` del §4.A dejaba la credencial donde más gente la mira. Es el cabo suelto 4 del SRS, que ya sugería esta salida.

### Un `ReadSource` propio en vez de un `AsyncRead`

El diagrama del §4.A dibuja un búfer `impl AsyncRead`; la API real de `rustic_core` 0.13 pide un trait **bloqueante**. El puente —hilo propio, `sync_channel` acotado, `Mutex<Option<Receiver<_>>>`— está justificado en `design.md`, y su viabilidad **está demostrada** por un *spike* de compilación en verde, no supuesta.

### El trabajo del motor va en hilos propios

El backend `opendal` de `rustic_backend` crea **su propio** runtime multi-hilo y llama a `runtime().enter()`, y `archive()` es bloqueante. Un `block_on` sobre el runtime que atiende las peticiones HTTP detiene el despacho y con él el resto de la aplicación. El acceso a la base desde esos hilos entra al runtime de forma explícita con un `Handle`.

### Un worker secuencial por repositorio, y no un worker global

§2 lo exige y el motivo está medido: `rustic_core` toma un bloqueo exclusivo del repositorio, así que dos copias contra el mismo repositorio se bloquean entre sí en vez de avanzar. Se serializa el **repositorio**, no la tarea: dos repositorios distintos avanzan a la vez.

### El hub es un `broadcast` acotado

Tiene tope, así que no crece; y descarta en vez de bloquear, así que un cliente lento pierde eventos y **no** frena al motor. Un hub que bloquea convierte a un cliente lento en un cliente que para las copias.

### Los tres desenlaces se distinguen preguntando al destino

`open()` e `init()` comparten `ErrorKind::Configuration` —«no hay configuración» frente a «ya hay una»—, así que por tipo es imposible y por texto sería atarse a una dependencia. Se conserva el `Arc<dyn WriteBackend>` de `RepositoryBackends::repository()` y se llama a `list(FileType::Config)`: vacío ⇒ inicializar, con ficheros ⇒ abrir, error ⇒ destino inalcanzable.

### La retención tiene tres operaciones, no dos

Los `keep_*` **no** son parámetros del `prune`: son reglas de retención de la ruta de `forget`, y `Repository` no tiene método `forget`. Decidir → retirar → purgar. Confundirlo escribe un motor que purga datos que nunca dejaron de estar referenciados.

### `DELETE` contesta `200` con `data` nulo, y las tres rutas nuevas no lo usan

De `api-endpoints`, sin cambios. Lo que sí cambia es que `POST /api/v1/repositories` **habla con el destino**: escribir una fila que describe un repositorio inalcanzable hace que el panel lo liste como disponible y que el error aparezca en la primera copia, cuando ya nadie lo relaciona con el alta.

### El snapshot graba su procedencia y no el `argv` de Kesto

`SnapshotFile::from_options` rellena el comando con `std::env::args_os()` cuando no se le informa, es decir con la línea de comandos de Kesto —y en un arranque con variables por argumento, con secretos dentro—. Se informa explícitamente.

### Solo tres `keep_*` tienen columna, y los que no la tienen se quedan al defecto de la biblioteca

No se añaden columnas al §3 en este change. Lo que el esquema no guarda, el motor no lo configura, y eso queda escrito en vez de implícito.

## Trampas encontradas y candadeadas

Las tres primeras las cazó el *spike* de compilación, **desmintiendo** lo que la lectura del código había dado por bueno:

| # | Trampa | Candado |
|---|---|---|
| 1 | `list::<SnapshotFile>()` **no compila**: `RepoId` exige `Copy` y `SnapshotFile` no lo es. | Listar por `SnapshotId` y resolver con `get_snapshots`; prueba del listado (1.29). |
| 2 | El `Display` de `SnapshotId` da **solo los 8 primeros hex**: prefijo ambiguo. | Pasar el hex completo; nota en la tarea 0.14. |
| 3 | Los `keep_*` **no** son de `prune` sino de `KeepOptions`, y `Repository` no tiene `forget`. | Requisito reescrito con las tres operaciones y escenario nuevo que impide purgar antes de retirar (1.14). |
| 4 | `SnapshotFile::from_options` cae a `std::env::args_os()` si `command` es `None`. | Prueba de que el snapshot no lleva el `argv` de Kesto (1.9 y 1.10). |
| 5 | El `stream` de `bollard` es asíncrono y `archive()` es bloqueante; el backend de opendal entra en su propio runtime. | Hilos propios + `sync_channel` acotado; prueba de que una ejecución lenta no bloquea la salud ni los listados (1.35). |
| 6 | La contraseña en el `cmd` del `exec` es pública en el contenedor. | Prueba de que no está en el `cmd` y sí en el entorno (1.2). |
| 7 | Un `stdout` vacío es ambiguo: fallo o base vacía. | Se lee el código de salida y el `stderr`; pruebas 1.7 y 1.8. |
| 8 | `foreign_keys` es **por conexión** (de `db-schema`). | Ya fijado en las opciones del pool, que este change reutiliza sin abrir un segundo. |

## Verificación

### Las cuatro puertas, en su orden

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test --locked` — con el `Cargo.lock` nuevo, que es lo que este paso comprueba de verdad
4. `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` (`just doc`)

Encadenadas en `just check` (las tres primeras) y `just check-all`. `just check-all` **no** puede pasar mientras no exista `ui/`, y eso no es un fallo.

### La auditoría

Cada módulo pasa por `@rust-reviewer` al terminar su GREEN, antes de empezar el siguiente. Lo que marque vuelve al mismo módulo con los registros.

### La verificación del operador (8 tareas)

Arranque real con el socket de Podman; arranque con un socket muerto; una copia de verdad contra un contenedor de base de datos con `curl -N` siguiendo el stream; una copia de directorio; el ciclo de vida comprobado con `sqlite3` después de una copia buena y una fallida; la comprobación de que el volcado no pasa por disco; y **doce ciclos seguidos** para descartar el fallo intermitente, que es la clase de fallo que este change más puede introducir.

## Riesgos

- **El `Cargo.lock` es un entregable.** Primer change con dependencias pesadas y el CI corre `--locked`. Se resuelve una vez con red y se commitea; sin él, el `preflight` cae.
- **La negociación de versión de la API de Docker con Podman.** Puede atascarse; si se atasca, se fija explícitamente en vez de dejar que la deduzca.
- **El CI no tiene motor de contenedores.** Las pruebas van por capas para que el subconjunto del CI no lo necesite, y lo que quede sin cubrir se declara por escrito en `docs/FEATURE-005-backup-engine.md`.
- **La parada ordenada.** Con una copia de minutos en vuelo, la espera es acotada y lo que no termine se registra; un `SIGKILL` del orquestador sigue siendo posible y es preferible a una espera indefinida.

## Cierre

`openspec validate backup-engine --strict` → archivar con `just spec-archive backup-engine` → `docs/FEATURE-005-backup-engine.md` con lo que **realmente** quedó construido → `plans/PLAN-005-COMPLETED.md` con `git mv` → `AGENTS.md` y `README.md` con el estado nuevo → PR de `feature/backup-engine` a `development` y **esperar** al CI en la cabeza nueva antes de fusionar. **No** se abre PR a `main` y **no** se etiqueta.
