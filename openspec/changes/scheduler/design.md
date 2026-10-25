# Design: scheduler

## Contexto

Este change construye la última pieza que el §2 del SRS pide y que no existe: el reloj. Hasta hoy `cron_expression` es una columna que se guarda, se sirve y se edita, y que **nadie interpreta**; `enabled` tampoco decide nada. El contrato está en `openspec/changes/scheduler/specs/` —una capability nueva (`scheduler`) y un delta de `api-resources`— y este documento fija las decisiones de forma que un implementador no tenga que volver a deducirlas.

Dos de las decisiones son de forma y conviene decirlas al principio, porque ordenan todo lo demás:

- **El disparo no construye nada.** Es una llamada a `Motor::encolar(job_id) -> Result<Ejecucion, ErrorDelMotor>`, que es exactamente lo que llama `POST /api/v1/jobs/:id/run`. El planificador no conoce la cola, no conoce la capa de acceso de `job_executions` y no escribe ni una fila por su cuenta.
- **La verdad es SQLite.** La biblioteca trae su propia persistencia y no se usa.

Los hechos de la biblioteca que aparecen aquí **están medidos** contra `tokio-cron-scheduler` 0.15.1 (17-oct-2026), no supuestos. Donde una medición dejó algo abierto, se dice cuál es la tarea que lo mide en vez de estimarlo.

## Estructura de módulos

| Módulo | Capability que implementa | Contenido |
|---|---|---|
| `src/scheduler/` | `scheduler` | El reloj y el mapeo de cada tarea a su mango de la biblioteca, la planificación inicial, la reconciliación periódica, el disparo con su tabla de desenlaces, la parada acotada y **la validación de la expresión**, que se publica hacia fuera. |
| `src/api/jobs.rs` | `api-resources` (delta) | El alta y la edición de una tarea llaman a esa validación antes de escribir. Dos llamadas, ninguna gramática nueva. |
| `src/main.rs` | `scheduler` | El arranque del planificador **detrás** del motor y **delante** del listener, y su parada **antes** que la del motor, dentro de la parada ordenada. |

`src/scheduler/` **no existe hoy**: no hay directorio ni declaración de módulo en `src/main.rs`. Es el único de los siete módulos del §7 sin una sola línea, y este change lo crea. Su reparto interno en ficheros lo decide la implementación —el §7 fija módulos, no ficheros—, con una única condición: **la validación tiene que quedar alcanzable desde `src/api` sin que `src/api` importe `tokio_cron_scheduler`**. Si el borde HTTP tuviera que nombrar la biblioteca, la gramática tendría dos dueños el día que alguien llame a otra cosa.

El planificador **no** entra en el `AppState`. El motor sí está ahí porque tres rutas del §5 existen para dispararlo y seguirlo; de este no hay ninguna ruta que dependa. Su mango viaja por `main.rs` desde donde se construye hasta la parada ordenada, al lado del motor, y las pruebas de las rutas no tienen que levantar un reloj para ejercitar un `POST`.

## Las dos dependencias nuevas, medidas

| Hecho | Medición |
|---|---|
| Versión | `tokio-cron-scheduler` **0.15.1**, publicada el 17-oct-2026 |
| API del reloj | `JobScheduler::new()` → `add(Job) -> Uuid` → `start()` → `remove(&Uuid)` → `shutdown()`; además `next_tick_for_job()` y `time_till_next_job()` |
| API que **no** se usa | `JobBuilder`, el trait `JobCode`, los almacenes `MetaDataStorage` / `NotificationStore` |
| Analizador | depende de **`croner ^3.0.0`**, no del crate `cron` |
| Fechas | depende de **`chrono` 0.4 y `chrono-tz`** de forma **no opcional** |
| Segundos | `croner` acepta **cinco** campos (`0 3 * * *`) y **seis** (`0 0 3 * * *`) |
| Caché local | ni `tokio-cron-scheduler` ni `croner` están en la caché de `~/.cargo/registry`, así que el `Cargo.lock` **necesita red** |
| `chrono` en el árbol | ya está, por debajo: `cargo tree -i chrono` da **un solo** camino, `suppaftp` ← `opendal-service-ftp` ← `opendal` ← `rustic_backend` |

