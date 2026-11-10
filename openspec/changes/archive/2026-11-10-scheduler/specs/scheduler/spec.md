# scheduler Specification

## Purpose
El reloj que faltaba: qué hace que una tarea se ejecute a su hora sin que nadie pulse nada. Cubre la **validación** de la expresión cron —la misma configuración de analizador que después la evaluará, con la normalización de cinco a seis campos y la agenda en UTC—, la **planificación** de las tareas activas al arrancar, la **sincronización** periódica con `backup_jobs` para que un alta, un borrado, una edición del cron o un cambio de `enabled` tengan efecto sin reiniciar el proceso, el **disparo** —una llamada a `Motor::encolar`, la misma puerta que el disparo manual, con la reacción que le toca a cada desenlace— y el **ciclo de vida** del propio planificador: arranca detrás del motor, se para antes que él y no acepta trabajo nuevo desde que la parada empieza. Fuera quedan la extracción del volcado —que es de `extraction`—, el trato con Rustic —de `repository`— y la cola y los trabajadores —de `engine`—, de los que es solo un cliente: no escribe ninguna fila de `job_executions`, no conoce la cola y no guarda tareas por su cuenta, porque la única verdad es la tabla del §3.

## ADDED Requirements

### Requirement: La expresión cron se parsea y se valida, y la agenda se interpreta en UTC

La expresión cron de una tarea SHALL parsearse con el analizador del planificador antes de usarse, SHALL aceptarse tanto con **cinco** campos —sin segundos, como `0 3 * * *`— como con **seis** —con segundos, como `0 0 3 * * *`—, y NO SHALL aceptarse una expresión **vacía**. Una expresión que no parsea SHALL rechazarse con un error que **nombra la expresión** y dice el motivo. La agenda de toda expresión SHALL interpretarse en **UTC**, con independencia de la zona horaria del proceso.

El analizador **del reloj** es el de `croner` **3** construido con **`Seconds::Required`** y **`dom_and_dow(true)`** —la configuración con la que la biblioteca fabrica su parser en los cinco sitios donde lo hace—, y **rechaza** la forma de cinco campos con `ParseSchedule`. Por eso la validación SHALL usar **esa misma configuración** sobre la **forma normalizada**, y el módulo SHALL normalizar de cinco a seis anteponiendo `0 ` **solo cuando el número de campos sea exactamente 5**, sin tocar las de 6 ni las de 7 —una de siete, con año, es válida y no se reescribe—. El analizador **por defecto** de `croner` (`Seconds::Optional`) sí acepta las dos formas, y su normalización de cinco a seis es literalmente `parts.insert(0, "0")`: lo que hace el módulo es reproducirla a propósito, en vez de validar con un analizador que no es el del reloj. Validar con otra gramática aceptaría expresiones que el reloj no entiende, y eso es precisamente el fallo que esta validación existe para evitar.

Que las dos formas sigan siendo válidas es deliberado: el §3 no impone el número de campos y todas las filas y pruebas del repositorio escriben cinco (`0 3 * * *`), así que exigir seis habría dejado inválido lo que ya está guardado. Aceptar las dos formas valida sin romper nada y sin migrar una sola fila, y la normalización ocurre **en vuelo**, al construir el job: `backup_jobs.cron_expression` **no** se reescribe, así que el panel devuelve la expresión que se tecleó.

La cadena vacía se nombra aparte aunque el analizador ya la rechace, porque es el valor que llega de verdad: `cron_expression` es `TEXT NOT NULL` en el §3, así que un formulario que manda el campo en blanco no produce un error de base sino una fila con una cadena vacía —una tarea que existe, se lista, se pinta como programada y no corre nunca—.

El mensaje del rechazo lo escribe **el módulo**, y no es un adorno: los textos de `croner` son del estilo `Invalid pattern: …` y **ninguno recibe la cadena original**, así que el analizador no puede nombrar la expresión; y la biblioteca **descarta** el error del analizador al construir el job —`map_err(|_| JobSchedulerError::ParseSchedule)`—, cuyo `Display` es su `Debug`, así que al operador le llega la palabra `ParseSchedule` y nada más. El mensaje SHALL nombrar la expresión y SHALL decir el motivo.

