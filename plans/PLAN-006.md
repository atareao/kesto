# PLAN-006 — `scheduler`

> **Estado:** en curso (change proposal escrito; pendiente de aprobación del usuario e implementación TDD).
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
3. **D3 — El cron es de 5 o 6 campos, y la base ya usa 5.** `croner` acepta los **segundos como opcionales**, así que `0 3 * * *` (5 campos —el de la base, las pruebas y el SRS—) y `0 0 3 * * *` (6) son **ambos válidos**. Está medido en la documentación de `croner`, y es lo que hace que la validación no rompa ninguna fila existente.
4. **D4 — La zona horaria es UTC.** Es la del resto del motor (`OffsetDateTime::now_utc()`), y una agenda que se interpretara en hora local cambiaría de disparo con el horario de verano sin que nadie lo pidiera.
5. **D5 — La sincronización con la base es periódica, no por notificación.** Cada ciclo el planificador lee `backup_jobs` y añade, retira o reemplaza lo que haga falta. Se elige así porque cubre también los cambios que **no** pasan por la API y no acopla la capa HTTP al planificador; el precio —una latencia acotada entre el alta y su planificación— se declara en la deuda.
6. **D6 — El disparo es una llamada a `Motor::encolar`, la misma puerta que el disparo manual.** El planificador NO crea la fila de `job_executions` por su cuenta; hereda así la guarda **atómica** del `409` y el nacimiento del evento en el hub, sin una segunda ruta de escritura que pueda divergir.
7. **D7 — Los desenlaces del disparo no tumban al planificador.** `YaTieneUnaEnCurso` y `ColaDelRepositorioLlena` se registran y se **saltan** (el siguiente disparo lo intentará); `TareaQueNoEsta` **retira** la tarea de la planificación; `MotorParado` **detiene** el planificador; `Base` se registra y **se sigue**.
8. **D8 — Una tarea con `enabled = 0` no se planifica.** `enabled` gobierna al planificador y el disparo manual sigue gobernado por el operador, que es lo que `api-endpoints` dejó fijado con una prueba.
9. **D9 — Una `cron_expression` que no parsea no impide arrancar.** Si una fila ya guardada tiene una expresión inválida, se registra el error y esa tarea se salta; el planificador arranca y atiende las demás.
10. **D10 — El coste de `chrono` se asume y se declara.** `tokio-cron-scheduler` 0.15.1 arrastra `chrono` 0.4 y `chrono-tz` como dependencias no opcionales: es una segunda familia de fechas cuando este proyecto eligió `time` a conciencia. El módulo del planificador NO convierte fechas entre familias —si necesita una fecha de la librería, la usa dentro y no la cruza con las del §3—, y el árbol resultante se documenta.
11. **D11 — El planificador no entra en `AppState`.** Lo arranca `main` después del motor y su mango viaja a la parada ordenada; la API no lo necesita. Si algún día el panel pide el «próximo disparo», ese endpoint abrirá la puerta a meterlo en el estado, y eso será otro change.

## Tareas, por fases

### 0. Preparación

- [ ] **0.1** Añadir `tokio-cron-scheduler = "0.15.1"` y `croner = "3"` a `Cargo.toml` y **resolver el `Cargo.lock` con red**, porque ninguna de las dos está en la caché local → esperado: árbol resuelto, `tokio-cron-scheduler` 0.15.1 y `croner` 3.x presentes **una sola vez** cada uno, `chrono` y `chrono-tz` declarados como coste en el comentario de la dependencia, y `Cargo.lock` commiteado.
- [ ] **0.2** **Spike de la API de `tokio-cron-scheduler` 0.15.1** (documentada solo al 14 %): medir `JobScheduler::new()`, `add(...)`, `start()`, `remove(...)`, `shutdown()` y la **variante asíncrona** de `Job` (`JobToRunAsync`) → esperado: escrito, con el nombre exacto del constructor de un job con closure `async` y la firma que recibe, porque el disparo tiene que ser `async` (llama a `Motor::encolar`, que lo es).
- [ ] **0.3** Medir que la librería **acepta la expresión de 5 campos** que ya está en la base, no solo la de 6 → esperado: un job con `0 3 * * *` se construye sin error y `next_tick_for_job` devuelve una hora en UTC; anotado el error literal si no lo acepta, porque entonces la validación de la API y la de la librería discreparían.
- [ ] **0.4** Medir el **comportamiento de `shutdown()`** con un job en vuelo y el de un `add` después del `shutdown` → esperado: escrito si `shutdown` espera a los jobs en curso y qué devuelve un `add` posterior.

