# ui — delta

## MODIFIED Requirements

### Requirement: El Dashboard resume el estado y pinta el avance de la copia en curso

El Dashboard SHALL mostrar tres tarjetas de estadísticas: espacio consumido en S3, número de repositorios y número de tareas activas. El espacio SHALL calcularse sumando los `bytes` de las instantáneas de cada repositorio; cuando el listado de un repositorio venga con `truncated` en `true` **o cuando algún repositorio no se haya podido leer**, la cifra SHALL presentarse como **mínimo** («al menos») y NO SHALL presentarse como total. Las tareas activas SHALL ser las tareas con `enabled` en `true`.

El Dashboard SHALL mostrar un `Progress` con el avance de la ejecución en curso alimentado por SSE, y una tabla de tareas con un `Badge` por estado. El estado de una tarea SHALL ser el de su ejecución más reciente —`success`, `processing` o `error`—, y una tarea sin ejecuciones SHALL pintarse como «sin ejecuciones» y no como correcta. El color NO SHALL ser el único portador del estado: el `Badge` SHALL ir acompañado de su texto.

El resumen del Dashboard NO SHALL ser **todo-o-nada**: cuando un repositorio no responda, las tarjetas que **no** dependen de S3 —el número de repositorios y las de tareas activas— SHALL seguir mostrándose, y el fallo SHALL señalarse en un aviso que **nombre el repositorio que falla**. Ese aviso NO SHALL sustituir las tres tarjetas por un error global, porque un repositorio caído no borra la información de los sanos. La cifra de espacio, si algún repositorio no se pudo leer, SHALL declararse como **mínimo**, reutilizando el mismo rótulo («al menos») que ya usa un listado recortado: una suma parcial NO SHALL presentarse como el total. Cuando todos los repositorios respondan, NO SHALL aparecer ningún aviso de fallo.

La consulta del espacio SHALL acotar su espera y su política de reintento: cuando el repositorio no responda, el Dashboard SHALL alcanzar un estado de **error** con una acción de reintentar, y NO SHALL quedarse en el estado de carga de forma indefinida. Reintentar la consulta por encima del tope del backend (el `retry: 3` por defecto de React Query, sin declararlo) multiplica una espera que ya es larga, así que las consultas que dependen del S3 SHALL llevar un `retry` acotado. La política de reintento global del cliente de consultas NO SHALL cambiarse: el acotado vive en cada consulta que depende del S3, para no alterar el comportamiento de las que no dependen de él.

#### Scenario: Las tarjetas cuentan lo que hay
- **GIVEN** dos repositorios, tres tareas de las cuales dos están activas y un total de instantáneas conocido
- **WHEN** se carga el Dashboard
- **THEN** la tarjeta de repositorios SHALL mostrar `2`
- **AND** la de tareas activas SHALL mostrar `2`
- **AND** la de espacio SHALL mostrar la suma de los bytes de las instantáneas

#### Scenario: Un listado recortado se presenta como mínimo
- **GIVEN** un repositorio cuya respuesta trae `truncated: true`
- **WHEN** se calcula el espacio consumido
- **THEN** la tarjeta SHALL indicar que la cifra es un mínimo
- **AND** NO SHALL presentarla como el total del repositorio

#### Scenario: El avance en vivo manda sobre el último dato del histórico
- **GIVEN** una ejecución en curso con un `percentage` conocido
- **WHEN** llega un evento `execution_progress`
- **THEN** el `Progress` SHALL mostrar el porcentaje del evento
- **AND** la tabla SHALL mostrar esa tarea como `processing`

#### Scenario: Una tarea sin ejecuciones no se pinta como correcta
- **GIVEN** una tarea activa que nunca se ha ejecutado
- **WHEN** se pinta la tabla
- **THEN** su estado SHALL ser «sin ejecuciones»
- **AND** NO SHALL mostrarse con el color de éxito

#### Scenario: La columna «Tarea» muestra el nombre de la tarea y nunca el del target
- **GIVEN** una tarea de nombre `copia nocturna` sobre un target de nombre distinto
- **WHEN** se pinta la tabla «Tareas activas»
- **THEN** la celda de la columna «Tarea» SHALL mostrar `copia nocturna`
- **AND** NO SHALL mostrar el nombre del target ni su identificador

#### Scenario: La columna «Tarea» nunca queda en blanco
- **GIVEN** una tarea con nombre y otra cuyo `name` llega a `null` o a una cadena en blanco —valores que el contrato ya no admite—
- **WHEN** se pinta la tabla
- **THEN** la primera SHALL mostrar su nombre
- **AND** la segunda SHALL mostrar un marcador explícito y atenuado («sin nombre»)
- **AND** ninguna de las dos SHALL mostrar el nombre de su target

