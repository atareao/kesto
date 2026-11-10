# PLAN-006 — `scheduler`

> **Estado:** en curso — change **implementado y verde**: fase 0 medida, fase 1 (RED) cerrada con 25 pruebas rojas y fase 2 (GREEN) con **407 pruebas en el binario y 5 en `tests/arranque.rs`, 0 fallos**; REFACTOR 3.1 y 3.2 hechos. Faltan la verificación del operador (4.1 y 4.2) y el cierre (5.1 y 5.2).
> **Rama:** `feature/scheduler`, desde `development`.
> **Change de OpenSpec:** `openspec/changes/scheduler/` — una capability **nueva**, `scheduler`, y un delta de `api-resources`.
> **Especificación de origen:** `docs/DEFINITION.md` §2 (arquitectura de componentes; «**Planificador:** `tokio-cron-scheduler`»), §3 (`backup_jobs.cron_expression` y `backup_jobs.enabled`), §5 (rutas), §6 (el generador de cron del panel) y §7 (la estructura del repo, donde `src/scheduler` es el único módulo que sigue vacío).

## Objetivo

Que una tarea programada se dispare **sola** por su `cron_expression`.

Hoy Kesto copia solo cuando alguien se lo pide: `cron_expression` se guarda, se sirve, y **nadie la evalúa**. `src/scheduler` es el único de los siete módulos del §7 que sigue vacío, y la única puerta que crea una ejecución es `POST /api/v1/jobs/:id/run` —la mano del operador—. Al cerrar este plan, una tarea con `enabled = 1` y una expresión válida crea su propia ejecución cuando le toca, sin que nadie llame a `run`.

## Alcance

**Dentro:**

1. `src/scheduler/` — el módulo del planificador: la **validación** de la expresión cron, el **servicio** de planificación sobre `tokio-cron-scheduler`, la **sincronización** periódica con `backup_jobs` y el **disparo**, que es una llamada a `Motor::encolar`.
2. La **integración en `src/main.rs`**: el planificador arranca **detrás** del motor y su parada ordenada ocurre **antes** de que el motor se pare.
3. La **validación de la expresión** en `POST /api/v1/jobs` y en `PUT /api/v1/jobs/{id}`: una expresión que no parsea SHALL contestar `400` con la envoltura, y NO SHALL escribir la fila. Es el delta de `api-resources`.
4. Las dependencias nuevas en `Cargo.toml` (`tokio-cron-scheduler` y `croner`) y el `Cargo.lock` resuelto **con red** y commiteado, porque el CI corre `cargo test --locked`.

**Fuera (y por qué):**

- **`ui/` y el `Dockerfile`**: son sus propios cambios.
- **Un endpoint que publique el «próximo disparo»** de cada tarea: el panel lo pedirá cuando exista; hoy no hay consumidor.
- **Configuración nueva por variable de entorno**: el §8 no pide ninguna y este change no necesita ninguna; el período de sincronización es una constante declarada, no un ajuste.
- **Restauración y los `keep_*` sin columna**, que el SRS deja fuera.

## Decisiones que este plan fija

