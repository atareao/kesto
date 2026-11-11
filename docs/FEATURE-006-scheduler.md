# FEATURE-006 — scheduler

> Implementación del change proposal `scheduler`, en `openspec/changes/scheduler/` — una capability **nueva** (`scheduler`, 6 requisitos) y un **delta** de `api-resources` (1 requisito), con su `design.md`. `tasks.md`: **23 tareas** en cinco fases (0 preparación, 1 RED, 2 GREEN, 3 REFACTOR, 4 operador y 5 cierre). Se cerró la verificación del operador el **2026-11-10**.
> Rama: `feature/scheduler`, desde `development`. Plan: `plans/PLAN-006.md`. Especificación de origen: `docs/DEFINITION.md` §2 (arquitectura de componentes; «**Planificador:** `tokio-cron-scheduler`»), §3 (`backup_jobs.cron_expression` y `backup_jobs.enabled`), §5 (rutas) y §7 (`src/scheduler` era el único módulo del layout sin una sola línea).
> Estado: **implementado, verde y verificado a mano** — **408 pruebas en el binario y 5 en `tests/arranque.rs`, 0 fallos** (medido con `cargo test --locked`), incluida la prueba de regresión que salió de la verificación del operador (el tick real del reloj). Los portales del backend (`just check`, `just doc`) limpios. **Nada empujado**: el trabajo vive solo en local y no hay PR.
> Dependencias nuevas: `tokio-cron-scheduler` **0.15.1** y `croner` **3.0.1** —una sola copia de cada uno—, más `chrono` **0.4.45** (que pasa de arrastre a dependencia directa) y `chrono-tz` **0.10.4** (nuevo).

**Kesto ya tiene reloj.** Hasta este change, `cron_expression` era una columna que se guardaba, se servía y se editaba, y que **nadie interpretaba**: `enabled` tampoco decidía nada y la única puerta que creaba una ejecución era `POST /api/v1/jobs/:id/run` —la mano del operador—. Ahora una tarea con `enabled = 1` y una expresión válida **se dispara sola** cuando le toca, sin que nadie llame a `run`, y el módulo `src/scheduler/` —el último de los siete del §7 que seguía vacío— deja de estarlo.

## Qué es y por qué

El §2 pide un planificador desde el primer día, y era la única mitad del gobierno del tiempo que faltaba: el motor `backup-engine` dejó hecha la mitad difícil —`Motor::encolar(job_id)` ya lee la tarea, toma la guarda atómica que produce el `409`, crea la fila en `pending`, la mete en la cola de su repositorio y publica el nacimiento en el hub—, así que un disparo del planificador es **esa misma llamada** y lo que había que decidir era **cuándo** se llama.

Y con el reloj llegaba, inseparable, una deuda: **`cron_expression` no se validaba en ninguna parte**. Sin planificador, el daño de una errata era una columna con basura; con él, el daño es una tarea que **no corre nunca** y un panel que la pinta como programada. Este change trae las dos mitades juntas —el que evalúa y el que valida— porque una sin la otra deja el fallo exactamente donde estaba.

## Ficheros implicados

| Fichero | Qué es |
|---|---|
| `src/scheduler/mod.rs` | La superficie del módulo: publica `Agenda`, `ErrorDeExpresion`, `normalizar` y `validar` —la gramática— más `Planificador`, `Ajustes`, `ErrorDelPlanificador`, `PERIODO_DE_RECONCILIACION` y `PLAZO_DE_PARADA`. **Ningún tipo de `tokio_cron_scheduler` aparece aquí.** |
| `src/scheduler/validacion.rs` | La validación: `Agenda`, `normalizar` (5→6) y `validar`, con el analizador del reloj; `ErrorDeExpresion` con sus dos desenlaces (vacía / no parsea). |
| `src/scheduler/planificacion.rs` | El reloj de la biblioteca, la tabla `job_id → mango`, la planificación inicial, la reconciliación periódica y la **puerta de parada**. |
| `src/scheduler/disparo.rs` | El cuerpo del disparo —una llamada a `Motor::encolar` bajo la puerta— y la **tabla de desenlaces**. |
| `src/scheduler/pruebas.rs` | El aparato compartido por las pruebas: base migrada, motor arrancado y un abridor que tarda un segundo en decir que no abre. |
| `src/main.rs` | El planificador se monta **detrás** del motor y **delante** del estado y la escucha; su mango entra en el apagado ordenado, que lo para **lo primero**. |
| `src/api/jobs.rs` | El alta y la edición de una tarea llaman a `crate::scheduler::validar` **antes de escribir** y traducen su rechazo a un `400` que nombra el campo `cron_expression`. |