De ahí salen tres cosas que hay que hacer y no solo saber:

1. **El `Cargo.lock` es un entregable.** El CI corre `cargo test --locked` y el árbol nuevo no está en la caché de esta máquina. Se resuelve una vez con red y se commitea, **antes** de escribir código, porque sin él no hay `cargo check` que valga.
2. **`croner` se declara como dependencia directa, en el `^3`.** La validación tiene que usar el mismo analizador que evaluará la expresión. Esconderse detrás de la reexportación de la biblioteca —si la hubiera— ataría la gramática a un detalle de paquetería de un tercero; declarar la misma generación es explícito y no duplica el crate.
3. **`chrono` deja de ser un detalle de dentro.** Ya está en el árbol, pero no está al alcance del código de Kesto: lo usa `suppaftp`, por debajo de `opendal`. A partir de este change hay un `chrono::Utc` en un módulo propio, al lado del `time` que el resto del programa usa. El coste de la segunda familia —y el de `chrono-tz`, que **no** aparece hoy en `Cargo.lock` y entra entero con sus datos— lo mide la fase 0 y no se estima en este documento.

Y una forma que sale de la API y condiciona el módulo: **`add()` devuelve un `Uuid` que es el mango de la tarea**, y `remove(&Uuid)` es lo único que la quita. El planificador tiene que llevar, por tanto, una tabla `job_id → Uuid` propia, y **olvidar el mango viejo al reemplazar**: una tarea cuyo cron cambia se retira por su identificador de entonces y se vuelve a añadir, y si el identificador viejo se pierde sin retirarse, la planificación duplicada no da ningún error —simplemente dispara dos veces—. Es la clase de fallo que no se ve hasta que hay dos ejecuciones por tick.

## Por qué la verdad sigue siendo SQLite y no el almacén de la biblioteca

`tokio-cron-scheduler` 0.15.1 trae una API de persistencia completa: `JobBuilder`, el trait `JobCode` para el código de una tarea guardada, y los almacenes `MetaDataStorage` y `NotificationStore`. **No se usa ninguno**, y el motivo no es el esfuerzo: es que `backup_jobs` **ya es** el almacén.

Una tarea de Kesto tiene target, repositorio, tres reglas de retención, un `enabled` y una expresión, y las cuatro tablas del §3 las edita el panel. Duplicar eso en el almacén de la biblioteca abriría dos verdades: la del panel y la del reloj. El síntoma sería el peor de los posibles —una tarea editada en la base seguiría disparando con la expresión vieja, porque el reloj leería la suya— y no habría forma de saber cuál manda sin leer el código. Además, la persistencia de la biblioteca guarda **lo que la tarea ejecuta**, y aquí lo que la tarea ejecuta es una fila de `backup_jobs` que el motor ya sabe leer.

Lo que se usa es la mitad que no guarda nada: un reloj con tareas en memoria. La consecuencia se declara: **el planificador es estado efímero y se reconstruye al arrancar** a partir de la base, que es lo que hace la planificación inicial del requisito 2.

## La forma del cron: cinco o seis campos, y en UTC

`croner` acepta las dos formas clásicas —`0 3 * * *` sin segundos y `0 0 3 * * *` con ellos—, y eso permite validar **sin romper nada**: el §3 no impone el número de campos, el ejemplo del §5 no lo dice y todas las filas y pruebas del repositorio usan cinco. Exigir seis habría dejado inválido lo que ya está guardado; aceptar cinco y seis no pierde expresividad y no obliga a migrar nada.