1. **D1 — Se usa `tokio-cron-scheduler` 0.15.1, con su API simple y sin su API de persistencia.** El §2 lo nombra y el módulo del §7 existe por él. La API que se usa es `JobScheduler::new()` → `add(Job) -> Uuid` → `start()` → `remove(&Uuid)` → `shutdown()`, más `next_tick_for_job` y `time_till_next_job` para observar. **NO** se usa su API de persistencia (`JobBuilder`, el trait `JobCode`, los `MetaDataStorage`/`NotificationStore`): la fuente de verdad de Kesto es SQLite y duplicarla en el almacén de la librería crearía dos verdades que se pueden desincronizar.
2. **D2 — La expresión se valida con `croner`, que es el parser que la librería usa por dentro.** `tokio-cron-scheduler` 0.15.1 depende de `croner ^3.0.0`; se declara `croner` como dependencia directa **en el mismo major** para que el resolvedor unifique en una sola copia, igual que se hizo con `base64` y `futures-util` en changes anteriores.
3. **D3 — El cron es de 5 o 6 campos, y el módulo normaliza cinco a seis antes de dárselo al reloj.** `croner` 3.0.1 con su parser por defecto —`Seconds::Optional`, el `#[default]` del enum— acepta las dos formas, inyectando el segundo `0` de una expresión de cinco campos (`croner/src/parser.rs:122-130`: `if num_parts == 5 { parts.insert(0, "0"); }`). Pero `tokio-cron-scheduler` 0.15.1 **no** usa ese parser: construye el suyo con `Seconds::Required` y `dom_and_dow(true)` en los **cinco** sitios donde parsea —`job/mod.rs:135`, `:227`, `builder.rs:105`, `lib.rs:126`, `job/mod.rs:896`—, así que `0 3 * * *` —la forma de la base, de las pruebas y del §3— llega al reloj como `ParseSchedule`. Está medido en las **tres** puertas de entrada (`Job::new_async`, `Job::new` y `JobBuilder::with_schedule`), y también lo está su contrario: el error literal del parser es `Invalid pattern: Pattern must have 6 or 7 fields when seconds are required and years are optional.` De ahí la forma que fija este plan: **el módulo normaliza** anteponiendo `0 ` **solo cuando el número de campos es exactamente 5**, sin tocar las de 6 ni las de 7 —una de 7 con año es válida y no se reescribe—. No es una invención: es literalmente lo que `croner` hace con cinco campos cuando el parser es `Optional`, reproducido a propósito para que la validación y la evaluación hablen la misma gramática. La validación usa **la configuración de parser de la biblioteca** (`Seconds::Required` + `dom_and_dow(true)`) sobre la forma normalizada, que es lo que hace imposible que la API acepte lo que el reloj no sabe leer. Y **la fila guarda lo que el usuario escribió**: `backup_jobs.cron_expression` no se reescribe y la normalización ocurre en vuelo, al construir el job, para no cambiarle a nadie lo que tecleó.
4. **D4 — La zona horaria es UTC.** Es la del resto del motor (`OffsetDateTime::now_utc()`), y una agenda que se interpretara en hora local cambiaría de disparo con el horario de verano sin que nadie lo pidiera.
5. **D5 — La sincronización con la base es periódica, no por notificación.** Cada ciclo el planificador lee `backup_jobs` y añade, retira o reemplaza lo que haga falta. Se elige así porque cubre también los cambios que **no** pasan por la API y no acopla la capa HTTP al planificador; el precio —una latencia acotada entre el alta y su planificación— se declara en la deuda.
6. **D6 — El disparo es una llamada a `Motor::encolar`, la misma puerta que el disparo manual.** El planificador NO crea la fila de `job_executions` por su cuenta; hereda así la guarda **atómica** del `409` y el nacimiento del evento en el hub, sin una segunda ruta de escritura que pueda divergir.
7. **D7 — Los desenlaces del disparo no tumban al planificador.** `YaTieneUnaEnCurso` y `ColaDelRepositorioLlena` se registran y se **saltan** (el siguiente disparo lo intentará); `TareaQueNoEsta` **retira** la tarea de la planificación; `MotorParado` **detiene** el planificador; `Base` se registra y **se sigue**.
8. **D8 — Una tarea con `enabled = 0` no se planifica.** `enabled` gobierna al planificador y el disparo manual sigue gobernado por el operador, que es lo que `api-endpoints` dejó fijado con una prueba.
9. **D9 — Una `cron_expression` que no parsea no impide arrancar.** Si una fila ya guardada tiene una expresión inválida, se registra el error y esa tarea se salta; el planificador arranca y atiende las demás.
10. **D10 — El coste de `chrono` se asume y se declara.** `tokio-cron-scheduler` 0.15.1 arrastra `chrono` 0.4 y `chrono-tz` como dependencias no opcionales: es una segunda familia de fechas cuando este proyecto eligió `time` a conciencia. El módulo del planificador NO convierte fechas entre familias —si necesita una fecha de la librería, la usa dentro y no la cruza con las del §3—, y el árbol resultante se documenta. **Medido en la fase 0, y completa esta decisión sin contradecirla**: `chrono` sigue con **una sola copia** (0.4.45) y lo que creció fue el número de caminos de entrada (de 1 a 4); `chrono-tz` 0.10.4 es **nuevo** y entra por un solo camino; el diff del `Cargo.lock` es **aditivo** (12 altas, ninguna versión existente cambió) y arrastra **tres duplicados nuevos** de librerías de terceros que no se pueden unificar sin forzar: `num-derive` 0.4.2 junto al 0.5.1 que ya estaba, y `strum` 0.27.2 junto al 0.28.0. No se han «arreglado» a propósito: subirlos exige tocar aguas arriba o forzar un `[patch]`, y eso es decisión de diseño, no de quien resuelve. Los declara la deuda de más abajo. **Enmendada al implementar la fase 2**: `chrono` deja de ser un arrastre y pasa a ser **dependencia directa** (`chrono = "0.4"` en `Cargo.toml`, con su comentario de coste, y una línea nueva en `Cargo.lock`), porque ni `croner` 3.0.1 ni `tokio-cron-scheduler` 0.15.1 lo reexportan y `Agenda::proxima_tras` no se puede implementar sin nombrarlo. El cruce de familias sigue prohibido **en todo el módulo salvo un punto**: la conversión `time` → `chrono` → `croner` → `time` vive en un único sitio documentado (`src/scheduler/validacion.rs`), y la superficie pública sigue hablando `time`.
11. **D11 — El planificador no entra en `AppState`.** Lo arranca `main` después del motor y su mango viaja a la parada ordenada; la API no lo necesita. Si algún día el panel pide el «próximo disparo», ese endpoint abrirá la puerta a meterlo en el estado, y eso será otro change.
12. **D12 — La garantía de «no se encola tras la parada» es del módulo, no de la biblioteca.** Medido: `shutdown()` del ticker es un `swap` atómico (`scheduler.rs:228-230`), `JobScheduler::shutdown` vuelve en **3,76 µs** con un job de 5 s en vuelo —que sigue avanzando después y es **inobservable**, porque su `JoinHandle` se descarta (`job/runner.rs:53`)—, `inited()` se queda en `true` tras apagar, y un tick puede disparar **después** de que la parada empiece, **4 de 4 rondas** medidas con desfases de 610–880 ms, porque el `JobRunner` no mira ningún flag (`job/runner.rs:28-65`) y el ticker lee el suyo una vez por vuelta antes de un `sleep` de 500 ms (`scheduler.rs:77-85`). Conclusión que el plan fija: el módulo cierra **su propia puerta**, y la cierra **en la misma sección crítica que el encolado**, de modo que un encolado o termina antes de que la puerta cierre o no empieza; eso cubre también el tick que la biblioteca dispare de más. La puerta no se puede delegar. Y el plazo de la parada acota **el drenaje del módulo**, no la llamada a `shutdown()`: envolver ésta en un `timeout` no acotaría nada, porque vuelve en microsegundos.
13. **D13 — El mensaje del rechazo lo escribe el módulo.** El `Display` de `croner` **no nombra la expresión** (`croner/src/errors.rs:45-60`: `Invalid pattern: {msg}`, `Pattern contains illegal characters: {msg}`, `CronPattern cannot be an empty string.`), y la biblioteca lo empeora: `map_err(|_| JobSchedulerError::ParseSchedule)` tira el `CronError` entero (`job/mod.rs:139`) y `impl Display for JobSchedulerError` es `Debug::fmt` (`error.rs:41-45`), así que al operador le llega la palabra `ParseSchedule` y nada más. El requisito exige que el error **nombre la expresión**, así que el mensaje lo compone el módulo: nombra la expresión y el campo `cron_expression`.
14. **D14 — La tabla `job_id -> Uuid` del módulo es la única autoridad sobre lo planificado.** La biblioteca no ofrece ninguna señal de estado utilizable: `remove` de un `Uuid` que no existe devuelve `Ok(())` (`simple/metadata_store.rs:56-66`), así que no sirve para comprobar existencia; `inited()` no distingue «nunca arrancado» de «ya parado»; `add` después de apagar devuelve `Ok(uuid)` y no dispara jamás; y el mango de un job es su `Uuid` y nada más, porque `add` **consume** el `JobLocked` y no hay `get`/`find` ni registro por nombre. Consecuencia de forma que hay que fijar antes de escribir: `shutdown`, `next_tick_for_job` y `time_till_next_job` piden **`&mut self`** mientras `add`, `remove` y `start` van con `&self`, así que el módulo **se queda el planificador** y le pasa **clones** a los cierres de disparo —cada job recibe uno (`runner.rs:51`), y todos comparten estado porque los campos son `Arc`—. Además la parada es de **un solo sentido**: tras apagar, `start()` devuelve `Err(StartScheduler)` y no hay reanudación.