UTC y no la zona del proceso porque la hora de Kesto es una sola: la base guarda UTC, las fechas del cable son RFC3339 con desplazamiento cero y el orden del listado de ejecuciones ya supone que dos marcas del mismo segundo son el mismo instante. Un reloj en la zona local haría que el momento de las copias cambiara con una variable de entorno del contenedor, sin tocar una línea de código y sin que nadie lo relacione con el cambio.

#### Scenario: Una expresión de cinco campos es válida y su próxima ejecución se calcula en UTC
- **GIVEN** la expresión `0 3 * * *`
- **WHEN** se valida
- **THEN** SHALL aceptarse
- **AND** su próxima ejecución SHALL poder calcularse sobre el calendario en **UTC**
- **AND** NO SHALL depender de la zona horaria del proceso

#### Scenario: La forma de cinco campos que se acepta es la que el reloj sabe leer
- **GIVEN** la expresión `0 3 * * *`, aceptada por la validación
- **WHEN** el planificador construye con ella el job del reloj
- **THEN** el reloj SHALL aceptarlo
- **AND** NO SHALL devolver `ParseSchedule`

#### Scenario: Una expresión de seis campos también es válida
- **GIVEN** la expresión `0 0 3 * * *`
- **WHEN** se valida
- **THEN** SHALL aceptarse
- **AND** su próxima ejecución SHALL ser la misma que la de `0 3 * * *`

#### Scenario: Una expresión que no parsea se rechaza nombrando la expresión y el motivo
- **GIVEN** una expresión que el analizador no entiende
- **WHEN** se valida
- **THEN** SHALL rechazarse
- **AND** el error SHALL nombrar la expresión
- **AND** SHALL decir el motivo

#### Scenario: La cadena vacía se rechaza
- **GIVEN** una expresión vacía
- **WHEN** se valida
- **THEN** SHALL rechazarse
- **AND** el motivo SHALL decir que la expresión está vacía

### Requirement: Al arrancar, se planifican las tareas activas y solo las activas

Al arrancar, el planificador SHALL leer las tareas de `backup_jobs` y SHALL planificar cada una con `enabled = 1`, con su expresión. NO SHALL planificar ninguna tarea con `enabled = 0`. Una tarea cuya expresión no parsea SHALL registrarse como un error que la nombra —con su identificador y con la expresión—, NO SHALL planificarse, y NO SHALL impedir que el planificador arranque ni que las demás tareas se planifiquen.

`enabled` es el interruptor del reloj y no el del botón: una tarea desactivada no se dispara por su `cron_expression`, y sigue pudiéndose disparar a mano —eso lo fija la spec de `engine` y no cambia aquí—.

Que una expresión inválida no impida arrancar es deliberado y sigue el criterio de la configuración: Kesto es el orquestador de las copias, y negarse a arrancar por una fila mala convertiría la errata de **una** tarea en la ausencia de **todas** las copias programadas, además de dejar al operador sin el panel con el que arreglarla. El error se registra con la expresión dentro porque ahí no hay petición ni campo que nombrar, y es lo único que distingue una expresión mala de otra en un registro con veinte tareas.

#### Scenario: Una tarea activa queda planificada
- **GIVEN** una base con una tarea `enabled = 1` y su expresión válida
- **WHEN** el planificador arranca
- **THEN** esa tarea SHALL quedar planificada
- **AND** SHALL tener calculada su próxima ejecución

#### Scenario: Una tarea desactivada no se planifica
- **GIVEN** una base con una tarea `enabled = 0` y su expresión válida
- **WHEN** el planificador arranca y llega la hora de esa expresión
- **THEN** esa tarea NO SHALL quedar planificada
- **AND** NO SHALL crearse ninguna ejecución suya

#### Scenario: Una expresión inválida se salta y las demás siguen
- **GIVEN** una base con tres tareas activas, una de ellas con una expresión que no parsea
- **WHEN** el planificador arranca
- **THEN** el arranque SHALL completarse
- **AND** SHALL quedar registrado el error de la tarea cuya expresión no parsea, nombrando su identificador y la expresión
- **AND** las otras dos SHALL quedar planificadas