`ui/` y el `Dockerfile` siguen sin existir, como en `backup-engine`.

## Cómo funciona

### La validación publicada: `Agenda`, `validar` y `normalizar`

El módulo publica una sola pieza de la gramática, y la publica para que `src/api` la llame **sin importar la biblioteca del reloj**. Ese es el contrato del borde: si el borde HTTP nombrara `tokio_cron_scheduler`, la gramática tendría dos dueños —el que valida y el que evalúa— y el día que uno cambiara, la validación aceptaría lo que el reloj no sabe leer.

- **`validar(expresion) -> Result<Agenda, ErrorDeExpresion>`** es la única puerta que fabrica una `Agenda`; dentro normaliza y parsea con **el mismo analizador que la biblioteca** usa al montar el job.
- **`normalizar`** antepone `0 ` **solo cuando el número de campos es exactamente 5**, y devuelve tal cual las de 6 y las de 7 —una de siete, con año, es válida y no se reescribe—. No es una invención: es literalmente el `parts.insert(0, "0")` que hace el analizador por defecto de `croner`, reproducido a propósito para que lo que se valida y lo que se le entrega al reloj sean la misma cadena.
- **`Agenda::normalizada()`** es la forma de seis campos que hay que darle al reloj; **`Agenda::proxima_tras(desde)`** calcula la próxima ejecución **estrictamente posterior** a un instante que llega por parámetro, en **UTC**, y devuelve `time::OffsetDateTime`.

Tres hechos medidos que gobiernan esta pieza:

1. `croner` acepta cinco campos con su parser **por defecto** (`Seconds::Optional`), pero la **biblioteca del reloj** construye el suyo con **`Seconds::Required`** y `dom_and_dow(true)` en los cinco sitios donde parsea, así que `0 3 * * *` —la forma de la base, de las pruebas y del §3— llega al reloj como `ParseSchedule`. De ahí que la validación use **esa** configuración y no la del crate: validar con otra gramática aceptaría lo que el reloj rechaza, y el síntoma sería una tarea que no corre nunca.
2. **El mensaje del rechazo lo escribe el módulo.** Los textos de `croner` no reciben la cadena original (`Invalid pattern: …`), y la biblioteca **descarta** su error (`map_err(|_| ParseSchedule)`) cuyo `Display` es su `Debug`. El mensaje de `ErrorDeExpresion` **nombra la expresión** y dice el motivo; la cadena vacía se nombra aparte porque es el valor que llega de verdad de un formulario en blanco.
3. **La fila guarda lo que tecleó el usuario**: `backup_jobs.cron_expression` no se reescribe, y la normalización ocurre **en vuelo**, al construir el job.

**La familia de fechas de la frontera habla `time`.** `croner` y la biblioteca miden el calendario con `chrono`; el §3 y el resto de Kesto lo miden con `time`. Ninguna firma pública del módulo nombra una fecha de `chrono`: el único cruce —`time` → `chrono` → `croner` → `time`— vive en un solo sitio documentado, `Agenda::proxima_tras`, y la superficie sigue hablando `time`.

### La planificación inicial