#### Scenario: Dos repositorios con uno caído conservan la información del sano
- **GIVEN** dos repositorios, uno que responde y otro que no
- **WHEN** se carga el Dashboard
- **THEN** SHALL mostrarse un aviso que **nombre el repositorio que falla**
- **AND** las tarjetas que no dependen del S3 —repositorios y tareas activas— SHALL seguir mostrando su cifra
- **AND** las tres tarjetas NO SHALL sustituirse por un error global

#### Scenario: La tarjeta de espacio declara un mínimo cuando un repositorio no se pudo leer
- **GIVEN** varios repositorios de los cuales uno no responde
- **WHEN** se calcula el espacio consumido
- **THEN** la tarjeta de espacio SHALL indicar que la cifra es un **mínimo**
- **AND** NO SHALL presentar la suma parcial como el total

#### Scenario: Con todos los repositorios sanos no aparece ningún aviso
- **GIVEN** dos repositorios que responden
- **WHEN** se carga el Dashboard
- **THEN** NO SHALL mostrarse ningún aviso de fallo de repositorio
- **AND** SHALL mostrarse la suma de los bytes como el total

#### Scenario: Un repositorio que no responde da error acotado y no un «cargando» sin fin
- **GIVEN** un repositorio cuyo listado de instantáneas falla —o cuya espera se agota—
- **WHEN** se carga el Dashboard
- **THEN** SHALL alcanzarse un estado de **error** con una acción de reintentar
- **AND** NO SHALL quedarse el resumen en el estado de carga de forma indefinida

#### Scenario: Las consultas dependientes del S3 acotan su reintento
- **GIVEN** una consulta del espacio en S3 y una consulta que no depende del S3
- **WHEN** ambas fallan
- **THEN** la primera SHALL llevar un `retry` acotado
- **AND** la política de reintento global del cliente NO SHALL haberse cambiado

### Requirement: El Histórico pagina, filtra y muestra la consola de la ejecución en vivo

El Histórico SHALL listar las ejecuciones con `GET /api/v1/executions`, SHALL paginar con `limit` y `offset` —`50` por defecto y `200` como tope—, y SHALL filtrar por `job_id` y por `status`. Como la respuesta es un array sin total, la paginación SHALL avanzar cuando la página venga llena y SHALL deshabilitar «siguiente» cuando venga vacía o incompleta; NO SHALL inventarse un total.

Cada fila SHALL mostrar las fechas en hora local, la duración cuando haya `finished_at`, los dos contadores de bytes y el estado con su `Badge` y su texto. La consola SHALL mostrar en vivo el `current_action` y el avance de la ejecución seleccionada a partir del stream SSE, SHALL conservar solo un número acotado de líneas y SHALL indicar cuando la conexión está caída.

Cada fila SHALL mostrar además una columna «Tarea» con el `job_name` de la ejecución y, cuando la tarea no tenga nombre, el identificador de la tarea: nunca el nombre del target. El filtro por tarea SHALL etiquetar cada opción con el nombre de la tarea —o con su identificador si no lo tiene—, y NO SHALL usar el nombre del target, que es lo que hoy hace indistinguibles dos tareas sobre el mismo target.

La consola NO SHALL quedarse vacía ante una ejecución **terminada**. Cuando la ejecución seleccionada ya no vaya a emitir eventos y no haya líneas que mostrar, la consola SHALL pintar un **resumen útil** compuesto a partir de la fila de la ejecución, en vez de un hueco: al menos su `error_message` cuando `status = failed`, y un texto claro que diga que no hay eventos cuando no haya nada que mostrar. El resumen NO SHALL inventar líneas que el panel no recibió, y el historial en vivo del stream NO SHALL persistirse: el log completo de una ejecución —su conservación y su replay— queda fuera del alcance de este contrato.

#### Scenario: La paginación avanza y se detiene al final
- **GIVEN** una primera página con `limit=50` que devuelve 50 filas
- **WHEN** se pide la siguiente página
- **THEN** SHALL pedirse con `offset=50`
- **AND** si la respuesta viene vacía, «siguiente» SHALL quedar deshabilitado

#### Scenario: Los filtros se envían a la API
- **GIVEN** un filtro por `status=error` y una tarea elegida
- **WHEN** se aplica
- **THEN** la petición SHALL llevar `job_id` y `status`
- **AND** la tabla SHALL mostrar solo esas filas

#### Scenario: La consola sigue la ejecución seleccionada
- **GIVEN** una ejecución seleccionada en el Histórico
- **WHEN** llega un evento `execution_progress` de esa ejecución
- **THEN** la consola SHALL añadir su `current_action` y su avance
- **AND** los eventos de otras ejecuciones NO SHALL mezclarse en esa consola