### Requirement: La planificación se sincroniza con la base sin reiniciar el proceso

La planificación SHALL sincronizarse con `backup_jobs` sin reiniciar el proceso y SHALL reflejar el **alta** de una tarea nueva, el **borrado** de una existente, la **edición** de su `cron_expression` y el **cambio de `enabled`**. El retraso entre el cambio en la base y su efecto en la planificación SHALL estar acotado por un período declarado y documentado. La sincronización NO SHALL exigir que el cambio haya pasado por la API.

La sincronización es **periódica**, una reconciliación: cada período el planificador lee las tareas y ajusta lo que tiene planificado —añade lo que falta, retira lo que sobra, reemplaza lo que cambió—. La alternativa era que el alta, la edición y el borrado avisaran al planificador, y se descarta por dos motivos. El primero es el acoplamiento: un manejador HTTP no tiene por qué saber que existe un reloj, y avisarlo obligaría a meterlo en el estado de la aplicación y a decidir qué hacer cuando el aviso falla —la fila ya está escrita, el proceso se está apagando, el canal está cerrado—. El segundo es el que decide: el aviso **no cubre los cambios que no pasan por la API** —una fila editada con `sqlite3`, una base restaurada de copia, un `UPDATE` de mantenimiento—, y esos se quedarían con la planificación vieja para siempre, porque nada volvería a avisar. La reconciliación los recoge sola en el siguiente período.

El precio es la latencia acotada que el requisito declara; lo que se compra es que la única forma de desincronizarse sea que el proceso esté parado, y de eso ya se ocupa la planificación inicial.

#### Scenario: Un alta entra en la planificación
- **GIVEN** un planificador en marcha y una tarea dada de alta después
- **WHEN** pasa el período de reconciliación
- **THEN** la tarea nueva SHALL quedar planificada
- **AND** SHALL hacerlo sin reiniciar el proceso

#### Scenario: Un borrado sale de la planificación
- **GIVEN** un planificador con una tarea planificada
- **WHEN** esa tarea se borra de la base y pasa el período de reconciliación
- **THEN** la tarea NO SHALL seguir planificada
- **AND** NO SHALL crearse ninguna ejecución nueva suya

#### Scenario: Un cambio de cron reemplaza la planificación
- **GIVEN** un planificador con una tarea planificada por su expresión
- **WHEN** esa expresión se edita en la base y pasa el período de reconciliación
- **THEN** la próxima ejecución SHALL ser la de la expresión nueva
- **AND** la tarea NO SHALL quedar planificada **dos** veces

#### Scenario: Desactivar retira la planificación y reactivar la devuelve
- **GIVEN** un planificador con una tarea activa planificada
- **WHEN** se pone su `enabled` a `0`, pasa el período de reconciliación, se vuelve a poner a `1` y pasa otro período
- **THEN** con `enabled = 0` la tarea NO SHALL estar planificada
- **AND** con `enabled = 1` SHALL volver a estarlo

### Requirement: Cada disparo encola la tarea en el motor, por la misma puerta que el disparo manual

Llegada la hora, el planificador SHALL disparar la tarea llamando a `Motor::encolar`, que es la misma llamada del disparo manual. NO SHALL crear la fila de `job_executions` por su cuenta, NO SHALL hablar con la cola del motor ni con la biblioteca de copia por otra vía, y NO SHALL publicar el evento del nacimiento de la ejecución. La ejecución SHALL nacer en `pending` con las mismas garantías que la del disparo manual, y NO SHALL llevar ninguna marca que diga que vino del planificador.

Una sola puerta, y no dos, por consecuencia y no por elegancia. `encolar` toma la guarda **atómica** que produce el `409` —una escritura condicional, no una lectura previa—, crea la fila y publica el nacimiento en el hub de eventos. Un planificador que escribiera la fila por su cuenta tendría que reproducir la guarda, y reproducirla mal significa dos ejecuciones `pending` para la misma tarea: una de ellas sin nadie que la ejecute y las dos ocupando el trabajador de su repositorio. Y hereda además los desenlaces ya tipados, que son los que gobiernan el requisito siguiente.