`Planificador::arrancar` monta el reloj de la biblioteca **sin su persistencia** (`JobScheduler::new()`), construye el interior y ejecuta la **misma** pasada que después corre la reconciliación: lee `backup_jobs`, planifica cada tarea con `enabled = 1` y su expresión, y **no** planifica ninguna con `enabled = 0`. Solo **después** arranca el reloj (`start()`), de modo que la ventana entre «nada planificado» y «todo planificado» se cierra **antes** de que el reloj empiece a tickar y antes de escuchar.

Una tarea cuya expresión no parsea se **registra** —nombrando su `job_id` y su `cron_expression`, que es lo único que la distingue en un registro con veinte tareas—, se **salta**, y **no impide el arranque ni la planificación de las demás**: negarse a arrancar por una fila mala convertiría la errata de una tarea en la ausencia de todas las copias, y dejaría al operador sin el panel con el que arreglarla.

### La reconciliación periódica

`PERIODO_DE_RECONCILIACION = 30 s` —constante declarada **en un solo sitio**, `planificacion.rs`, y no un ajuste de entorno— es lo que acota la latencia entre un cambio en la base y su efecto en el reloj. `Planificador::vigilar` lanza un `tokio::time::interval` que cada período llama a `Planificador::reconciliar`; el primer tick de `interval` —inmediato— se consume sin reconciliar, porque `arrancar` ya cerró la ventana inicial con una pasada.

La sincronización es **periódica y no una notificación desde la API**, y el motivo que decide es que la notificación **no cubre los cambios que no pasan por ella** —una fila editada con `sqlite3`, una base restaurada—, que se quedarían con la planificación vieja para siempre. La reconciliación los recoge sola.

El otro pilar es la **tabla `job_id → mango`** de `Estado`, que es la **única autoridad** sobre lo planificado: la biblioteca no ofrece ninguna señal de estado —`remove` de un mango que no existe devuelve `Ok(())`, `inited()` no distingue «nunca arrancado» de «ya parado», y `add` después de apagar devuelve `Ok(uuid)` sin disparar jamás—. La tabla guarda el **mango** y no un booleano, porque un cambio de `cron_expression` se resuelve **retirando el mango viejo antes de añadir el nuevo**: si el mango viejo se perdiera sin retirarse, la planificación duplicada **no daría ningún error** —simplemente dispararía dos veces—. Una segunda tabla, `expresiones`, recuerda **con qué** forma normalizada se planificó cada tarea, que es lo único que permite distinguir «sigue igual» de «cambió la expresión» sin leerle el calendario a la biblioteca (que no deja).

### La tabla de desenlaces del disparo

`encolar` puede devolver seis desenlaces y cada uno pide una reacción distinta. El criterio que ordena la tabla es **dónde está la causa**:

| Desenlace de `encolar` | Código | Qué hace el planificador | Por qué |
|---|---|---|---|
| `Ok` | — | **sigue** sin cambiar nada | El disparo hizo su trabajo. |
| `YaTieneUnaEnCurso` | 409 | se registra y se **salta** | Reintentar en el acto convertiría una copia larga en una tormenta de intentos; el siguiente tick ya lo intentará. |
| `ColaDelRepositorioLlena` | 503 | se registra y se **salta** | El repositorio está saturado; es transitorio y no queda una `pending` fantasma. |
| `SinQuienAtiendaLaCola` | 503 | se registra y se **salta** | Se arregla mirando el proceso, no la tarea. |
| `TareaQueNoEsta` | 404 | **retira** la tarea de la planificación | Seguir disparando contra un `404` es una tarea fantasma; la reconciliación la habría retirado igual, y esto lo adelanta. |
| `MotorParado` | 503 | **detiene** el planificador | El motor ha dejado de aceptar trabajo: seguir planificando encolaría «casi», que es una carrera con la parada. |
| `Base` | 500 | se registra y **se sigue** | Un `SQLITE_BUSY` de un tick no puede dejar el reloj caído. |

