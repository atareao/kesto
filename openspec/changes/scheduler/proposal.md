# Proposal: scheduler

## Why

`backup_jobs.cron_expression` es una columna que **nadie lee**. El §3 la declara `TEXT NOT NULL` en la tabla de tareas, la API la guarda, la sirve y la edita, y no hay una sola línea del programa que la interprete; `enabled` está en la misma situación: se escribe, se devuelve y no decide nada. Kesto copia cuando alguien pulsa el botón, y ese es todo el gobierno del tiempo que tiene. El §2 del SRS pide un planificador desde el primer día, y `src/scheduler/` **no existe**: es el último de los siete módulos del §7 —`main.rs`, `api`, `db`, `docker`, `engine`, `rustic` y este— que queda por llenar.

Y la pieza que falta es pequeña porque el motor dejó hecha la mitad difícil. `Motor::encolar(job_id)` ya lee la tarea, comprueba que existe, toma la guarda atómica que produce el `409`, crea la fila en `pending`, la mete en la cola de su repositorio y publica el nacimiento en el hub. Un disparo del planificador es **esa misma llamada**: del camino de una copia no hay nada nuevo que escribir, lo que hay que decidir es **cuándo** se llama.

Lo que sí hay que cerrar en el mismo change es una deuda, y es de las que se convierten en fallo silencioso si se construye el planificador sin ella: **`cron_expression` no se valida en ninguna parte**. Ni el alta ni la edición miran su sintaxis —ni el número de campos ni nada—, así que una errata entra en la base, contesta `201` y no la ve nadie. Hoy el daño es una columna con basura; con un planificador encima, el daño es una tarea que **no corre nunca** y un panel que la pinta como programada. Este change trae las dos mitades juntas —el que evalúa y el que valida— porque una sin la otra deja el fallo exactamente donde estaba.

El punto de partida está medido y conviene tenerlo delante: **405 pruebas en verde** —401 en el binario, de las que 20 van `ignored` porque exigen un motor de contenedores, más 4 de integración— y los cuatro portales limpios (`cargo fmt`, `cargo clippy -D warnings`, `cargo test` y `cargo doc`). Lo que entra, por tanto, es un módulo nuevo y una validación, no un arreglo.

## What Changes

**Una capability nueva.**

- **`scheduler`** — el reloj que faltaba. Cubre cinco cosas: la **validación** de la expresión cron —que es lo que el alta y la edición van a invocar—, la **planificación** inicial de las tareas activas al arrancar, la **sincronización** periódica con `backup_jobs` para que un alta, un borrado, una edición del cron o un cambio de `enabled` tengan efecto sin reiniciar el proceso, el **disparo** —una llamada a `Motor::encolar`, la misma puerta del botón manual— con la tabla de desenlaces de cada error, y el **ciclo de vida** del propio planificador: arranca detrás del motor, se para antes que él y no acepta trabajo nuevo desde que la parada empieza.

**Una capability que se modifica.**

- **`api-resources`** — el alta y la edición de una tarea validan su `cron_expression` con la misma validación que usa el planificador. Una expresión inválida contesta `400` con la envoltura y el motivo, y **no escribe la fila**: el alta no deja fila nueva y la edición deja la fila como estaba. Sin esta mitad, el borde HTTP seguiría aceptando lo que el reloj no sabe interpretar, y el fallo aparecería como una tarea que no corre.

Y una consecuencia de dependencias que hay que declarar en vez de descubrir: `tokio-cron-scheduler` 0.15.1 y su `croner` **no están en la caché local de Cargo**, así que el `Cargo.lock` se resuelve **una vez con red** y se commitea. No es un efecto colateral: el CI corre `cargo test --locked` y sin el `Cargo.lock` al día el `preflight` se cae.

## Decisiones tomadas al abrir el change