## Tareas, por fases

### 0. Preparación

- [x] **0.1** Añadir `tokio-cron-scheduler = "0.15.1"` y `croner = "3"` a `Cargo.toml` y **resolver el `Cargo.lock` con red**, porque ninguna de las dos está en la caché local → esperado: árbol resuelto, `tokio-cron-scheduler` 0.15.1 y `croner` 3.x presentes **una sola vez** cada uno, `chrono` y `chrono-tz` declarados como coste en el comentario de la dependencia, y `Cargo.lock` commiteado. → MEDIDO: Lock resuelto con red y commiteado: `tokio-cron-scheduler` 0.15.1 y `croner` 3.0.1, **una sola copia** de cada uno (`cargo tree -i croner` da los dos padres apuntando al mismo nodo); `chrono` sigue con una sola copia (0.4.45); `cargo check --locked --all-targets` en verde. Commiteado en `2823d16`.
- [x] **0.2** **Spike de la API de `tokio-cron-scheduler` 0.15.1** (documentada solo al 14 %): medir `JobScheduler::new()`, `add(...)`, `start()`, `remove(...)`, `shutdown()` y la **variante asíncrona** de `Job` (`JobToRunAsync`) → esperado: escrito, con el nombre exacto del constructor de un job con closure `async` y la firma que recibe, porque el disparo tiene que ser `async` (llama a `Motor::encolar`, que lo es). → MEDIDO: `Job::new_async(schedule, run)` con `run: FnMut(Uuid, JobsSchedulerLocked) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync`: **el cierre asíncrono existe**, así que el disparo no hay que sacarlo del runtime. Las cinco del ciclo de vida son `async` y **solo** `shutdown`, `next_tick_for_job` y `time_till_next_job` piden `&mut self`. `add` y `remove` se auto-inicializan. **El `Uuid` que devuelve `add` es el único mango**: `add` consume el `JobLocked` y no hay `get`/`find`/registro por nombre. `new_async` delega en `new_async_tz(schedule, Utc, run)`, así que **la zona por defecto ya es UTC** y D4 sale sin cambios.
- [x] **0.3** Medir **si** la biblioteca acepta la expresión de cinco campos que ya está en la base, o solo la de seis → esperado: un job con `0 3 * * *` se construye sin error y `next_tick_for_job` devuelve una hora en UTC; anotado el error literal si no lo acepta, porque entonces la validación de la API y la de la librería discreparían. → MEDIDO: **La biblioteca RECHAZA cinco campos**: parsea con `Seconds::Required` y `0 3 * * *` da `ParseSchedule` en las tres puertas de entrada, mientras `croner` con su parser por defecto (`Seconds::Optional`) acepta cinco y seis inyectando `0` (`parser.rs:122-130`). El texto de `croner` **no nombra la expresión**, y la biblioteca además lo descarta. De aquí salen D3 enmendada, D12 y D13.
- [x] **0.4** Medir el **comportamiento de `shutdown()`** con un job en vuelo y el de un `add` después del `shutdown` → esperado: escrito si `shutdown` espera a los jobs en curso y qué devuelve un `add` posterior. → MEDIDO: `shutdown()` vuelve en **3,76 µs** con un job de 5 s en vuelo, **no espera**, y el cierre en vuelo sigue y es inobservable. Un tick dispara **después** de la parada: **4 de 4** rondas con desfases de 610–880 ms. `add` posterior devuelve `Ok` sin disparar; `inited()` sigue en `true`; `start()` tras apagar da `Err(StartScheduler)`. El único gancho awaitable es `set_shutdown_handler`. De aquí sale D12.
- [x] **0.5** Medir el **coste de `chrono` en el árbol** → esperado: escrito que `chrono` 0.4 y `chrono-tz` entran como dependencias **no opcionales** de la biblioteca, que `chrono` **ya estaba** por debajo (un solo camino: `suppaftp` ← `opendal-service-ftp` ← `opendal` ← `rustic_backend`) y que lo que cambia es que pasa a ser una segunda familia de fechas **visible al código**, con su `Utc` al lado del `time` elegido a conciencia; contadas las copias de cada uno en el `Cargo.lock` nuevo y añadido el número real, no una estimación. → MEDIDO: `chrono` 0.4.45 con **una sola copia** y cuatro caminos de entrada (antes uno, `suppaftp` ← `opendal-service-ftp` ← `opendal` ← `rustic_backend`); `chrono-tz` 0.10.4 **nuevo**, un solo camino; ninguna versión existente cambió y el diff del lock es aditivo (12 altas). Tres duplicados nuevos de terceros, declarados.