La cadena **vacía** se nombra aparte aunque el analizador ya la rechace, porque es el valor que llega de verdad: `cron_expression` es `NOT NULL` en el §3, así que un formulario que manda el campo en blanco no produce un error de base, produce una fila con una cadena vacía —una tarea que existe, se lista, se pinta como programada y **no corre nunca**—. Nombrar el vacío por su nombre convierte ese caso en un motivo que se entiende.

La agenda se interpreta en **UTC**, que es la hora del resto del programa: la base guarda UTC, las fechas del cable son RFC3339 con desplazamiento cero y el orden del listado de ejecuciones ya supone que dos marcas del mismo segundo son el mismo instante. Un reloj en la zona del proceso introduciría una tercera lectura de la hora, y la que decide cuándo se copia es la que peor se depura: en un contenedor, `TZ` es una variable que nadie mira y que cambiaría el momento de las copias sin tocar una línea de código. Que `chrono-tz` venga por arrastre no obliga a usarlo.

## La reconciliación, y por qué no una notificación desde la API

El planificador **reconcilia**: cada cierto período lee las tareas de la base y ajusta lo que tiene planificado —añade lo que falta, retira lo que sobra, reemplaza lo que cambió—. El retraso entre el cambio en la base y su efecto queda **acotado por ese período**, que se declara y se documenta en un solo sitio; el valor lo fija la implementación y no esta spec.

La alternativa era que la API avisara: que el alta, la edición y el borrado le dijeran al planificador «esta tarea cambió». Se descarta por dos motivos, y el segundo es el que decide:

- **Acopla el borde HTTP al reloj.** Un `POST` no tendría que saber que existe un planificador, y para saberlo haría falta meterlo en el `AppState` y decidir qué hacer cuando el aviso falla —la fila ya está escrita, el proceso se está apagando, el canal está cerrado—. Cada uno de esos casos es una compensación: deshacer una escritura que sí valía para que el reloj no quede desincronizado.
- **No cubre los cambios que no pasan por la API.** Una fila editada con `sqlite3` en el contenedor, una base restaurada de copia, un `UPDATE` de mantenimiento: la notificación no los ve y el reloj seguiría con la planificación vieja **para siempre**, porque nada volvería a avisar. La reconciliación los recoge sola en el siguiente período.

El precio es una latencia pequeña y acotada; lo que se compra es que la única forma de desincronizarse sea que el proceso esté parado, y de eso ya se ocupa el arranque.

Una consecuencia que hay que tener escrita: la reconciliación **no reacciona al reloj**. Si una tarea se planifica con un tick a un segundo vista y el período de reconciliación es mayor, el tick no se pierde —lo que está planificado en la biblioteca se dispara igual—, pero un cambio que llegue justo después de un tick podrá tardar hasta un período entero.

## El disparo: una puerta, la del motor

Llegada la hora, el planificador llama a `Motor::encolar(job_id)`. Es la misma puerta que el disparo manual, y no por elegancia sino por consecuencia:

- **Hereda la guarda atómica.** El `409` no lo decide una lectura previa: lo decide `alta_si_no_hay_ninguna_en_curso`, que es una escritura condicional en la capa de acceso. Un planificador que creara la fila por su cuenta tendría que reproducir esa guarda, y reproducirla mal significa dos ejecuciones `pending` para la misma tarea —una de las cuales nadie va a ejecutar y las dos ocupando el worker de su repositorio—.
- **Hereda la publicación del evento.** El nacimiento de la ejecución se publica en el hub de la misma forma, así que un disparo del reloj se ve en el panel exactamente como uno del botón, sin que nadie escriba el mismo código dos veces.
- **Hereda los desenlaces.** Los seis errores de `encolar` ya están tipados con su código HTTP, así que la tabla del requisito 5 se escribe sobre un enum cerrado y no sobre cadenas.

Y una limitación que se declara en vez de disimularse: **el §3 no tiene columna de origen**, así que en la base no se distingue un disparo automático de uno manual. Podría deducirse por el momento —un tick cae en un segundo redondo del calendario— pero eso sería una inferencia, no un dato, y no se va a presentar como si lo fuera. Quien necesite distinguirlos necesitará una columna que este change no añade.