#### Scenario: Sin ejecuciones se muestra un vacío, no un error
- **GIVEN** un histórico sin filas
- **WHEN** se carga la pantalla
- **THEN** SHALL mostrarse el estado vacío
- **AND** NO SHALL mostrarse un error

#### Scenario: La columna Tarea y el filtro usan el nombre de la tarea
- **GIVEN** dos tareas sobre el mismo target, una con nombre y otra sin él
- **WHEN** se carga el Histórico
- **THEN** la tabla SHALL mostrar una columna «Tarea» con el nombre de la primera y el identificador de la segunda
- **AND** el filtro de tarea SHALL etiquetar cada opción con el nombre de la tarea
- **AND** NO SHALL usar el nombre del target para etiquetar las opciones

#### Scenario: La consola de una ejecución fallida terminada muestra su error_message
- **GIVEN** una ejecución con `status = failed` y un `error_message` no nulo, seleccionada en el Histórico
- **WHEN** se abre «Ver consola» y esa ejecución ya no emite eventos
- **THEN** la consola SHALL mostrar el `error_message` de la ejecución
- **AND** NO SHALL mostrarse vacía

#### Scenario: La consola de una ejecución terminada sin líneas muestra un texto claro
- **GIVEN** una ejecución con `status = success` y sin líneas en el historial en vivo
- **WHEN** se abre «Ver consola»
- **THEN** la consola SHALL mostrar un texto que diga que no hay eventos que mostrar
- **AND** NO SHALL inventar líneas ni quedarse en blanco

### Requirement: Snapshots explora un repositorio y dice cuándo el listado está recortado

La pantalla de Snapshots SHALL exigir elegir un repositorio de los que devuelve `GET /api/v1/repositories` y SHALL listar sus instantáneas con `GET /api/v1/snapshots?repository_id=<id>`. La tabla SHALL mostrar el identificador, la fecha en hora local, la etiqueta, los ficheros y los bytes, y SHALL presentar `paths` sin romper la fila. Cuando `truncated` sea `true`, SHALL avisar de que puede haber más instantáneas y NO SHALL presentar la lista como completa.

La tabla SHALL mostrar además una columna «Tarea» **entre la fecha y la etiqueta**, con el `job_name` de la instantánea. Esa columna NO SHALL caer a `label`, ni al identificador de la tarea, ni al nombre del target: «Tarea» y «Etiqueta» NO SHALL mostrar el mismo dato, que es exactamente lo que las hacía indistinguibles —`label` es el identificador de la tarea, así que usarlo como sustituto dejaba las dos columnas diciendo lo mismo—. Cuando `job_name` llegue a `null` —el único caso que queda ahora que toda tarea tiene nombre es la instantánea **huérfana**, la que no casa con ninguna ejecución—, SHALL mostrarse un marcador explícito y atenuado («sin tarea»), no el contenido de `label`. La columna «Etiqueta» NO SHALL tocarse: sigue mostrando el identificador del snapshot.

La pantalla NO SHALL ofrecer una acción de purga manual mientras la API no exponga una ruta para ella: un botón que no puede cumplir lo que promete es peor que su ausencia.

La pantalla NO SHALL quedarse «cargando» de forma indefinida cuando el repositorio elegido no responde: la consulta del listado SHALL acotar su espera y su reintento, y un fallo SHALL llevar al estado de **error** con una acción de reintentar, con el mensaje de la API. Reintentar por encima del tope del backend multiplica una espera que ya es larga, así que la consulta del listado SHALL llevar un `retry` acotado.

#### Scenario: Se elige un repositorio y se listan sus instantáneas
- **GIVEN** dos repositorios dados de alta
- **WHEN** se elige uno
- **THEN** SHALL pedirse `GET /api/v1/snapshots` con su `repository_id`
- **AND** la tabla SHALL mostrar sus instantáneas

#### Scenario: Un listado recortado se avisa
- **GIVEN** una respuesta con `truncated: true`
- **WHEN** se pinta la tabla
- **THEN** SHALL mostrarse un aviso de que puede haber más
- **AND** NO SHALL afirmarse que ésas son todas

#### Scenario: Un repositorio sin instantáneas muestra el vacío
- **GIVEN** un repositorio sin instantáneas
- **WHEN** se consulta
- **THEN** SHALL mostrarse el estado vacío
- **AND** la tabla NO SHALL pintar filas inventadas