### 1. RED

- [x] **1.1** Pruebas de la **validación**: `0 3 * * *` válida, `0 0 3 * * *` válida, `"a b c d e"` inválida, cadena vacía inválida, y que el motivo nombra la expresión —y además que **la forma de cinco campos que se acepta es la que el reloj acepta**: construir con ella el job no da `ParseSchedule`, que es la discrepancia que cazó el spike y la que deja una tarea que no corre nunca— → cubre `scheduler`, requisito 1; esperado: rojas porque no existe la función. → RED: 6 pruebas nuevas; rojo por «no está implementado», con las 381 preexistentes en verde.
- [x] **1.2** Pruebas de la **planificación inicial**: una tarea `enabled = 1` queda planificada; una `enabled = 0` NO; una con expresión inválida se registra y se salta sin impedir que arranque → cubre el requisito 2; esperado: rojas. → RED: pruebas de la planificación inicial; rojo en el arranque sin implementar.
- [x] **1.3** Pruebas de la **sincronización**: alta → planificada; borrado → retirada; cambio de cron → reemplazada; `enabled` a 0 → retirada; vuelta a 1 → planificada → cubre el requisito 3; esperado: rojas. → RED: pruebas de la reconciliación (alta, borrado, cambio de cron sin duplicar, desactivar/reactivar).
- [x] **1.4** Pruebas del **disparo**: que un disparo crea una ejecución `pending` llamando a `Motor::encolar` y **no** por una segunda ruta de escritura → cubre el requisito 4; esperado: rojas. → RED: pruebas del disparo y de sus desenlaces; la tabla pura cubre las variantes que no se alcanzan sin contenedor.
- [x] **1.5** Pruebas de los **desenlaces**: tarea ya en curso → se registra y se salta; tarea borrada → se retira; motor parado → el planificador se detiene → cubre el requisito 5; esperado: rojas. → RED: pruebas del disparo y de sus desenlaces; la tabla pura cubre las variantes que no se alcanzan sin contenedor.
- [x] **1.6** Pruebas del **ciclo de vida**: arranca detrás del motor; al empezar la parada deja de aceptar y su parada está acotada —y que **un tick posterior al inicio de la parada no crea ninguna ejecución**, porque la biblioteca sigue disparando después de que la parada empiece (medido: 4 de 4), así que la puerta tiene que ser del módulo— → cubre el requisito 6; esperado: rojas. → RED: prueba de la parada sin encolar; los dos escenarios que dependen del cableado del proceso se cerraron en 2.2.
- [x] **1.7** Pruebas de la **API**: `POST /api/v1/jobs` con expresión inválida contesta `400` con la envoltura y **no** deja fila; `PUT` con expresión inválida igual; y una expresión válida de 5 campos se sigue aceptando → cubre `api-resources`, el requisito del delta; esperado: rojas. → RED: 4 pruebas en src/api/jobs.rs; 3 rojas (hoy el cron no se valida en ningún sitio) y la de la forma de cinco campos ya verde como guardarraíl.