## La tabla de desenlaces del disparo

`encolar` puede devolver seis desenlaces —`TareaQueNoEsta` (404), `YaTieneUnaEnCurso` (409), `MotorParado` (503), `ColaDelRepositorioLlena` (503), `SinQuienAtiendaLaCola` (503) y `Base` (500)— y cada uno pide una reacción distinta:

| Desenlace | Código | Qué hace el planificador | Por qué |
|---|---|---|---|
| `YaTieneUnaEnCurso` | 409 | se registra y se **salta** | La tarea está corriendo. Reintentar en el acto convertiría una copia larga en una tormenta de intentos, y el siguiente tick ya lo intentará. |
| `ColaDelRepositorioLlena` | 503 | se registra y se **salta** | El repositorio está saturado. Es transitorio y el trabajo de la tarea no ha perdido nada: la fila que el motor creó y no pudo encolar la deja marcada como fallida, así que no queda una `pending` fantasma. |
| `SinQuienAtiendaLaCola` | 503 | se registra y se **salta** | Para el operador es el mismo desenlace que el anterior —no hay trabajador— y por eso el motor los nombra con la misma frase. Se arregla mirando el proceso, no la tarea: parar el planificador aquí no arreglaría nada. |
| `TareaQueNoEsta` | 404 | **retira** la tarea de la planificación | La fila ya no está. Seguir disparando cada tick contra un `404` es una tarea fantasma que llena el registro y no puede acabar bien; la reconciliación la habría retirado igual en el siguiente período, y esto lo adelanta al momento en que se sabe. |
| `MotorParado` | 503 | **detiene** el planificador | El motor ha dejado de aceptar trabajo: es el proceso apagándose. Seguir planificando no encolaría nada y encolaría «casi» —una ventana de carrera con la parada—, que es exactamente lo que el requisito 6 prohíbe. |
| `Base` | 500 | se registra y **se sigue** | Un fallo de base en un tic es un fallo de ese tic: la tabla puede estar bloqueada un instante por otro escritor. Detener el planificador por eso dejaría el reloj caído por un `SQLITE_BUSY`. |

El reparto tiene un criterio, y es el que hay que poder leer de un vistazo: **la causa decide si se salta o se para**. Si la causa está en la tarea —está corriendo, ya no existe— el planificador sigue vivo y lo que cambia es la planificación de esa tarea; si la causa está en el proceso —el motor está parado— el planificador se para; y todo lo demás se registra y se sigue, porque un reloj que se cae por el primer error se convierte en la ausencia de todas las copias programadas.

Y nada de esto se propaga como un pánico: un fallo de un disparo no puede matar el hilo del planificador ni el proceso. Es la misma regla que la spec de `engine` exige de un worker con una ejecución que falla, y por el mismo motivo —en un orquestador de copias lo normal es que algo falle—.

## El arranque y la parada

El orden del arranque sigue el que ya tiene el proceso y le añade un escalón: **config → base abierta y migrada → `Motor::arrancar` → planificador → `AppState` → listener**. El planificador se monta detrás del motor porque su única operación es llamarlo: uno que arrancara antes tendría una ventana —corta, pero real— en la que el reloj puede disparar contra un motor que todavía no acepta trabajo, y todos esos disparos acabarían en `SinQuienAtiendaLaCola` por un motivo que no es cierto.

**¿Y el `AppState`, entonces?** El motor está en el estado y el planificador no, y el orden no se rompe por eso: el estado se construye con el motor como campo no opcional, y el planificador se construye aparte y su mango **no** entra en el estado. Por eso su mango viaja por `main.rs` hasta la parada ordenada, que es lo que hoy recibe el motor.