La marca de origen no existe porque el §3 no tiene columna para ella, y este change no añade columnas: en la base, un disparo del reloj y uno del botón son la misma fila. Podría inferirse por el momento del disparo, pero eso sería una deducción y no un dato, y aquí no se presenta como si lo fuera.

#### Scenario: Llegada la hora, la tarea queda con una ejecución en `pending`
- **GIVEN** una tarea planificada con una expresión cuya hora llega
- **WHEN** el planificador la dispara
- **THEN** SHALL quedar una fila nueva en `job_executions`
- **AND** su estado SHALL ser `pending`

#### Scenario: El origen de la ejecución no es distinguible en la base
- **GIVEN** una tarea disparada por el planificador
- **WHEN** se lee su fila de `job_executions` buscando de dónde vino
- **THEN** NO SHALL haber ninguna columna que lo diga
- **AND** la fila SHALL tener los mismos campos que la de un disparo manual

#### Scenario: El nacimiento se publica en el hub como el de un disparo manual
- **GIVEN** un cliente conectado a `GET /api/v1/events`
- **WHEN** el planificador dispara una tarea
- **THEN** el cliente SHALL recibir un evento con el nombre y los campos de siempre
- **AND** NO SHALL hacer falta un evento nuevo para los disparos del planificador

### Requirement: Un disparo que no puede encolar no tumba al planificador

Un disparo que no consigue encolar SHALL seguir esta tabla, y NO SHALL propagarse como un pánico que mate al planificador ni al proceso:

| Desenlace de `encolar` | Código | Qué hace el planificador |
|---|---|---|
| `YaTieneUnaEnCurso` | 409 | se registra y se **salta** |
| `ColaDelRepositorioLlena` | 503 | se registra y se **salta** |
| `SinQuienAtiendaLaCola` | 503 | se registra y se **salta** |
| `TareaQueNoEsta` | 404 | **retira** la tarea de la planificación |
| `MotorParado` | 503 | **detiene** el planificador |
| `Base` | 500 | se registra y se **sigue** |

El criterio que ordena la tabla es **dónde está la causa**. Si está en la tarea —está corriendo, ya no existe— el planificador sigue vivo y lo que cambia es la planificación de esa tarea. Si está en el proceso —el motor ha dejado de aceptar trabajo— el planificador se para, porque seguir planificando no encolaría nada y encolaría «casi», que es una ventana de carrera con la parada. Y todo lo demás se registra y se sigue: un reloj que se cae por el primer error se convierte en la ausencia de todas las copias programadas, que es el peor desenlace posible.

Una tarea ya en curso **se salta y no se reintenta en el acto**: reintentar convertiría una copia larga en una tormenta de intentos contra el mismo repositorio y el mismo trabajador, y el siguiente tick ya lo intentará. Una tarea que ya no está se **retira** en el momento en que se sabe, y no se espera al siguiente período: seguir disparando cada tick contra un `404` es una tarea fantasma que llena el registro y no puede acabar bien. Una cola llena no deja trabajo fantasma a su paso, porque la fila que el motor creó y no pudo encolar la deja él marcada como fallida. Y un error de base en un tick es un fallo de **ese** tick —la tabla puede estar bloqueada un instante por otro escritor—, así que parar por él dejaría al reloj caído por un bloqueo pasajero.

#### Scenario: Una tarea ya en curso no crea una segunda ejecución
- **GIVEN** una tarea planificada que ya tiene una ejecución en curso
- **WHEN** llega su hora
- **THEN** NO SHALL crearse una segunda fila en `job_executions`
- **AND** el planificador SHALL seguir en marcha
- **AND** el siguiente tick SHALL volver a intentarlo

#### Scenario: Una tarea borrada deja de estar planificada
- **GIVEN** una tarea planificada que se borra de la base sin pasar por la API
- **WHEN** llega su hora antes de la siguiente reconciliación
- **THEN** el disparo NO SHALL crear ninguna ejecución
- **AND** la tarea NO SHALL seguir planificada