### 2. GREEN

- [x] **2.1** Implementar `src/scheduler/` con el mínimo para pasar la fase 1, nombrando ya las **dos piezas nuevas** que la medición obliga: la **normalización de cinco a seis en vuelo** —anteponer `0 ` solo cuando el número de campos sea exactamente 5, validar y evaluar con la configuración del reloj y no reescribir la fila— y la **puerta de parada propia**, cerrada en la misma sección crítica que el encolado → esperado: verde y `cargo check` sin errores. → VERDE: la validación, la planificación, la reconciliación, el disparo con su tabla de desenlaces y la puerta de parada en la misma sección crítica que el encolado; la API llama a la validación desde alta_desde.
- [x] **2.2** Integrar en `src/main.rs` el arranque detrás del motor y la parada antes de pararlo → esperado: verde. → VERDE: el planificador arranca detrás del motor y delante del listener, y se para lo primero dentro del futuro de apagado; dos escenarios del requisito 6 cerrados en tests/arranque.rs.

### 3. REFACTOR

- [x] **3.1** `cargo fmt --all` y `cargo clippy --all-targets -- -D warnings` → esperado: cero avisos. → VERDE: cargo fmt --check y cargo clippy --all-targets -- -D warnings, los dos limpios.
- [x] **3.2** `cargo doc` con `RUSTDOCFLAGS="-D warnings"` → esperado: sin enlaces rotos. → VERDE: just doc —RUSTDOCFLAGS="-D warnings" cargo doc --no-deps— limpio tras corregir un enlace intra-doc.

### 4. Operador

- [ ] **4.1** Medir en verde con el Podman de esta máquina: una tarea con una expresión de segundos (`*/5 * * * * *`) y un repositorio local crea su ejecución sola → esperado: la fila nace sin que nadie llame a `run`.
- [ ] **4.2** Declarar qué queda sin cubrir por una prueba automática y por qué, e incluir **expresamente** estos dos: la carrera de doble disparo por `next_tick`/`last_tick` —no medida, no descartada— y el *jitter* del tick de hasta 500 ms.

### 5. Cierre