La parada es donde está el matiz que no se deduce del resto. Hoy la forma es `serve(state, apagado_ordenado(motor, shutdown_signal()))` y, **después** de que `serve` resuelva, `motor.parar(espera)`. Con el planificador quedan tres cosas que parar y una sola que puede esperar:

1. **El reloj, al empezar el apagado.** Es lo primero que ocurre dentro del futuro de apagado, antes de cerrar el reparto de eventos: desde ese instante no se planifica ni se encola nada nuevo, que es la razón de que el planificador se pare **dentro** del futuro de apagado y no después de que `serve` resuelva. Si se parara al final, entre la señal y la parada cabrían ticks que encolarían trabajo contra un motor que ya está a punto de pararse.
2. **El reparto de eventos**, como hoy, antes de esperar a las conexiones —el porqué está medido y escrito en la spec de `engine`—.
3. **El motor**, después de que `serve` resuelva: es lo que espera de forma acotada a la ejecución en vuelo, y tiene que ser lo último para que las ejecuciones ya encoladas puedan terminar.

De ahí la frase que resume el requisito 6: **el planificador se para antes que el motor y arranca detrás de él**. Nada se encola después de que la parada empiece, y una ejecución ya encolada sigue siendo del motor y no del reloj.

La parada del reloj está **acotada por un plazo**, y por qué hace falta medirlo antes de decidirlo es lo mismo que en el motor: si la biblioteca espera a un disparo en vuelo, un `shutdown()` sin plazo convierte un `docker stop` en una espera indefinida que el orquestador resuelve con un `SIGKILL`. La tarea 0.4 mide qué hace `shutdown()` con un job en vuelo; el plazo es lo que garantiza el requisito en cualquiera de los dos casos, y lo que no termine dentro de él se registra y el apagado continúa.

## Una expresión que no parsea no impide arrancar

Es la decisión con la que más fácil es equivocarse, porque lo «correcto» parece lo contrario. La planificación inicial lee todas las tareas activas; si una trae una expresión que no parsea —una fila escrita antes de que existiera la validación, o escrita a mano con `sqlite3`—, el planificador **registra el error con la tarea y su expresión, se salta esa tarea y sigue con las demás**.

El argumento es el mismo que el de la configuración: Kesto es el orquestador de las copias, y negarse a arrancar por una fila mala convertiría un error de configuración de **una** tarea en la ausencia de **todas** las copias programadas. El operador tiene que poder levantar el panel —que es donde se ve el problema y donde se arregla editando la tarea—, y un proceso que no arranca no da panel. Es exactamente el criterio de `config` con una credencial de menos de 32 caracteres: avisa y arranca.

Que el error **nombre la expresión** es parte del requisito y tiene una justificación que conviene no confundir con la regla de los mensajes de error de la API. Esa regla —nombrar el campo y nunca el valor— existe por las credenciales; una expresión cron **no es una credencial**, y es además lo único que distingue «una expresión que no parsea» de «otra expresión que no parsea» en un registro donde hay veinte tareas. Por eso hay dos textos y no uno: en el registro del arranque se nombra **la expresión**, porque ahí no hay petición ni campo; en la respuesta HTTP se nombra **el campo** (`cron_expression`), como el resto de los `400` de la superficie de recursos.

## La validación en el borde HTTP

El delta de `api-resources` no añade gramática: añade un sitio donde se aplica la que ya existe. El alta y la edición de una tarea llaman a la validación que publica `src/scheduler` **antes de escribir**, y una expresión inválida contesta `400` con la envoltura y el motivo, sin dejar fila —en el alta— y dejando la fila como estaba —en la edición—.

Por qué en los dos sitios y por qué antes de escribir:

- **Sin la validación del borde**, una expresión con una errata se guarda, contesta `201`, y el fallo aparece mucho después —cuando nadie relaciona la tarea que no corre con el formulario que la creó—. Es el mismo razonamiento que llevó a que el alta de un repositorio hable con el destino: convertir un fallo diferido en un `400` en la misma petición en la que se cometió.
- **Antes de escribir**, porque una fila inválida que se escribe y luego se rechaza es peor que un rechazo: la fila existe, el panel la lista y la reconciliación la saltará en cada pasada, llenando el registro de avisos por un error que ya se había detectado.
- **En el alta y en la edición**, porque la edición es cómo se arregla una expresión mala: si solo validara el alta, una fila mala solo se podría corregir escribiendo otra buena —lo que funciona— pero también se podría empeorar sin límite.

La validación no sustituye a la robustez del planificador: la validación protege de lo que entra por la API y el «se registra y se salta» protege de lo que entra por debajo —una fila escrita con `sqlite3`, una base restaurada de una versión anterior—. Las dos capas existen porque protegen de dos caminos distintos. Es la misma forma que ya tienen la validación de la retención en el borde y el guardarraíl del motor.

## Trampas medidas de la biblioteca

| Trampa | Consecuencia |
|---|---|
| `add()` devuelve un `Uuid` que es el mango, y `remove(&Uuid)` es lo único que retira | Hay que llevar una tabla `job_id → Uuid` propia; un mango perdido deja una tarea planificada que nadie puede retirar y que dispara **dos veces** al reemplazarla |
| La API de persistencia (`JobBuilder`, `JobCode`, `MetaDataStorage`/`NotificationStore`) existe y es cómoda | Usarla duplicaría la verdad: la tarea vive en `backup_jobs` y el panel la edita ahí |
| Depende de `croner ^3`, **no** del crate `cron` | Declarar `cron` o `croner` en otro major daría dos gramáticas: la validación aceptaría lo que el reloj no entiende |
| `croner` acepta segundos opcionales | Es lo que permite validar sin romper nada: cinco campos y seis son ambos válidos |
| `chrono` y `chrono-tz` entran **no opcionales** | El árbol gana una segunda familia de fechas al lado del `time` elegido a conciencia. `chrono` ya estaba por debajo (`suppaftp` ← `opendal`), `chrono-tz` no |
| Ni `tokio-cron-scheduler` ni `croner` están en la caché local de Cargo | El `Cargo.lock` necesita red y es entregable: el CI corre `cargo test --locked` |
| El disparo es `async` y `encolar` también | El job tiene que ser de la variante asíncrona; la 0.2 mide cuál es, en vez de suponerla |
| `shutdown()` con un job en vuelo | Es lo que hay que medir en la fase 0: si espera, la parada del planificador necesita el plazo, no la fe |
| El planificador es estado **efímero** | Al reiniciar no hay nada planificado hasta que la planificación inicial termina: la ventana entre el arranque y el primer tick planificado es real y se cierra antes de escuchar |

## Alternativas descartadas

- **La persistencia de la biblioteca como almacén de las tareas.** Dos verdades: lo que el panel edita en `backup_jobs` y lo que el reloj tiene guardado. El fallo no da error, da una tarea que dispara con la expresión vieja.
- **Avisar al planificador desde el alta, la edición y el borrado.** Acopla el borde HTTP al reloj y no cubre los cambios que no pasan por la API —una fila editada con `sqlite3`, una base restaurada—, que se quedarían sin corregir para siempre.
- **Evaluar el `cron_expression` a mano, con un analizador propio o con el crate `cron`.** Sería una segunda gramática: lo que la validación acepte y lo que el reloj entienda podrían discrepar, y discreparían en los bordes —segundos, días de la semana— que es donde aparecen las expresiones raras.
- **Reintentar en el acto un disparo que se salta.** Convierte una copia larga en una tormenta de intentos contra el mismo repositorio y el mismo worker.
- **Planificar por reloj de pared en la zona del proceso.** El momento de las copias cambiaría con una variable de entorno, sin tocar una línea de código, y la base seguiría guardando UTC.
- **Meter el planificador en el `AppState`.** Ninguna ruta lo necesita —la validación es una función, no un estado— y obligaría a que cada prueba de las rutas levantara un reloj.
- **Parar el planificador después de que `serve` resuelva, como el motor.** Deja una ventana entre la señal y el apagado en la que el reloj sigue encolando trabajo contra un motor que está a punto de pararse.
- **No arrancar si una expresión no parsea.** Convierte la errata de una tarea en la ausencia de todas las copias y deja al operador sin el panel con el que arreglarla.
- **Exigir seis campos.** Deja inválido todo lo guardado: el §3 no impone el número de campos y las filas y pruebas del repositorio usan cinco.