#### Scenario: La tabla muestra la tarea entre la fecha y la etiqueta
- **GIVEN** instantáneas listadas, una de una tarea con nombre y otra cuyo vínculo con la tarea falta
- **WHEN** se pinta la tabla
- **THEN** SHALL verse una columna «Tarea» entre la fecha y la etiqueta
- **AND** la primera SHALL mostrar el nombre de su tarea
- **AND** la segunda SHALL mostrar el marcador atenuado, no el identificador de la tarea
- **AND** la columna «Etiqueta» SHALL seguir mostrando el identificador del snapshot

#### Scenario: «Tarea» y «Etiqueta» nunca muestran el mismo valor
- **GIVEN** instantáneas listadas, una con `job_name` y otra cuyo `job_name` llega a `null`
- **WHEN** se pinta la tabla
- **THEN** en cada fila el contenido de «Tarea» SHALL ser distinto del de «Etiqueta»
- **AND** «Etiqueta» SHALL seguir mostrando el identificador del snapshot

#### Scenario: Un repositorio que no responde da error acotado, no un cargando sin fin
- **GIVEN** el repositorio elegido, cuyo listado falla o cuya espera se agota
- **WHEN** se carga la pantalla
- **THEN** SHALL mostrarse el estado de **error** con el mensaje de la API
- **AND** SHALL ofrecerse una acción que repita la petición
- **AND** NO SHALL quedarse la pantalla en el estado de carga de forma indefinida

### Requirement: Cada pantalla tiene estados de carga, error y vacío, y es accesible con teclado

Cada pantalla que lee de la API SHALL mostrar un estado de carga mientras la petición está en vuelo, un estado de error con el `message` de la API y una acción para reintentar cuando la petición falla, y un estado vacío cuando la respuesta es correcta pero no hay filas. NO SHALL mostrar una tabla vacía como si no hubiera datos ni un error como si no hubiera nada.

El estado de carga NO SHALL ser indefinido. Una petición contra un recurso que puede no responder —el repositorio S3— SHALL acotar su espera y su reintento, de modo que **siempre** alcance un desenlace: o datos, o un estado de error con su acción de reintentar. Un «cargando» sin fin NO SHALL ser el único desenlace de una vista.

El panel SHALL ser operable solo con teclado: todo control SHALL ser alcanzable y activable, los campos SHALL tener etiqueta asociada, el foco SHALL entrar en un `Modal` al abrirse y volver al control que lo abrió al cerrarse, y el avance en vivo SHALL anunciarse en una región `aria-live` sin robar el foco. El estado de una tarea NO SHALL depender solo del color y el contraste del texto SHALL cumplir el nivel AA.

El resumen del Dashboard NO SHALL tratar el fallo de un recurso como el fallo de todos: cuando un repositorio no responda, SHALL seguir mostrándose lo que sí se sabe —las cifras que no dependen de él— y SHALL avisarse del fallo nombrando el recurso que falla, en vez de colapsar toda la pantalla en un único error.

#### Scenario: Una petición lenta muestra carga y no un vacío
- **GIVEN** una petición que aún no ha contestado
- **WHEN** se pinta la pantalla
- **THEN** SHALL mostrarse el estado de carga
- **AND** NO SHALL mostrarse el estado vacío

#### Scenario: Un error ofrece reintentar
- **GIVEN** una petición que falla con un `message` de la API
- **WHEN** se pinta la pantalla
- **THEN** SHALL mostrarse ese `message`
- **AND** SHALL ofrecerse una acción que repita la petición

#### Scenario: El modal devuelve el foco a quien lo abrió
- **GIVEN** un formulario abierto desde un botón
- **WHEN** se cierra el formulario con `Esc`
- **THEN** el foco SHALL volver al botón que lo abrió

#### Scenario: El avance se anuncia sin robar el foco
- **GIVEN** una ejecución en curso y el foco en otro control
- **WHEN** llega un evento de avance
- **THEN** el porcentaje SHALL anunciarse en una región `aria-live`
- **AND** el foco NO SHALL moverse

#### Scenario: Una petición que depende del S3 no se queda cargando para siempre
- **GIVEN** una vista cuya petición depende del repositorio S3 y un destino que no responde
- **WHEN** se pinta la vista
- **THEN** la vista SHALL alcanzar un estado de **error** con su acción de reintentar
- **AND** NO SHALL permanecer en el estado de carga de forma indefinida

#### Scenario: El fallo de un repositorio no colapsa la pantalla entera
- **GIVEN** una pantalla que consulta varios recursos y uno de ellos no responde
- **WHEN** se pinta
- **THEN** SHALL seguirse mostrando lo que sí se sabe de los recursos que responden
- **AND** SHALL avisarse del fallo nombrando el recurso que falla
- **AND** NO SHALL sustituirse toda la pantalla por un único error