- [ ] **5.1** `just check-all` en verde, y **`just check-spec`** —que `check-all` no arrastra— porque el cierre exige que el change siga siendo el activo.
- [ ] **5.2** Archivar el change (`just spec-archive scheduler`) y fusionar la capability nueva en `openspec/specs/scheduler/`.
- [ ] **5.3** Escribir `docs/FEATURE-006-scheduler.md` con lo que **realmente** quedó construido: contratos, decisiones y deuda.
- [ ] **5.4** Renombrar este plan a `plans/PLAN-006-COMPLETED.md` (el renombrado lo hace el director, no tú).

## Anexo: el API literal de las dos dependencias

Todo lo de este anexo salió de un spike en `/tmp/opencode/spike-scheduler/` (los binarios `a_api`, `b_croner` y `c_shutdown`, con sus `.out`) que **compiló y corrió**: los tipos y las firmas se declaran **explícitamente** en ese código, así que una firma falsa habría impedido compilar. Vale la pena tenerlo aquí porque la documentación del crate es `rust,ignore` y **no compila**: no se puede consultar. Ojo: el spike vive en `/tmp`, donde un reinicio se lo lleva; lo que importa queda transcrito abajo.

### `croner` 3.0.1 — validar con el parser del reloj

```rust
use croner::Cron;
use croner::parser::{CronParser, Seconds};

// El parser POR DEFECTO del crate: acepta 5 y 6 campos, inyectando el segundo "0".
let c = Cron::from_str("0 3 * * *")?;              // -> Ok

// El parser que usa la biblioteca del reloj: EXIGE 6 campos (7 si llevan año).
let parser = CronParser::builder()
    .seconds(Seconds::Required)
    .dom_and_dow(true)
    .build();
parser.parse("0 3 * * *")?;                        // -> Err(...)
parser.parse("0 0 3 * * *")?;                      // -> Ok
```

- No hay dos constructores: `Seconds::{Optional, Required, Disallowed}` son variantes del **mismo** `CronParser`, y `Cron::from_str` usa el `Default` del enum, que es `Optional`.
- `Cron` implementa `Display` dando la **forma normalizada** —`* * * * *` sale como `0 * * * * *`— y tiene `describe()`, que da una frase humana; sirve de apoyo para el mensaje del rechazo, aunque **no** trae la expresión original, que es lo que obliga a D13.
- El texto del error **no nombra la expresión**: `Display` dice `Invalid pattern: <motivo>`. De ahí D13.

### `tokio-cron-scheduler` 0.15.1 — la vida y el disparo

```rust
use std::future::Future;
use std::pin::Pin;
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError, JobToRunAsync};
use uuid::Uuid;

let sched: JobScheduler = JobScheduler::new().await?;   // async
sched.start().await?;                                   // async, y con &self

// El cierre asíncrono: la firma literal del alias JobToRunAsync.
let run: Box<JobToRunAsync> = Box::new(
    move |job_id: Uuid, sched: JobScheduler| -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            // aquí va la llamada a Motor::encolar
        })
    },
);

// La forma de 6 campos, que es la normalizada; con 5 la biblioteca da ParseSchedule.
let job = Job::new_async("0 0 3 * * *", run)?;
let job_id: Uuid = sched.add(job).await?;               // el Uuid ES el mango
```

- `JobScheduler` es **`Clone`** y los clones comparten estado: cada disparo recibe **su** clon en el cierre, y por eso el módulo puede quedarse el original. Es exactamente lo que fija D14.
- `add`, `remove` y `start` van con **`&self`**; `next_tick_for_job`, `time_till_next_job` y `shutdown` con **`&mut self`** — de ahí que se llamen sobre un clon, `sched.clone().next_tick_for_job(id).await`, y que el módulo tenga que clonar.
- `remove(&uuid_inventado)` devuelve `Ok(())` sin error: **no** sirve para comprobar si un job existe.
- `JobLocked` **no** implementa `Debug`, así que para imprimir un `Result` que lo contenga hay que resumirlo a mano.
- `Job::new` es la variante con cierre **bloqueante** y `JobBuilder::with_schedule` tiene su propio parser: ninguna de las dos se usa (D1).

## Deuda que este plan declara por adelantado