### 1. RED

- [ ] **1.1** Pruebas de la **validación**: `0 3 * * *` válida, `0 0 3 * * *` válida, `"a b c d e"` inválida, cadena vacía inválida, y que el motivo nombra la expresión → cubre `scheduler`, requisito 1; esperado: rojas porque no existe la función.
- [ ] **1.2** Pruebas de la **planificación inicial**: una tarea `enabled = 1` queda planificada; una `enabled = 0` NO; una con expresión inválida se registra y se salta sin impedir que arranque → cubre el requisito 2; esperado: rojas.
- [ ] **1.3** Pruebas de la **sincronización**: alta → planificada; borrado → retirada; cambio de cron → reemplazada; `enabled` a 0 → retirada; vuelta a 1 → planificada → cubre el requisito 3; esperado: rojas.
- [ ] **1.4** Pruebas del **disparo**: que un disparo crea una ejecución `pending` llamando a `Motor::encolar` y **no** por una segunda ruta de escritura → cubre el requisito 4; esperado: rojas.
- [ ] **1.5** Pruebas de los **desenlaces**: tarea ya en curso → se registra y se salta; tarea borrada → se retira; motor parado → el planificador se detiene → cubre el requisito 5; esperado: rojas.
- [ ] **1.6** Pruebas del **ciclo de vida**: arranca detrás del motor; al empezar la parada deja de aceptar y su parada está acotada → cubre el requisito 6; esperado: rojas.
- [ ] **1.7** Pruebas de la **API**: `POST /api/v1/jobs` con expresión inválida contesta `400` con la envoltura y **no** deja fila; `PUT` con expresión inválida igual; y una expresión válida de 5 campos se sigue aceptando → cubre `api-resources`, el requisito del delta; esperado: rojas.

### 2. GREEN

- [ ] **2.1** Implementar `src/scheduler/` con el mínimo para pasar la fase 1 → esperado: verde y `cargo check` sin errores.
- [ ] **2.2** Integrar en `src/main.rs` el arranque detrás del motor y la parada antes de pararlo → esperado: verde.

### 3. REFACTOR

- [ ] **3.1** `cargo fmt --all` y `cargo clippy --all-targets -- -D warnings` → esperado: cero avisos.
- [ ] **3.2** `cargo doc` con `RUSTDOCFLAGS="-D warnings"` → esperado: sin enlaces rotos.

### 4. Operador

- [ ] **4.1** Medir en verde con el Podman de esta máquina: una tarea con una expresión de segundos (`*/5 * * * * *`) y un repositorio local crea su ejecución sola → esperado: la fila nace sin que nadie llame a `run`.
- [ ] **4.2** Declarar qué queda sin cubrir por una prueba automática y por qué.

### 5. Cierre

- [ ] **5.1** `just check-all` en verde.
- [ ] **5.2** Archivar el change (`just spec-archive scheduler`) y fusionar la capability nueva en `openspec/specs/scheduler/`.
- [ ] **5.3** Escribir `docs/FEATURE-006-scheduler.md` con lo que **realmente** quedó construido: contratos, decisiones y deuda.
- [ ] **5.4** Renombrar este plan a `plans/PLAN-006-COMPLETED.md` (el renombrado lo hace el director, no tú).

## Deuda que este plan declara por adelantado

La **latencia de sincronización** —el hueco entre dar de alta una tarea y verla planificada— es el precio que se paga por D5: la sincronización es periódica y no por notificación, así que una tarea recién creada no dispara hasta el siguiente ciclo; a cambio, el planificador ve también los cambios que no pasan por la API y la capa HTTP no queda acoplada a él. La **zodificación es UTC** (D4), así que quien espere que `0 3 * * *` signifique las tres de su hora local se llevará una sorpresa, y este plan no la mitiga: lo deja escrito. En el árbol de dependencias entra **`chrono`** (con `chrono-tz`) como arrastre no opcional de `tokio-cron-scheduler` (D10), una segunda familia de fechas junto al `time` que este proyecto eligió a conciencia; el módulo no cruza fechas entre familias, pero el árbol lo carga. Y el **«próximo disparo»** de cada tarea **no se publica**: no hay endpoint ni campo que lo exponga, porque hoy no hay consumidor; el panel lo pedirá cuando exista y eso llegará con su propio change, probablemente el que meta el planificador en `AppState` (D11).