#### Scenario: Un motor parado acaba con el planificador
- **GIVEN** un planificador en marcha y un motor que ya no acepta trabajo
- **WHEN** llega la hora de una tarea
- **THEN** el planificador SHALL dejar de planificar
- **AND** NO SHALL seguir intentando encolar en cada tick

#### Scenario: Un error de base no impide los siguientes disparos
- **GIVEN** un disparo que falla con un error de base
- **WHEN** llega la hora de la siguiente tarea
- **THEN** el planificador SHALL seguir en marcha
- **AND** el siguiente disparo SHALL intentarse

### Requirement: El planificador arranca detrás del motor y se para antes que él

El arranque SHALL montar el planificador **después** de que el motor esté en marcha y **antes** de empezar a aceptar peticiones. Al empezar la parada, el planificador NO SHALL planificar ni encolar trabajo nuevo, y su parada SHALL estar acotada por un plazo. Lo que no termine dentro de ese plazo SHALL registrarse, y el apagado SHALL continuar hasta el final. El planificador NO SHALL entrar en el estado de la aplicación.

Detrás del motor porque su única operación es llamarlo: uno que arrancara antes tendría una ventana —corta, pero real— en la que el reloj dispara contra un motor que todavía no acepta trabajo, y esos disparos acabarían con el desenlace de «no hay quien atienda» por un motivo que no es cierto.

Al empezar la parada, y no al final: el reloj se para **dentro** del apagado ordenado, antes de cerrar el reparto de eventos y desde luego antes de que `serve` resuelva. Si se parara al final, entre la señal y la parada cabrían ticks que encolarían trabajo contra un motor que está a punto de pararse, que es justo lo que este requisito prohíbe.

Y la parada es **acotada**, pero no porque la biblioteca espere a nada: está medido que su `shutdown()` vuelve en **microsegundos** —3,76 µs con un job durmiendo cinco segundos en vuelo— porque no espera al disparo en curso, que sigue avanzando después y es inobservable, y que un tick puede disparar **después** de que la parada empiece (4 de 4 rondas medidas, con desfases de 610–880 ms). Así que la garantía de que no se encola tras la parada **no se delega en la biblioteca**: es **del módulo**, que SHALL cerrar su puerta en la misma sección crítica que el encolado, de modo que un encolado o termina antes de que la puerta cierre o no empieza —lo que cubre también el tick que la biblioteca dispare después—. El plazo acota **el drenaje del módulo** —su reconciliación y sus disparos—, no la llamada a `shutdown()`, y lo que no termine dentro de él se registra y el apagado continúa: esperarlo sin límite convertiría un `docker stop` en una espera indefinida que el orquestador acaba resolviendo con un `SIGKILL`, el mismo motivo por el que el motor acota la suya.

Por último, el orden completo: el planificador se para **antes** que el motor y no después, y el motor es lo último porque es quien espera de forma acotada a la ejecución en vuelo. Las ejecuciones ya encoladas son del motor, y el reloj no las toca.

#### Scenario: El planificador arranca detrás del motor
- **GIVEN** un arranque con la base abierta y migrada y el motor montado
- **WHEN** el planificador se monta y el proceso empieza a escuchar
- **THEN** el planificador SHALL estar en marcha
- **AND** el motor SHALL estar en marcha antes que él

#### Scenario: La parada del planificador no crea ninguna ejecución nueva
- **GIVEN** un planificador en marcha al que se le pide parar
- **WHEN** transcurre el tiempo que habría hecho disparar una tarea planificada
- **THEN** NO SHALL crearse ninguna ejecución nueva
- **AND** NO SHALL encolarse trabajo nuevo

#### Scenario: El apagado ordenado no se cuelga
- **GIVEN** el proceso en marcha con el planificador y su motor
- **WHEN** llega `SIGTERM`
- **THEN** el proceso SHALL acabar dentro del plazo acotado
- **AND** SHALL hacerlo sin que el período de gracia del orquestador tenga que matarlo con `SIGKILL`