La tabla vive en `disparo.rs` como la función **pura** `decidir(&Result<Ejecucion, ErrorDelMotor>) -> Decision`, para poder probar cada variante **sin contenedor**, incluidas las dos que no se alcanzan en un CI —`ColaDelRepositorioLlena` y `SinQuienAtiendaLaCola`—. Nada se propaga como un pánico: un fallo de un disparo no mata al planificador ni al proceso.

### La parada, y por qué la puerta es del módulo

La parada tiene un matiz que no se deduce del resto. Está medido que el `shutdown()` de la biblioteca **no espera** al disparo en vuelo —vuelve en microsegundos— y que **un tick puede disparar después** de que la parada empiece. Por eso la garantía de que **no se encola tras la parada no se delega en la biblioteca**: la puerta es una **bandera atómica** (`Interior::abierta`) que `Planificador::disparar` consulta **dentro de la misma sección crítica que el encolado** —el candado que comparte con la tabla `job_id → mango`—. Un encolado o termina antes de que la puerta cierre o no empieza, y eso cubre también el tick que la biblioteca dispare de más.

`Planificador::parar(plazo)` cierra la puerta **lo primero de todo y antes de cualquier `await`** —el cierre no depende de adquirir candado alguno—, después drena acotadamente lo que el módulo tuviera en vuelo y apaga el reloj de la biblioteca, y deja la marca `planificador parado`. Las **dos** esperas —la del candado de la tabla y la del reloj— están acotadas por `plazo`: lo que no drene dentro del plazo se registra y el apagado continúa.

El cierre de la puerta **no depende del plazo**: si el candado de la tabla está tomado más que `plazo` —un disparo reteniéndolo durante `encolar` mientras una escritura espera el `BUSY_TIMEOUT` de 5 s—, una puerta que solo se cerrara al adquirirlo se quedaría abierta para siempre —`esta_parado()` seguiría diciendo «no parado», el bucle de `vigilar` no terminaría y un tick posterior encolaría—. Por eso la bandera se pone a `false` **antes de cualquier `await`** y `esta_parado()` la lee sin candado, síncrono y fiable.

`PLAZO_DE_PARADA = 2 s` acota **el drenaje del módulo** —su reconciliación y sus disparos—, **no** la llamada a `shutdown()`, que volvería igualmente en microsegundos. Es menor que la espera de la parada del motor (5 s por defecto) a propósito: las dos caben, una detrás de otra, dentro del período de gracia de un `docker stop` (10 s).

**El orden del arranque** es: config → base abierta y migrada → `Motor::arrancar` → comprobación del socket (que solo **avisa**) → **planificador** → `AppState` → listener. El planificador va detrás del motor porque su única operación es llamarlo; delante del listener porque el requisito 6 exige que esté en marcha cuando el proceso empiece a atender. Y **la parada ordenada** —`apagado_ordenado(motor, planificador, senal)`— es: **planificador primero** (desde la señal no se planifica ni se encola nada nuevo), **cierre del hub** después, y **motor lo último**, tras resolverse `serve`, porque es quien espera de forma acotada a la ejecución en vuelo. Las ejecuciones ya encoladas son del motor y el reloj no las toca. El planificador **no** entra en el `AppState`.

### El bug encontrado y corregido durante la verificación del operador

La fase 4.1 descubrió que el planificador **no creaba ninguna ejecución**. `planificar_una` construía el cierre de disparo con el `Uuid` que la biblioteca entrega —su **mango** interno, el mismo que devuelve `add`— y lo usaba como si fuera el `job_id` de Kesto, así que cada tick acababa en `TareaQueNoEsta` y nunca nacía una fila. Las pruebas automáticas no lo veían porque todas llaman a `Planificador::disparar` **directamente** —el cuerpo del cierre— y ninguna dejaba que el cierre **eligiera el identificador**.