La **latencia de sincronización** —el hueco entre dar de alta una tarea y verla planificada— es el precio que se paga por D5: la sincronización es periódica y no por notificación, así que una tarea recién creada no dispara hasta el siguiente ciclo; a cambio, el planificador ve también los cambios que no pasan por la API y la capa HTTP no queda acoplada a él. La **zodificación es UTC** (D4), así que quien espere que `0 3 * * *` signifique las tres de su hora local se llevará una sorpresa, y este plan no la mitiga: lo deja escrito. En el árbol de dependencias entra **`chrono`** (con `chrono-tz`) como arrastre no opcional de `tokio-cron-scheduler` (D10), una segunda familia de fechas junto al `time` que este proyecto eligió a conciencia; el módulo no cruza fechas entre familias, pero el árbol lo carga. Y el **«próximo disparo»** de cada tarea **no se publica**: no hay endpoint ni campo que lo exponga, porque hoy no hay consumidor; el panel lo pedirá cuando exista y eso llegará con su propio change, probablemente el que meta el planificador en `AppState` (D11).

Medido en la fase 0, el árbol arrastra además **tres duplicados nuevos** de librerías de terceros que este change **no** unifica a propósito: `num-derive` 0.4.2 junto al 0.5.1 que ya estaba, y `strum` 0.27.2 junto al 0.28.0. Subirlos exige tocar aguas arriba o forzar un `[patch]`, y eso es decisión de diseño, no de quien resuelve, así que se declaran aquí en vez de silenciarlos. El diff del `Cargo.lock` es **aditivo** —12 altas, ninguna versión existente cambiada— y `chrono` sigue con **una sola copia** (0.4.45) aunque pase de uno a cuatro caminos de entrada (D10).

Y quedan **dos riesgos que la fase 0 dejó abiertos** —los dos son de la biblioteca y no del módulo, y los dos están ya declarados en el `design.md` del change—, que este plan no puede dar por cerrados. El primero es **la carrera de doble disparo por `next_tick`/`last_tick`**: la biblioteca actualiza esos dos valores en una tarea aparte, **después** de despachar (`scheduler.rs:174-222`), mientras la decisión de disparar los lee del almacén; si esa actualización se retrasara más de 500 ms, la vuelta siguiente vería los valores viejos y volvería a disparar. **No se ha conseguido provocar** —en corridas de 2,6 s un job de cada segundo disparó exactamente 3 veces, sin duplicados—, así que queda **no medido**, no descartado. Y con la nota de método que importa: el primer intento de esta medida dio **0 de 4** porque los desfases caían en la vuelta equivocada del ticker, y el **4 de 4** que sí reproduce el fallo es con desfases de 610–880 ms; es decir, el experimento es sensible al desfase y un 0 de 4 no era «no pasa», era «no he apuntado ahí». El segundo es **el *jitter* del tick**: por construcción de la biblioteca es **≤ 500 ms** (duerme 500 ms y mira una vez por vuelta, `scheduler.rs:77-85`), y **no se ha cronometrado** disparo a disparo; es el retardo que puede llevar una copia programada sobre su hora.

## Estado de la ejecución

Dónde está esto, para quien lo retome sin la conversación delante:

- **Rama** `feature/scheduler`, creada desde `development`. **Nada se ha empujado** y no hay PR abierto: el trabajo vive solo en local.
- **El change está implementado y verde**, sobre la base de la fase 0 (`cbe505e`, `2823d16` y la enmienda de la fase 0, que sigue siendo el tip de la rama —su hash no se escribe aquí porque este fichero viaja dentro de ese mismo commit—).
- **Fase 0: completa** (0.1 a 0.5), con la medición volcada en las tareas y en las decisiones D3, D12, D13 y D14, y enmendada en el `design.md` y en las specs del change.
- **Fase 1 (RED) cerrada**: al terminar quedaron **382 pruebas en verde y 25 en rojo** —6 de la validación, 16 de la planificación y el disparo, y 3 de la API—, las rojas por la razón correcta («no está implementado»).
- **Fase 2 (GREEN) cerrada**: **407 pruebas en el binario y 5 en `tests/arranque.rs`, 0 fallos**. La validación se aplica en `alta_desde` —alta y edición—, y el planificador se monta en `src/main.rs` **detrás** del motor y se para lo primero dentro del futuro de apagado; dos escenarios del requisito 6 se cierran en `tests/arranque.rs`.
- **Fase 3 (REFACTOR) hecha** (3.1 y 3.2): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` y `just doc` (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`), los tres limpios.
- **Fases 4 y 5 pendientes**: la verificación del operador (4.1 y 4.2) y el cierre (5.1 a 5.4).
- **Sin commitear en el árbol**: `src/scheduler/` sigue sin seguimiento; además hay trabajo ajeno de la poda v2 de la documentación (`AGENTS.md`, `GIT_FLOW.md` y cinco `docs/*.md`) que se decidió dejar fuera del PR, y `assets/`, que es del operador. Nada de esto entra en ningún commit del planificador.
- **Por dónde se empieza a leer**: la sección `## Siguiente movimiento` de este mismo fichero.