## Decisiones cerradas en la puerta de aprobación

1. **Alcance: el planificador y la validación.** Una capability nueva (`scheduler`) y un delta de `api-resources`. `ui/` y `Dockerfile` siguen sin existir.
2. **La verdad es `backup_jobs`.** La API de persistencia de la biblioteca no se usa; el planificador es estado efímero y se reconstruye al arrancar.
3. **`croner` directo en el `^3`.** El mismo analizador que evaluará la expresión.
4. **Cinco o seis campos**, segundos opcionales; la cadena vacía se rechaza con nombre propio.
5. **UTC** para toda agenda; ninguna columna ni variable introduce una zona por tarea.
6. **Reconciliación periódica** con el período declarado; se acepta la latencia acotada a cambio de no acoplar la API al reloj y de cubrir los cambios que no pasan por ella.
7. **El disparo es `Motor::encolar`**, sin una segunda ruta de escritura; se heredan la guarda del `409` y la publicación del evento, y se declara que el §3 no tiene columna de origen.
8. **Una tarea en curso se salta.** No se reintenta en el acto ni se encola una segunda.
9. **`enabled = 0` no se planifica y sí se dispara a mano** —esto segundo ya lo fijó `api-endpoints`—.
10. **Una expresión ya guardada que no parsea no impide arrancar**: se registra, se salta y las demás siguen.
11. **El planificador arranca detrás del motor y se para antes que él**, dentro del futuro de apagado y con un plazo que acota su parada.
12. **El planificador no entra en el `AppState`**; su mango va por `main.rs` a la parada ordenada.
13. **El reloj no se para por un error de base.** `Base` se registra y se sigue: un `SQLITE_BUSY` de un tick no puede dejar sin copias programadas a todo el despliegue.
14. **El error de una expresión inválida se nombra distinto en cada camino**: la expresión en el registro del arranque, el campo en la respuesta HTTP. La regla de no repetir el valor existe por las credenciales, y una expresión cron no lo es.

## Riesgos abiertos

- **El `Cargo.lock`.** Dos dependencias fuera de la caché local y un CI que corre `cargo test --locked`. Se resuelve con red en la fase 0 y se commitea; sin él no hay ni `cargo check`.
- **El coste de `chrono` y `chrono-tz`.** `chrono` ya está en el árbol por debajo; `chrono-tz` entra entero con sus datos de zonas. La medición de la fase 0 fija el número real en vez de una estimación.
- **La variante asíncrona del job.** El disparo tiene que ser `async` porque `encolar` lo es; si la 0.15.1 solo ofreciera cierres bloqueantes, el disparo tendría que correr fuera del runtime y la forma del módulo cambiaría. Lo mide la tarea 0.2 **antes** de escribir nada.
- **El `shutdown()` con un job en vuelo.** Si espera a un disparo en curso, la parada del planificador depende del plazo para no colgar el apagado. Lo mide la tarea 0.4.
- **La cobertura automática del tick.** Un tick de cron real obliga a esperar a la hora del calendario, así que las pruebas de la planificación y del disparo tienen que poder forzar el momento en vez de esperarlo. Lo que quede sin cubrir se declara por escrito en `docs/FEATURE-006-scheduler.md` en vez de fingirlo.
- **La ventana entre el arranque y la primera planificación.** Nadie planificado es un estado que dura lo que tarde la planificación inicial; se cierra antes de escuchar y por eso el orden del arranque importa.