El arreglo: `planificar_una` recibe el `job_id` real y lo **captura en el cierre** (que es donde se conoce), ignorando el mango de la biblioteca —`_mango`—. Y salió una prueba de regresión, `un_tick_real_del_reloj_crea_la_ejecucion_de_su_tarea`, que planifica una tarea `* * * * * *` y espera, con plazo acotado, a que aparezca la fila de **esa** tarea: es la única de todo el módulo que atraviesa el tick de verdad.

### Los dos arreglos de la revisión del código

Dos hallazgos del revisor, corregidos con TDD:

- **La puerta de parada no se cerraba si vencía el plazo.** `parar` cerraba la puerta adquiriendo el candado de la tabla con `timeout(plazo, ..)`: si el candado estaba tomado más que el plazo —un disparo reteniéndolo durante `encolar`—, la rama del `Err` **no** cerraba nada y la puerta quedaba abierta para siempre. La puerta pasó a ser una **bandera atómica** (`Interior::abierta`) que `parar` cierra **antes de cualquier `await`** y que `esta_parado()` lee sin candado; `disparar` la consulta también en la misma sección crítica que el encolado. Pruebas: `la_puerta_queda_cerrada_aunque_el_drenaje_venza_el_plazo` y `un_disparo_tras_una_parada_que_vencio_el_plazo_no_encola`.
- **Mango huérfano si `reloj.remove` falla.** La tabla `planificadas` perdía la entrada **antes** de retirar el mango del reloj: un `remove` que fallara dejaba la tarea planificada y sin nadie capaz de retirarla. Ahora la entrada solo sale con el `Ok` —helper `Estado::aplicar_retirada`, único sitio por el que pasan las tres retiradas—, así que el fallo se reintenta en la siguiente pasada; y en el caso «cambió», la planificación nueva **no** se añade hasta confirmar el `remove` del mango viejo, para no duplicar. El camino del `Err` no es alcanzable con el almacén en memoria de la biblioteca (`SimpleMetadataStore::delete` devuelve `Ok` siempre), así que el invariante se prueba con el error doblado: `una_retirada_que_el_reloj_no_confirma_deja_el_mango_en_la_tabla`.

## Variables y dependencias

**Ninguna variable de entorno nueva.** El §8 no pide ninguna y este change no necesita ninguna: el período de reconciliación y el plazo de parada son constantes declaradas, no ajustes. Para levantarlo en dev siguen bastando las de siempre: `KESTO_MASTER_KEY` (obligatoria siempre), `DATABASE_URL`, `BIND_ADDRESS` y `DOCKER_HOST`; sin ninguna `OIDC_*` el modo es `Disabled` y `/api/v1/health` contesta `auth_mode: "disabled"`.

Las dependencias nuevas, medidas en la fase 0:

| Dependencia | Versión | Nota |
|---|---|---|
| `tokio-cron-scheduler` | **0.15.1** | El reloj del §2. **Una sola copia.** Se usa su API simple (`new`/`add`/`start`/`remove`/`shutdown`), **no** su persistencia. |
| `croner` | **3.0.1** | El mismo analizador que la biblioteca usa por dentro, declarado directo **en el mismo major** (`^3`) para que el resolvedor unifique en una sola copia. |
| `chrono` | **0.4.45** | Ya estaba en el árbol por debajo (`suppaftp` ← `opendal`); pasa a **dependencia directa** porque ni `croner` ni la biblioteca lo reexportan y `Agenda::proxima_tras` no se puede escribir sin nombrarlo. Sigue con **una sola copia**. |
| `chrono-tz` | **0.10.4** | **Nuevo**, arrastre no opcional de la biblioteca; entra por un solo camino. No se usa: la agenda es UTC. |

El diff del `Cargo.lock` es **aditivo** (12 altas, ninguna versión existente cambiada) y arrastra tres duplicados nuevos de terceros que no se unifican a propósito (`num-derive` 0.4.2 junto al 0.5.1; `strum` 0.27.2 junto al 0.28.0). El `Cargo.lock` es **entregable**, porque el CI corre `cargo test --locked` y ninguna de las dos dependencias nuevas está en la caché local.