## Siguiente movimiento

1. **4.1** (operador, contra el Podman de esta máquina): una tarea con una expresión que use los **segundos** —por ejemplo cada diez segundos—; comprobar que solas aparecen las filas en `job_executions`, que se ven los eventos en `curl -N` sobre `GET /api/v1/events`, que editar la expresión y borrar la tarea dejan de disparar tras el período de reconciliación, y que `enabled = 0` deja de disparar y reactivar vuelve a dispararla.
2. **4.2** declarar por escrito lo que queda sin cubrir y **de forma expresa los dos riesgos abiertos de la fase 0**: la carrera de doble disparo por `next_tick`/`last_tick` —no medida, no descartada— y el *jitter* del tick de hasta 500 ms. Se vuelca en `docs/FEATURE-006-scheduler.md`.
3. **5.1** `just check-all` y `just check-spec`; **5.2** `just spec-archive scheduler`, comprobando el `Purpose` de `api-resources` tras el archivado.
4. `docs/FEATURE-006-scheduler.md` y `just plan-done 6` (→ `plans/PLAN-006-COMPLETED.md`).
5. Revisión del código con `@rust-reviewer`, y después los commits con gitmoji y el PR a `development`.

## Cierre

- **4.1 verde** contra el Podman de esta máquina: una tarea `*/10 * * * * *` con `enabled = 1` creó **6 ejecuciones en 60 s** (todas `success`, cadencia exacta de 10 s) sin que nadie llamara a `run`; editar la expresión, poner `enabled = 0` y borrar la tarea dejan de disparar tras el período de reconciliación, reactivar vuelve a dispararla, y el `SIGTERM` para el planificador antes que el motor. **Y destapó un bug**: `planificar_una` trataba el `Uuid` de la biblioteca como `job_id` de Kesto y cada tick acababa en `TareaQueNoEsta`, sin crear nunca una ejecución; corregido capturando el `job_id` real en el cierre, con la prueba de regresión `un_tick_real_del_reloj_crea_la_ejecucion_de_su_tarea`.
- **Revisión posterior de `src/scheduler/` (dos hallazgos, corregidos con TDD)**: la **puerta de parada** pasa a ser una bandera atómica que `parar` cierra **antes de cualquier `await`** —antes, si el candado no llegaba dentro del plazo, quedaba abierta para siempre, `esta_parado()` mentía y un tick posterior encolaba tras la parada—, con `esta_parado()` síncrono y sin candado; y la tabla `planificadas` **no** pierde el mango hasta que `reloj.remove` confirma `Ok` —antes, un `remove` fallido dejaba la tarea planificada y sin forma de retirarla—, con el caso «cambió» sin añadir la nueva planificación hasta confirmar. Pruebas: `la_puerta_queda_cerrada_aunque_el_drenaje_venza_el_plazo`, `un_disparo_tras_una_parada_que_vencio_el_plazo_no_encola` y `una_retirada_que_el_reloj_no_confirma_deja_el_mango_en_la_tabla`.
- **4.2 declarada por escrito**, y el detalle completo vive en `docs/FEATURE-006-scheduler.md` (sección «Verificación manual»): el tick de calendario real se prueba forzando el momento —salvo la regresión de 1 s, que usa un tick real con plazo acotado—, la recuperación de lo que no corrió no existe por decisión, la parada con un job en vuelo no se puede provocar si la biblioteca no deja, y **los dos riesgos de la fase 0 siguen abiertos** —la carrera de doble disparo por `next_tick`/`last_tick`, no medida ni descartada, y el *jitter* del tick de hasta 500 ms—, más la deuda de la columna de origen que el §3 no tiene y la de las ejecuciones perdidas.
- **5.1**: `just check` **verde** —`fmt`, `clippy` con `-D warnings` y `cargo test --locked`, que es lo que comprueba de verdad que el `Cargo.lock` nuevo está commiteado—. `just check-all` **no puede ponerse verde todavía** y no es un fallo: sus cuatro pasos del panel abortan solos mientras no exista `ui/package.json`, y el panel es de un plan posterior.
- **5.2 pendiente** de `just spec-archive scheduler` (con la comprobación del `Purpose` de `api-resources`) y **5.4 pendiente** del renombrado a `PLAN-006-COMPLETED.md`: las dos las cierra el director, no este plan.