1. **Alcance: el planificador y la validación, y nada más.** Una capability nueva (`scheduler`) y un delta de `api-resources`. Ni UI ni `Dockerfile`: los dos siguen sin existir y sus guardas del pipeline siguen haciendo su trabajo.
2. **El planificador no usa la API de persistencia de la biblioteca.** `tokio-cron-scheduler` 0.15.1 trae `JobBuilder`, el trait `JobCode` y los almacenes `MetaDataStorage`/`NotificationStore`, y **ninguno** se usa: la verdad de Kesto es SQLite y `backup_jobs` ya guarda la tarea, su expresión y su `enabled`. Un segundo almacén sería una segunda verdad, y la del planificador no la mira nadie. De la biblioteca se usa la mitad que solo mira el reloj: `new()`, `add()`, `start()`, `remove()` y `shutdown()`.
3. **`croner` entra como dependencia directa, en el mismo major que usa la biblioteca** (`^3`). La validación tiene que usar **el mismo** analizador que evaluará la expresión: una segunda gramática —o una copia de otra generación— aceptaría expresiones que el reloj no entiende, que es el fallo que este change viene a evitar.
4. **Cinco o seis campos, con la normalización de cinco a seis en vuelo.** Medido: el analizador **por defecto** de `croner` acepta `0 3 * * *` y `0 0 3 * * *`, pero el que construye la **biblioteca del reloj** —`Seconds::Required` y `dom_and_dow(true)`— **rechaza** los de cinco con `ParseSchedule`. El §3, el §5 y todas las filas y pruebas del repositorio usan **cinco** (`0 3 * * *`), así que exigir seis habría dejado inválido todo lo guardado. La solución no es validar con otra gramática: el **módulo** antepone `0 ` **solo cuando el número de campos es exactamente 5** —lo mismo que hace `croner` con `Optional`, reproducido a propósito—, no toca las de 6 ni las de 7, valida y evalúa con la misma configuración y normaliza **en vuelo**, al construir el job: la fila guarda lo que escribió el usuario y el panel devuelve eso mismo.
5. **La agenda se interpreta en UTC.** La base guarda UTC (spec `db`), las fechas del cable son RFC3339 con desplazamiento cero (spec `api-resources`) y el orden del listado de ejecuciones ya supone que dos marcas del mismo segundo son el mismo instante. Un planificador en la zona del proceso introduciría una tercera lectura de la hora, y es justo la que decide cuándo se copia.
6. **La sincronización es periódica —una reconciliación—, no una notificación desde la API.** El precio es una latencia acotada entre el cambio en la base y su efecto en la planificación; lo que se compra es que un cambio que **no** pasó por la API —una fila editada con `sqlite3`, una base restaurada— también tenga efecto, y que la capa HTTP no sepa que existe un reloj.
7. **El disparo es una llamada a `Motor::encolar`.** El planificador no crea la fila de `job_executions` por su cuenta ni conoce la cola: hereda la guarda atómica del `409` y la publicación del evento en el hub. Consecuencia que se declara: el §3 **no tiene columna de origen**, así que en la base no se distingue un disparo automático de uno manual.
8. **Un disparo que llega con la tarea en curso se salta.** Es la consecuencia de lo anterior: `encolar` contesta `YaTieneUnaEnCurso` y no se reintenta en el acto ni se encola una segunda. Reintentar convertiría una copia larga en una tormenta de intentos, y el siguiente tick ya lo intentará.
9. **Una tarea `enabled = 0` no se planifica; el disparo manual sí la ejecuta.** Lo segundo ya lo fijó `api-endpoints` y está en la spec de `engine`. `enabled` es el interruptor del reloj, no el del botón.
10. **Una expresión ya guardada que no parsea no impide arrancar:** se registra con la tarea y su expresión, esa tarea se salta y las demás se planifican igual. No arrancar por una fila mala convertiría la errata de una tarea en la ausencia de **todas** las copias, que es el mismo criterio que sigue `config` con una credencial floja.
11. **El planificador arranca detrás del motor y se para antes que él.** Nada se planifica ni se encola después de que la parada empiece. La garantía es **del módulo**, no de la biblioteca: medido que su `shutdown()` vuelve en microsegundos sin esperar al disparo en vuelo y que un tick puede disparar **después** de que la parada empiece, así que el módulo cierra su propia puerta en la misma sección crítica que el encolado. El plazo acota **el drenaje del módulo** —su reconciliación y sus disparos—, no la llamada a `shutdown()`: lo que tarde más se registra y el apagado continúa.
12. **El planificador no entra en el `AppState`.** Su mango viaja por `main.rs` a la parada ordenada, al lado del motor. Las rutas no lo necesitan —el alta y la edición llaman a la validación, que es una función y no un estado—, y meterlo en el estado obligaría a que las pruebas de las rutas levantaran un reloj.

## Non-goals

- **La UI y el `Dockerfile`.** `ui/` y `Dockerfile` siguen sin existir.
- **La API de persistencia de la biblioteca.** No se usa y no se va a usar: la verdad es `backup_jobs`.
- **La recuperación de lo que no corrió.** No hay política de ejecuciones perdidas: un tick que cae con el proceso apagado no se recupera al arrancar, y un disparo que se salta por estar la tarea en curso no se reintenta después. El siguiente tick es todo el mecanismo.
- **La zona horaria por tarea.** El §3 no tiene columna para ella y este change no la añade: la agenda es UTC para todas.
- **Una columna de origen en `job_executions`.** No se toca el §3, así que no se podrá distinguir un disparo automático de uno manual mirando la base.
- **El aviso al operador.** Nada manda correos ni notificaciones cuando una copia programada falla; para eso está el panel, que lee el histórico.
- **La captura de errores del propio `shutdown()` de la biblioteca.** Se acota con un plazo y se registra; no se inventa una recuperación.

## Impact

- **Módulo nuevo:** `src/scheduler/` — el último de los siete del §7. Hoy no existe ni el directorio. Dentro vive el reloj, la tabla de lo planificado, la reconciliación, el disparo y **la validación**, que se publica para que el borde HTTP la pueda llamar sin importar la biblioteca de cron.
- **Ficheros tocados:** `Cargo.toml` y `Cargo.lock` (dos dependencias nuevas y su árbol; `chrono` ya está dentro por debajo pero pasa a ser visible al código), `src/main.rs` (el planificador se monta detrás del motor y su mango entra en la parada ordenada) y `src/api/jobs.rs` (la validación en el alta y en la edición).
- **Especificaciones tocadas:** una nueva (`scheduler`) y un delta (`api-resources`, un requisito).
- **Riesgo principal, ya identificado:** el `Cargo.lock`. Es el segundo change con dependencias que hay que resolver con red —el primero fue `backup-engine`— y el CI corre `cargo test --locked`. Se resuelve una vez, se commitea, y la tarea 0.1 lo deja hecho antes de escribir una línea de código.
- **Coste declarado:** `chrono` 0.4 y `chrono-tz` entran como dependencias **no opcionales** de la biblioteca, y el proyecto eligió el crate `time` a conciencia. `chrono` ya está en el árbol por debajo (medido: un solo camino, `suppaftp` ← `opendal-service-ftp` ← `opendal` ← `rustic_backend`); lo que cambia es que pasa a ser una segunda familia de fechas **visible al código** de Kesto, con su propio `Utc` al lado del de `time`. El coste exacto de `chrono-tz` se mide en la fase 0 en vez de estimarse aquí.