## Verificación manual

Fase 4.1 **verde**, medida contra el binario de verdad. Entorno: `./target/debug/kesto` en modo dev (`KESTO_MASTER_KEY=…`, `DATABASE_URL=sqlite:///tmp/kesto-dev/kesto.db`, `BIND_ADDRESS=127.0.0.1:8099`, `DOCKER_HOST=unix:///run/user/1000/podman/podman.sock`), S3 contra un servidor **`moto`** local (`http://127.0.0.1:9100`, bucket `kesto-prueba`) y un contenedor Postgres de prueba en Podman (`kesto-pg-prueba`), con un target `postgresql` sobre ese contenedor.

| Qué se comprobó | Lo medido |
|---|---|
| Una tarea dispara sola | Con una tarea de cron `*/10 * * * * *` y `enabled = 1`, **aparecen solas** filas en `job_executions` cada 10 s, todas en `success` (la copia completa: `pg_dumpall` vía Podman → rustic → S3). **6 ejecuciones en 60 s**, cadencia exacta de 10 s. |
| Los eventos | Con `curl -N http://127.0.0.1:8099/api/v1/events` se ven `event: execution_progress` con el payload de **7 campos** del §5; el nacimiento llega con `status:"pending"` y `current_action:"encolada"`. |
| Editar la expresión | Cambiarla a `0 0 3 * * *`: deja de disparar dentro del período de reconciliación (**delta 0 en 40 s** tras el cambio). |
| Desactivar / reactivar | `enabled = 0` deja de disparar (**delta 0 en 40 s**); reactivar vuelve a dispararla. |
| Borrar la tarea | `DELETE` (200) deja de disparar; el siguiente tick que la encuentra retirada la saca de la planificación, con el aviso «la tarea del disparo ya no existe». |
| Parada ordenada | Con `SIGTERM` se registra «planificador parado» **antes** que «motor parado», dentro del plazo. |

### Lo que queda sin cubrir por prueba automática (fase 4.2)

Se declara, no se maquilla:

- **El tick de calendario real** se prueba **forzando el momento** en la suite —se llama a `Planificador::disparar`, que es el cuerpo que el cierre de la biblioteca invoca—, salvo la prueba de regresión del bug, que usa un tick real de **1 s** (`* * * * * *`) con plazo acotado.
- **La recuperación de lo que no corrió con el proceso apagado no existe** por decisión: un tick que cae con el proceso apagado no se recupera al arrancar; el siguiente tick es todo el mecanismo.
- **La parada con un job en vuelo** no se puede provocar si la biblioteca no deja; por eso la garantía de «no se encola tras la parada» se implementó **en el módulo** y se prueba llamando a `disparar` después de `parar`, que es exactamente el tick que la biblioteca puede disparar de más.
- **Riesgo abierto 1 — carrera de doble disparo por `next_tick`/`last_tick`: no medida, no descartada.** La biblioteca actualiza esos dos valores en una tarea aparte, **después** de despachar, mientras la decisión de disparar los lee; si esa actualización se retrasara más de 500 ms, la vuelta siguiente vería los valores viejos y volvería a disparar. **No se consiguió provocar**, así que queda como no medida y no como «no ocurre». Ver `design.md`.
- **Riesgo abierto 2 — *jitter* del tick de hasta 500 ms:** por construcción, la biblioteca duerme 500 ms y mira su flag una vez por vuelta, así que una copia programada puede retrasarse hasta medio segundo sobre su hora. **No se ha cronometrado** disparo a disparo.
- **Deuda A — la columna de origen en `job_executions` que el §3 no tiene.** En la base no se distingue un disparo del reloj de uno manual: son la misma fila, y lo único que los separa es el `current_action` del evento al nacer. Podría deducirse por el momento del disparo, pero eso sería una inferencia y no un dato.
- **Deuda B — la recuperación de ejecuciones perdidas no existe**, y es la misma decisión del segundo punto: no hay política de ejecuciones perdidas.

## Cómo verificarlo y desplegarlo

**Verificación en local** (lo que se hizo en 4.1, y se repite igual):

1. Compilar y arrancar el binario en modo dev con `KESTO_MASTER_KEY`, un `DATABASE_URL` propio —el defecto del §8 apunta al volumen del `compose` y en una máquina de desarrollo no existe—, `BIND_ADDRESS` y el `DOCKER_HOST` del Podman local. Sin `OIDC_*`.
2. Levantar un destino de pruebas —S3 (`moto` sirvió; en `backup-engine` fue SeaweedFS y MinIO ya no es descargable sin credenciales) y un contenedor de base de datos con su cliente de volcado dentro— y dar de alta un repositorio y un target que apunten ahí.
3. Dar de alta una tarea con una expresión que **use los segundos** —por ejemplo `*/10 * * * * *`— y `enabled = 1`. Es lo único que hace observable un tick sin cambiar la hora del sistema.
4. Sin tocar nada, comprobar que aparecen solas las filas en `job_executions` (con `sqlite3` sobre el fichero) y que los eventos se ven con `curl -N` sobre `GET /api/v1/events`.
5. Editar la expresión, poner `enabled = 0`, reactivar y borrar la tarea, midiendo en cada caso que deja o vuelve a disparar tras el período de reconciliación (30 s).
6. Medir la parada con `SIGTERM` y el reloj: «planificador parado» antes que «motor parado».

**Despliegue.** No hay nada que configurar de nuevo: la programación depende solo de lo que ya esté en `backup_jobs` —la expresión y el `enabled` de cada tarea— y de ninguna variable de entorno. Lo único que hay que tener presente es que la agenda se interpreta en **UTC**: `0 3 * * *` son las tres de la mañana UTC, no la hora local del contenedor. El `compose` del §8 tal cual **no arranca** —le falta `KESTO_MASTER_KEY`— y sin `OIDC_*` arrancaría abierto: es modo de desarrollo, no un despliegue expuesto.

## Deuda pendiente y límites aceptados

- **La latencia de sincronización** —hasta `PERIODO_DE_RECONCILIACION`— es el precio de D5: una tarea recién creada no dispara hasta el siguiente ciclo. A cambio, el planificador ve también los cambios que no pasan por la API y la capa HTTP no queda acoplada a él.
- **La agenda es UTC**, así que quien espere que `0 3 * * *` signifique las tres de su hora local se llevará una sorpresa, y este change no la mitiga.
- **La columna de origen no existe** (deuda A): la base no distingue un disparo automático de uno manual.
- **La recuperación de lo que no corrió no existe** (deuda B), ni la de un disparo saltado por estar la tarea en curso.
- **El «próximo disparo» de cada tarea no se publica**: no hay endpoint ni campo que lo exponga, porque hoy no hay consumidor. Llegará con el panel, probablemente en el change que meta el planificador en el `AppState`.
- **Los riesgos 1 y 2** —la carrera de `next_tick`/`last_tick` y el *jitter* del tick— siguen **abiertos**: son de la biblioteca y no del módulo, y quedan declarados en el `design.md` del change.
- **El coste de `chrono`/`chrono-tz`**: el árbol gana una segunda familia de fechas al lado del `time` elegido a conciencia; el módulo no cruza fechas entre familias salvo en un punto documentado.
- **Los tres duplicados nuevos de terceros** (`num-derive`, `strum`) no se unifican: subirlos exige tocar aguas arriba o forzar un `[patch]`, y eso es decisión de diseño.

Este documento es el hermano de `plans/PLAN-006.md` y continúa a `docs/FEATURE-005-backup-engine.md`.
