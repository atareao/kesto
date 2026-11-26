# ui Specification

## Purpose
El panel web de Kesto: el SPA que convierte la API del §5 en un producto operable —layout y navegación, cliente HTTP y SSE, las pantallas de Dashboard, Tareas, Histórico, Snapshots, Ajustes y sesión— y que fija cómo se comporta cuando los datos llegan, cuando fallan y cuando no hay ninguno.

## Requirements

### Requirement: El layout ofrece las cinco pantallas del §6 y una ruta para cada una

El panel SHALL montarse en un layout con menú lateral y SHALL ofrecer, como mínimo, las rutas `/` (Dashboard), `/tareas`, `/historico`, `/snapshots` y `/ajustes`. Cada entrada del menú SHALL navegar a su ruta sin recargar el documento, la entrada de la ruta activa SHALL distinguirse visualmente, y una ruta desconocida SHALL renderizar una pantalla de «no encontrada» **dentro** del layout, sin perder el menú. En una ventana estrecha el menú SHALL poder plegarse sin dejar ninguna entrada inalcanzable. El control de plegado del menú SHALL residir en el pie de la barra lateral —dentro de ella y a continuación del menú—, no en una cabecera aparte. La barra lateral SHALL montarse solo cuando haya panel (`auth_mode` `disabled` o `oidc` con sesión) y NO SHALL montarse sin sesión.

Las cinco pantallas y sus nombres salen del §6. Las rutas son del cliente y no del servidor: por eso el backend sirve `index.html` como fallback y una recarga en `/tareas` tiene que seguir funcionando. El control que pliega el menú pertenece a la barra que pliega: ponerlo dentro de la barra lateral lo deja donde el operador lo busca y libera la cabecera.

#### Scenario: Cada entrada del menú abre su pantalla
- **GIVEN** el panel cargado en `/`
- **WHEN** se pulsa cada una de las cinco entradas del menú
- **THEN** SHALL mostrarse la pantalla de esa entrada
- **AND** la URL SHALL cambiar a su ruta
- **AND** NO SHALL recargarse el documento

#### Scenario: La ruta activa se marca en el menú
- **GIVEN** el panel abierto en `/historico`
- **WHEN** se pinta el menú
- **THEN** la entrada de Histórico SHALL ser la marcada como activa
- **AND** las demás NO SHALL estarlo

#### Scenario: Una ruta desconocida no rompe el layout
- **GIVEN** una URL del cliente que no corresponde a ninguna pantalla
- **WHEN** se carga el panel
- **THEN** SHALL mostrarse la pantalla de «no encontrada»
- **AND** el menú lateral SHALL seguir visible y operativo

#### Scenario: El control de plegado vive en el pie de la barra lateral
- **GIVEN** el panel con la barra lateral desplegada
- **WHEN** se pinta la barra lateral
- **THEN** el control de plegado SHALL estar dentro de la barra lateral, a continuación del menú
- **AND** SHALL ser alcanzable y activable con el teclado
- **AND** SHALL conservar su nombre accesible y su estado plegado/desplegado

#### Scenario: Sin sesión la barra lateral no se monta
- **GIVEN** el modo `oidc` sin sesión
- **WHEN** se abre cualquier ruta
- **THEN** el menú lateral NO SHALL montarse

### Requirement: El cliente de la API consume la envoltura y no filtra secretos

Toda llamada del panel SHALL dirigirse a `/api/v1/*` en el mismo origen, con credenciales de cookie incluidas, y SHALL interpretar la respuesta según la envoltura de `api-shell`: una respuesta atendida trae `{status, message, data}` y una fallida trae `{status, message}`. El cliente SHALL comprobar que `status` coincide con el código HTTP, SHALL tratar un cuerpo que no sea la envoltura como error de protocolo y NO SHALL inventar un `data` vacío para disimularlo.

Las fechas SHALL presentarse al usuario en la zona horaria local a partir de la cadena RFC3339 que publica la API, sin reinterpretarla como si fuera un instante sin desplazamiento, y los identificadores UUID SHALL conservarse en su forma canónica con guiones en cualquier comparación, clave de lista o URL. El panel NO SHALL mostrar, registrar ni guardar en almacenamiento del navegador ningún campo de secreto; para las credenciales SHALL usar únicamente los booleanos `has_*` que publica la API.

Los mensajes de error SHALL ser el `message` de la envoltura, presentado como texto y nunca como HTML, y NO SHALL incluir el cuerpo de la petición ni el valor de un campo del formulario.

#### Scenario: Una respuesta correcta se entrega como datos
- **GIVEN** una respuesta `200` con `{status: 200, message, data}`
- **WHEN** el cliente la procesa
- **THEN** SHALL devolver `data` a la pantalla
- **AND** NO SHALL mostrar ningún error

#### Scenario: Un error de la API se muestra con su mensaje y sin el cuerpo enviado
- **GIVEN** una respuesta `400` cuyo `message` nombra el campo `cron_expression`
- **WHEN** el cliente la procesa
- **THEN** la pantalla SHALL mostrar ese `message`
- **AND** NO SHALL mostrar el valor que se había escrito en el formulario como parte del error

#### Scenario: Un cuerpo fuera de la envoltura se trata como error de protocolo
- **GIVEN** una respuesta `200` cuyo cuerpo es JSON pero no trae `status` ni `data`
- **WHEN** el cliente la procesa
- **THEN** SHALL producirse un error de protocolo
- **AND** NO SHALL pintarse una pantalla vacía como si la petición hubiera ido bien

#### Scenario: Un 401 devuelve al usuario a la pantalla de sesión
- **GIVEN** una petición cualquiera que contesta `401` con la envoltura
- **WHEN** el cliente la recibe
- **THEN** SHALL descartarse la caché de datos de la sesión
- **AND** SHALL mostrarse la portada pública en `/`

### Requirement: El panel sigue el stream global y refleja el estado en vivo

El panel SHALL abrir **una sola** conexión `EventSource` contra `GET /api/v1/events`, SHALL escuchar el evento `execution_progress` y SHALL leer sus siete campos del §5: `execution_id`, `job_id`, `status`, `bytes_processed`, `bytes_uploaded`, `percentage` y `current_action`. NO SHALL inventar nombres de evento ni campos adicionales.

El estado recibido SHALL reflejarse en el Dashboard y en el Histórico sin recargar la página. Un `percentage` nulo SHALL pintarse como progreso indeterminado y NO SHALL convertirse en `0`, porque el motor lo publica nulo cuando no conoce el total. Cuando la conexión se cierre o falle, el panel SHALL reintentar con una espera creciente y SHALL mostrar el estado de la conexión; al desmontar la última pantalla que lo consume, SHALL cerrar el `EventSource`. NO SHALL abrirse una conexión por pantalla ni una por ejecución.

#### Scenario: Un evento actualiza la ejecución en vivo
- **GIVEN** el panel abierto y una ejecución en curso
- **WHEN** llega un evento `execution_progress` con `status` `running` y `percentage` `40.15`
- **THEN** el Dashboard SHALL mostrar ese porcentaje y ese estado
- **AND** el Histórico SHALL mostrar la misma ejecución actualizada
- **AND** NO SHALL hacer falta recargar la página

#### Scenario: Un porcentaje nulo se pinta como indeterminado
- **GIVEN** un evento con `percentage` nulo
- **WHEN** el panel lo procesa
- **THEN** el indicador SHALL mostrarse indeterminado
- **AND** NO SHALL mostrarse `0` ni una barra llena

#### Scenario: Una conexión caída se reintenta
- **GIVEN** una conexión SSE abierta
- **WHEN** el servidor cierra la conexión
- **THEN** el panel SHALL marcar la conexión como perdida
- **AND** SHALL reintentar con una espera creciente
- **AND** al volver SHALL seguir procesando eventos

#### Scenario: Solo hay una conexión abierta
- **GIVEN** el Dashboard y el Histórico montados a la vez
- **WHEN** se cuentan las conexiones al stream
- **THEN** SHALL haber exactamente una

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

### Requirement: Tareas permite dar de alta, editar, disparar y borrar sin salir del panel

La pantalla de Tareas SHALL listar las tareas y SHALL permitir crear y editar en un `Modal` con `Form`, borrar con confirmación y disparar manualmente cada tarea. El formulario SHALL exigir `target_id`, `repository_id`, `cron_expression` y las tres reglas de retención, y SHALL usar `Select` para el target y el repositorio. El campo `container_name` del target SHALL ser un `AutoComplete` que sugiere los contenedores ya usados por los targets existentes y admite texto libre; NO SHALL introducirse un endpoint de contenedores en este change.

El formulario SHALL incluir además el campo de texto `name` («Nombre»), **obligatorio** y con la misma regla que el servidor: no vacío tras recortar, con un tope de **120** caracteres en el propio campo. El panel NO SHALL enviar una tarea sin nombre —el `400` del servidor es la red, no la primera línea— y NO SHALL omitir la clave cuando el campo esté vacío: una tarea sin nombre ya no es un estado válido.

La tabla de Tareas SHALL mostrar una columna «Nombre» con el `name` de cada fila y, como **red de seguridad** ante un `null` o una cadena en blanco —valores que el contrato ya no admite pero que un panel que hable con un backend anterior todavía puede recibir—, un marcador explícito y atenuado («sin nombre»): un hueco en blanco no distingue «no tiene nombre» de «no se pintó», y el nombre del target NO SHALL usarse como sustituto, que es justo la confusión que este campo viene a resolver.

La expresión cron SHALL poder construirse con un generador visual de **seis** campos —segundo, minuto, hora, día del mes, mes y día de la semana— y SHALL poder editarse como texto. La validación del cliente SHALL ser una ayuda y NO SHALL sustituir a la del servidor: un `400` que nombre `cron_expression` SHALL mostrarse en el campo. Las tres reglas de retención SHALL ser `InputNumber` no negativos y el formulario NO SHALL enviar una tarea con las tres a cero.

Los rechazos del guardado SHALL repartirse **por el campo que nombran**, y el `409` del nombre duplicado SHALL mostrarse **en el formulario**, sin cerrarlo: el `400` que nombra `name` SHALL señalarse en el campo «Nombre» y el `409` en el mismo sitio —es su nombre el que está en uso—, mientras que un `400` que nombre `cron_expression` sigue señalándose en la expresión cron. Señalar el nombre rechazado sobre la expresión cron manda a corregir el sitio equivocado, que es peor que no señalarlo.

Cuando haya que crear un target o un repositorio desde el propio formulario, las credenciales —`db_password`, `secret_access_key` y `passphrase`— SHALL escribirse con `Input.Password`, NO SHALL prerrellenarse nunca y, al editar, dejarlas vacías SHALL significar «conservar la que hay».

El disparo manual SHALL llamar a `POST /api/v1/jobs/:id/run`. Un `202` SHALL incorporar la ejecución devuelta con estado `pending` y refrescar el Histórico; un `409` SHALL explicar que la tarea ya tiene una ejecución en curso; un `404` SHALL retirar la tarea de la lista tras refrescarla. Ese `409` —ejecución en curso— es de la **acción de disparar** y NO SHALL confundirse con el del nombre duplicado, que es del **guardado** y se muestra en el formulario.

#### Scenario: El alta envía los campos del §3 y refresca la lista
- **GIVEN** un formulario con target, repositorio, cron `0 0 3 * * *`, retención `7/4/6`, nombre y `enabled` marcado
- **WHEN** se guarda
- **THEN** SHALL enviarse `POST /api/v1/jobs` con esos campos
- **AND** con un `201` la lista SHALL mostrar la tarea nueva

#### Scenario: El generador produce una expresión de seis campos
- **GIVEN** el generador puesto a las 03:00 de todos los días
- **WHEN** se aplica al formulario
- **THEN** el campo `cron_expression` SHALL contener `0 0 3 * * *`
- **AND** SHALL poder enviarse sin edición manual

#### Scenario: Una retención que no conserva nada no se envía
- **GIVEN** un formulario con `keep_daily`, `keep_weekly` y `keep_monthly` a cero
- **WHEN** se intenta guardar
- **THEN** el formulario SHALL impedir el envío
- **AND** SHALL explicar que al menos una regla tiene que conservar algo

#### Scenario: Un cron rechazado por el servidor se señala en su campo
- **GIVEN** un formulario con una expresión que el servidor rechaza con `400` nombrando `cron_expression`
- **WHEN** llega la respuesta
- **THEN** el mensaje SHALL mostrarse junto al campo `cron_expression`
- **AND** el formulario NO SHALL cerrarse

#### Scenario: El disparo manual distingue el 202 del 409
- **GIVEN** una tarea sin ejecución en curso
- **WHEN** se pulsa «ejecutar» y el servidor contesta `202`
- **THEN** SHALL mostrarse la ejecución encolada como `pending`
- **AND** con un `409` SHALL mostrarse que ya hay una ejecución en curso

#### Scenario: Las credenciales se escriben enmascaradas y no se prerrellenan
- **GIVEN** el formulario de alta de un repositorio desde la pantalla de Tareas
- **WHEN** se pinta el campo `passphrase`
- **THEN** SHALL ser un `Input.Password` vacío
- **AND** al editar un repositorio existente, dejarlo vacío NO SHALL borrar la credencial guardada

#### Scenario: El formulario acepta un nombre y la tabla lo muestra
- **GIVEN** el formulario de alta de una tarea
- **WHEN** se escribe `copia nocturna` en el campo «Nombre» obligatorio y se guarda
- **THEN** la petición SHALL llevar `name` con ese valor
- **AND** la tabla de Tareas SHALL mostrar una columna «Nombre» con ese valor en la fila de la tarea

#### Scenario: El formulario no envía una tarea sin nombre
- **GIVEN** el formulario de alta de una tarea con el campo «Nombre» vacío o con solo espacios
- **WHEN** se intenta guardar
- **THEN** el formulario SHALL impedir el envío
- **AND** SHALL explicar que el nombre es obligatorio
- **AND** NO SHALL enviarse ninguna petición de alta

#### Scenario: Un 400 del nombre se señala en el campo del nombre
- **GIVEN** un formulario cuyo guardado es rechazado por el servidor con un `400` que nombra `name`
- **WHEN** llega la respuesta
- **THEN** el mensaje SHALL mostrarse junto al campo «Nombre»
- **AND** NO SHALL mostrarse junto a la expresión cron
- **AND** el formulario NO SHALL cerrarse

#### Scenario: Un 409 del nombre se muestra en el formulario
- **GIVEN** un formulario cuyo guardado es rechazado con `409` porque ese nombre ya está en uso
- **WHEN** llega la respuesta
- **THEN** el rechazo SHALL mostrarse en el formulario, señalando el campo «Nombre»
- **AND** el formulario NO SHALL cerrarse
- **AND** NO SHALL confundirse con el `409` de una ejecución en curso del disparo manual

#### Scenario: La celda «Nombre» nunca queda en blanco
- **GIVEN** una tarea con nombre y otra cuyo `name` llega a `null` —un valor que el contrato ya no admite—
- **WHEN** se pinta la tabla
- **THEN** la primera SHALL mostrar su nombre
- **AND** la segunda SHALL mostrar un marcador explícito y atenuado
- **AND** ninguna de las dos SHALL mostrar el nombre de su target

### Requirement: El Histórico pagina, filtra y muestra la consola de la ejecución en vivo

El Histórico SHALL listar las ejecuciones con `GET /api/v1/executions`, SHALL paginar con `limit` y `offset` —`200` como tope— y SHALL filtrar por `job_id` y por `status`. La pantalla SHALL usar el paginador de Ant Design alimentado por el `total` de la respuesta: SHALL mostrar el total de elementos, el número de páginas y un selector de tamaño de página con **`10` por defecto** y las opciones `10`, `20` y `50`. Al cambiar de página o de tamaño SHALL pedirse la página al servidor con el `offset` correspondiente (`(página − 1) × tamaño`); NO SHALL paginarse en cliente y NO SHALL inventarse un total: el total SHALL ser el que devuelve la API.

Cada fila SHALL mostrar las fechas en hora local, la duración cuando haya `finished_at`, los dos contadores de bytes y el estado con su `Badge` y su texto. La consola SHALL mostrar en vivo el `current_action` y el avance de la ejecución seleccionada a partir del stream SSE, SHALL conservar solo un número acotado de líneas y SHALL indicar cuando la conexión está caída.

Cada fila SHALL mostrar además una columna «Tarea» con el `job_name` de la ejecución y, cuando la tarea no tenga nombre, el identificador de la tarea: nunca el nombre del target. El filtro por tarea SHALL etiquetar cada opción con el nombre de la tarea —o con su identificador si no lo tiene—, y NO SHALL usar el nombre del target, que es lo que hoy hace indistinguibles dos tareas sobre el mismo target.

La consola NO SHALL quedarse vacía ante una ejecución **terminada**. Cuando la ejecución seleccionada ya no vaya a emitir eventos y no haya líneas que mostrar, la consola SHALL pintar un **resumen útil** compuesto a partir de la fila de la ejecución, en vez de un hueco: al menos su `error_message` cuando `status = failed`, y un texto claro que diga que no hay eventos cuando no haya nada que mostrar. El resumen NO SHALL inventar líneas que el panel no recibió, y el historial en vivo del stream NO SHALL persistirse: el log completo de una ejecución —su conservación y su replay— queda fuera del alcance de este contrato.

#### Scenario: El paginador muestra el total y el número de páginas
- **GIVEN** una respuesta con `total` de 120 ejecuciones
- **WHEN** se carga el Histórico con `10` por página
- **THEN** el paginador SHALL mostrar el total de 120 elementos y 12 páginas
- **AND** SHALL pedirse la primera página con `limit=10&offset=0`

#### Scenario: Cambiar de página pide el offset correspondiente
- **GIVEN** el Histórico con 10 elementos por página
- **WHEN** se salta a la página 3
- **THEN** SHALL pedirse `GET /api/v1/executions` con `limit=10&offset=20`
- **AND** la tabla SHALL mostrar esa página

#### Scenario: Cambiar el tamaño de página vuelve a la primera página
- **GIVEN** el Histórico en una página distinta de la primera, con 10 por página
- **WHEN** se elige `50` en el selector de tamaño
- **THEN** SHALL pedirse el listado con `limit=50&offset=0`

#### Scenario: La paginación avanza y se detiene al final
- **GIVEN** una respuesta con `total` de 30 filas
- **WHEN** se salta a la última página
- **THEN** el paginador NO SHALL ofrecer una página siguiente
- **AND** la petición SHALL llevar el `offset` de esa última página

#### Scenario: Los filtros se envían a la API
- **GIVEN** un filtro por `status=error` y una tarea elegida
- **WHEN** se aplica estando en una página posterior a la primera
- **THEN** la petición SHALL llevar `job_id` y `status` y partir de la primera página
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

La pantalla de Snapshots SHALL exigir elegir un repositorio de los que devuelve `GET /api/v1/repositories` y SHALL listar sus instantáneas con `GET /api/v1/snapshots?repository_id=<id>`, paginando con `limit` y `offset`. La tabla SHALL mostrar el identificador, la fecha en hora local, la etiqueta, los ficheros y los bytes, y SHALL presentar `paths` sin romper la fila. SHALL usar el paginador de Ant Design alimentado por el `total` de la respuesta, con el total de elementos, el número de páginas y un selector de tamaño con `10` por defecto y las opciones `10`, `20` y `50`; al cambiar de página o de tamaño SHALL pedirse la página al servidor y NO SHALL paginarse en cliente. Cuando `truncated` sea `true`, SHALL seguir avisando de que puede haber más instantáneas de las contadas y NO SHALL presentar la lista como completa.

La tabla SHALL mostrar además una columna «Tarea» **entre la fecha y la etiqueta**, con el `job_name` de la instantánea. Esa columna NO SHALL caer a `label`, ni al identificador de la tarea, ni al nombre del target: «Tarea» y «Etiqueta» NO SHALL mostrar el mismo dato, que es exactamente lo que las hacía indistinguibles —`label` es el identificador de la tarea, así que usarlo como sustituto dejaba las dos columnas diciendo lo mismo—. Cuando `job_name` llegue a `null` —el único caso que queda ahora que toda tarea tiene nombre es la instantánea **huérfana**, la que no casa con ninguna ejecución—, SHALL mostrarse un marcador explícito y atenuado («sin tarea»), no el contenido de `label`. La columna «Etiqueta» NO SHALL tocarse: sigue mostrando el identificador del snapshot.

La pantalla NO SHALL ofrecer una acción de purga manual mientras la API no exponga una ruta para ella: un botón que no puede cumplir lo que promete es peor que su ausencia.

La pantalla NO SHALL quedarse «cargando» de forma indefinida cuando el repositorio elegido no responde: la consulta del listado SHALL acotar su espera y su reintento, y un fallo SHALL llevar al estado de **error** con una acción de reintentar, con el mensaje de la API. Reintentar por encima del tope del backend multiplica una espera que ya es larga, así que la consulta del listado SHALL llevar un `retry` acotado.

#### Scenario: Se elige un repositorio y se listan sus instantáneas
- **GIVEN** dos repositorios dados de alta
- **WHEN** se elige uno
- **THEN** SHALL pedirse `GET /api/v1/snapshots` con su `repository_id` y su paginación
- **AND** la tabla SHALL mostrar sus instantáneas

#### Scenario: El paginador muestra el total de instantáneas
- **GIVEN** una respuesta con `total` de 45 instantáneas
- **WHEN** se pinta la tabla con `10` por página
- **THEN** el paginador SHALL mostrar el total de 45 elementos y 5 páginas
- **AND** SHALL pedirse la página con `limit=10&offset=0`

#### Scenario: Cambiar de página pide el offset al servidor
- **GIVEN** el listado de instantáneas con 10 por página
- **WHEN** se salta a la página 2
- **THEN** SHALL pedirse el listado con `limit=10&offset=10`

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

### Requirement: La sesión se adapta al modo de autenticación y no llama a rutas que no existen

En modo de desarrollo (`auth_mode` `disabled`) el panel SHALL entrar directamente al Dashboard, NO SHALL pedir `GET /api/v1/auth/session` y NO SHALL llamar a ninguna ruta `/api/v1/auth/*`, que en ese modo contesta `404`.

En modo OIDC el panel SHALL consultar `GET /api/v1/auth/session` al arrancar. Si contesta `200`, SHALL mostrar el `sub` y el `email` de la sesión en la barra lateral. Si contesta `401`, SHALL mostrar la portada pública en `/` con un botón que navegue a `GET /api/v1/auth/login` en el mismo documento —la redirección la hace el servidor— y NO SHALL reintentar la petición en bucle. El login SHALL ofrecerse desde la portada pública (`/`). Sin sesión, las rutas protegidas SHALL desviarse a `/`. El cierre de sesión SHALL llamarse desde el pie de la barra lateral, junto al conmutador de tema, SHALL llamar a `POST /api/v1/auth/logout`, SHALL vaciar la caché de consultas y SHALL volver a `/`.

#### Scenario: En desarrollo no se pide sesión
- **GIVEN** un `GET /api/v1/health` que declara `auth_mode: disabled`
- **WHEN** se carga el panel
- **THEN** SHALL mostrarse el Dashboard
- **AND** NO SHALL pedirse `GET /api/v1/auth/session`

#### Scenario: Con sesión se muestra la identidad
- **GIVEN** una sesión OIDC válida con `sub` y `email`
- **WHEN** se abre el panel con sesión
- **THEN** SHALL mostrarse el `sub` y el `email` en la barra lateral
- **AND** NO SHALL mostrarse ningún token

#### Scenario: Sin sesión se ofrece entrar
- **GIVEN** el modo OIDC y un `401` de `GET /api/v1/auth/session`
- **WHEN** se carga el panel
- **THEN** SHALL mostrarse la portada pública en `/`
- **AND** el botón de entrada SHALL navegar a `/api/v1/auth/login`
- **AND** NO SHALL repetirse la petición de sesión en bucle

#### Scenario: Sin sesión las rutas protegidas desvían a la portada
- **GIVEN** el modo OIDC sin sesión
- **WHEN** se abre una ruta protegida distinta de `/`
- **THEN** SHALL navegarse a `/`
- **AND** NO SHALL navegarse a `/sesion`

#### Scenario: Al salir se vacía la caché
- **GIVEN** una sesión abierta con datos cargados
- **WHEN** se cierra la sesión desde el pie de la barra lateral y el servidor contesta
- **THEN** SHALL volverse a `/`
- **AND** las consultas cacheadas NO SHALL seguir visibles

### Requirement: Ajustes describe el despliegue y gestiona las fuentes y los destinos

La pantalla de Ajustes SHALL mostrar la versión y el `auth_mode` que publica `GET /api/v1/health`, sin pedir ni mostrar ningún secreto. SHALL permitir gestionar los targets y los repositorios con las rutas existentes —listar, crear, editar y borrar—, y en esos formularios `db_password`, `secret_access_key` y `passphrase` SHALL escribirse con `Input.Password`, NO SHALL prerrellenarse y, al editar, dejarlas vacías SHALL conservar la credencial. La pantalla SHALL mostrar para cada fila si la credencial está configurada, a partir de los booleanos `has_*`, y NO SHALL publicar el valor.

#### Scenario: La salud describe el despliegue sin secretos
- **GIVEN** un panel conectado
- **WHEN** se abre Ajustes
- **THEN** SHALL mostrarse la versión y el `auth_mode`
- **AND** el cuerpo de la respuesta NO SHALL contener ningún secreto

#### Scenario: Una credencial configurada se anuncia sin revelarse
- **GIVEN** un repositorio con `has_passphrase: true`
- **WHEN** se pinta su fila
- **THEN** SHALL indicarse que la passphrase está configurada
- **AND** NO SHALL mostrarse su valor

#### Scenario: Editar sin escribir la credencial la conserva
- **GIVEN** un repositorio con passphrase guardada
- **WHEN** se edita el repositorio dejando el campo vacío
- **THEN** el cuerpo enviado NO SHALL incluir `passphrase`
- **AND** la credencial guardada SHALL seguir configurada

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

### Requirement: El formulario de fuente ofrece SQLite y sus campos propios

El selector de tipo de fuente de la pantalla de Ajustes SHALL ofrecer los cuatro valores `directory`, `mariadb`, `postgresql` y `sqlite`. Al elegir `sqlite` SHALL mostrarse `container_name` y `db_path`, y SHALL ocultarse `host_path`, `db_user`, `db_password` y `db_name`. Los tipos de la API en TypeScript SHALL incluir `'sqlite'` en la unión del tipo de target y `db_path` en las interfaces de lectura y de alta. El campo `db_path` SHALL ser un texto y NO SHALL tratarse como secreto.

El porqué: el formulario es el espejo del contrato de la API y del `CHECK` de la base. Si el selector no ofrece `sqlite`, nadie puede dar de alta una fuente SQLite desde el panel, y la feature queda a medias por la puerta de delante.

#### Scenario: El selector ofrece SQLite
- **GIVEN** un panel conectado y el formulario de fuente abierto
- **WHEN** se despliega el selector de tipo
- **THEN** SHALL ofrecerse `directory`, `mariadb`, `postgresql` y `sqlite`

#### Scenario: Elegir sqlite muestra solo sus campos
- **GIVEN** el formulario de fuente con el tipo `sqlite` elegido
- **WHEN** se pintan los campos
- **THEN** SHALL verse `container_name` y `db_path`
- **AND** NO SHALL verse `host_path`, `db_user`, `db_password` ni `db_name`

#### Scenario: Una fuente sqlite se da de alta desde el panel
- **GIVEN** el formulario relleno con un `container_name` y un `db_path` válidos
- **WHEN** se envía el alta
- **THEN** SHALL llamarse a la API con `target_type = 'sqlite'` y el `db_path`
- **AND** la fuente SHALL aparecer en el listado

### Requirement: El panel declara su identidad de marca

El panel SHALL servir los assets de marca de Kesto y SHALL declararlos en el documento. En concreto SHALL servirse el favicon (`favicon.ico`, `favicon-16x16.png`, `favicon-32x32.png`, `favicon-48x48.png`, `favicon-64x64.png`), el icono para dispositivos Apple (`apple-touch-icon.png`), los iconos de Android (`android-chrome-192x192.png`, `android-chrome-512x512.png`), el icono enmascarable (`maskable-icon-512x512.png`), el icono de Windows (`mstile-150x150.png`) y el manifiesto (`site.webmanifest`). El documento SHALL declarar los `<link>` de iconos, el `<link rel="manifest">`, la meta `theme-color` con el azul de marca y las meta de Open Graph, y el menú lateral SHALL mostrar el logo de Kesto además del nombre cuando la barra lateral está desplegada. Con la barra lateral plegada el nombre NO SHALL renderizarse y el logo SHALL permanecer visible. Además, la portada pública SHALL mostrar el logo de Kesto, un único asset con fondo transparente legible en el tema activo, además de la marca de la barra lateral.

El porqué: hoy la pestaña del navegador no identifica a Kesto —no hay favicon ni manifest— y la marca del panel es solo texto. Una aplicación que el operador deja abierta todo el día debe poder reconocerse en una pestaña y en un marcador. Con la barra plegada el nombre no cabe y solo estorba: retirarlo deja el logo limpio y centrado. La portada pública es la primera impresión de Kesto para quien todavía no ha entrado, así que reutiliza el mismo logo, legible en cualquier tema.

#### Scenario: La pestaña identifica a Kesto
- **GIVEN** el panel cargado en el navegador
- **WHEN** se lee el documento servido
- **THEN** SHALL declararse un `<link rel="icon">` y un `<link rel="manifest">`
- **AND** el favicon y el manifest SHALL servirse desde la raíz del panel

#### Scenario: El menú lateral muestra el logo
- **GIVEN** el panel con el layout desplegado
- **WHEN** se pinta el menú lateral
- **THEN** SHALL verse el logo de Kesto
- **AND** el logo SHALL ser legible en el tema activo

#### Scenario: La marca se oculta al plegar
- **GIVEN** el panel con la barra lateral plegada
- **WHEN** se pinta la marca del panel
- **THEN** el nombre «Kesto» NO SHALL renderizarse
- **AND** el logo SHALL seguir visible

#### Scenario: La marca se ve desplegada
- **GIVEN** el panel con la barra lateral desplegada
- **WHEN** se pinta la marca del panel
- **THEN** SHALL verse el nombre «Kesto» junto al logo

#### Scenario: La portada pública muestra el logo de Kesto
- **GIVEN** la portada pública visible en `/`
- **WHEN** se pinta la portada
- **THEN** SHALL verse el logo de Kesto
- **AND** el logo SHALL ser legible en el tema activo

### Requirement: El panel ofrece tema claro y oscuro, con oscuro por defecto y elección persistida

El panel SHALL arrancar en **tema oscuro** siempre que no exista una elección guardada. Un control en el layout SHALL permitir alternar entre tema claro y oscuro; al activarlo, el tema SHALL cambiar de inmediato en todo el panel y la elección SHALL persistir en el almacenamiento del navegador, de modo que una recarga la respete. El arranque NO SHALL seguir la preferencia del sistema por sí solo: sin elección guardada, el resultado SHALL ser el oscuro. El control de tema SHALL residir en el pie de la barra lateral, junto al control de plegado.

El control SHALL ser accesible: SHALL tener nombre accesible, SHALL ser alcanzable y activable solo con teclado, y SHALL anunciar el tema hacia el que cambia, no solo su color. Ningún color del panel SHALL quedar fijado a un tema: los colores SHALL proceder del tema activo, de modo que ambos sean legibles.

El porqué: el panel se usa en sesiones largas y la identidad visual pide el modo oscuro por defecto, pero el operador debe poder pasar a claro y que su elección se recuerde. Que el color esté fijado a mano es justo lo que hoy hace ilegible una mitad del producto. El control de tema se agrupa con el de plegado, al pie de la barra lateral, para que los dos mandos del chasis queden juntos.

#### Scenario: Por defecto, oscuro
- **GIVEN** un navegador sin ninguna elección de tema guardada
- **WHEN** se carga el panel
- **THEN** el tema activo SHALL ser el oscuro

#### Scenario: La elección persiste tras recargar
- **GIVEN** el panel en tema oscuro
- **WHEN** se activa el control de tema y se recarga el panel
- **THEN** el tema activo SHALL ser el claro
- **AND** NO SHALL volver al oscuro por defecto

#### Scenario: Un control accesible
- **GIVEN** el foco en el control de tema
- **WHEN** se activa con el teclado
- **THEN** el tema SHALL cambiar
- **AND** el control SHALL anunciar su estado al usuario

#### Scenario: El control de tema vive en el pie de la barra lateral
- **GIVEN** el panel con la barra lateral desplegada
- **WHEN** se pinta la barra lateral
- **THEN** el control de tema SHALL estar en el pie de la barra lateral, junto al de plegado
- **AND** SHALL ser alcanzable con el teclado

#### Scenario: Ambos temas son legibles
- **GIVEN** el panel en cualquiera de los dos temas
- **WHEN** se recorre la barra lateral, su pie y el contenido
- **THEN** NO SHALL haber texto sobre un fondo del mismo color
- **AND** los colores SHALL proceder del tema activo

### Requirement: El código de aplicación no depende de tipos de Node

El proyecto de UI SHALL separar la configuración de TypeScript por fronteras: aplicación, tooling y tests. El `tsconfig.json` de la **aplicación** (`ui/src`) NO SHALL declarar tipos de Node ni globales de Vitest. Los tipos de Node SHALL acotarse a los dos proyectos que sí los necesitan y SHALL declararse en el tsconfig de cada uno, no en el de la aplicación: el proyecto de **tooling** (`ui/tsconfig.node.json`, que typechequea `vite.config.ts`, ejecutado en Node) y el de **tests** (`ui/tsconfig.test.json`, que extiende del de aplicación y declara `"types": ["vitest/globals", "node"]`). El código de aplicación NO SHALL importar ni referenciar APIs o tipos de Node, y el andamiaje de pruebas (setup y dobles) SHALL vivir fuera de `ui/src`. El typecheck de la aplicación SHALL usar el tsconfig de aplicación y el de tests SHALL usar el tsconfig de tests.

El porqué: el panel es código de navegador; permitir tipos de Node en toda la app invita a dependencias accidentales del entorno y oculta errores que solo aparecerían en el bundle. Acotarlos al tooling y a los tests mantiene la frontera explícita: el tooling los necesita para `vite.config.ts` (que corre en Node) y los tests para leer el sistema de ficheros.

#### Scenario: La aplicación compila sin tipos de Node
- **GIVEN** el código de aplicación de `ui/src`
- **WHEN** se ejecuta el typecheck con el tsconfig de aplicación
- **THEN** la compilación SHALL completarse sin resolver tipos de Node
- **AND** el tsconfig de aplicación NO SHALL contener `"types": ["node"]` ni `"types": ["vitest/globals"]`

#### Scenario: El proyecto de tooling conserva sus tipos de Node
- **GIVEN** `ui/vite.config.ts`, que se ejecuta en Node
- **WHEN** se resuelve el tsconfig de ese proyecto de tooling (`ui/tsconfig.node.json`)
- **THEN** los tipos de Node SHALL estar disponibles para él
- **AND** esos tipos SHALL declararse en el tsconfig de tooling, no en el de aplicación

#### Scenario: Los tests conservan los tipos de Node y de Vitest
- **GIVEN** los tests de UI
- **WHEN** se ejecutan con el tsconfig de tests
- **THEN** los tipos de Node y los globales de Vitest SHALL estar disponibles
- **AND** el tsconfig de tests SHALL extender del tsconfig de aplicación

### Requirement: El tema conserva su comportamiento tras el refactor de memoización

El provider y el hook de tema SHALL mantener su API pública y su comportamiento observable: tema oscuro por defecto sin elección guardada, persistencia de la elección en `localStorage` bajo la clave `kesto-tema`, y conmutación inmediata y accesible. La eliminación de `useCallback`/`useMemo` redundantes NO SHALL alterar el tema aplicado, la persistencia ni la accesibilidad del control.

El porqué: la memoización sobrante añade complejidad sin beneficio; retirarla no debe cambiar lo que el operador ve ni cómo se guarda su elección.

#### Scenario: El tema por defecto sigue siendo oscuro
- **GIVEN** un navegador sin elección guardada
- **WHEN** se carga el panel
- **THEN** el tema activo SHALL ser el oscuro

#### Scenario: La elección persiste tras el refactor
- **GIVEN** el panel en tema claro
- **WHEN** se recarga el panel
- **THEN** el tema activo SHALL seguir siendo el claro
- **AND** `localStorage` SHALL conservar la clave `kesto-tema`

#### Scenario: El conmutador sigue siendo accesible
- **GIVEN** el panel cargado
- **WHEN** se navega con teclado hasta el control de tema y se activa
- **THEN** el tema SHALL cambiar
- **AND** el control SHALL anunciar el tema hacia el que cambia

### Requirement: El panel ofrece restaurar un snapshot con confirmación y progreso

La pantalla de snapshots SHALL ofrecer, por cada fila con target conocido, una acción de restaurar. Antes de lanzarla SHALL mostrarse una confirmación que indique el target sobre el que se va a restaurar —nombre y tipo, tomados de `target_name` y `target_type`—, y esa confirmación NO SHALL reservarse a los targets de base: SHALL ofrecerse para **todos** los tipos restaurables. Cuando el target sea de **base de datos** (`mariadb`, `postgresql`, `sqlite`), la confirmación SHALL exigir además que el operador **teclee el nombre del target**, SHALL advertir de que la operación **sobrescribe los datos vivos**, y SHALL enviar `sobrescribir: true` solo cuando el nombre tecleado coincida. Cuando el target sea de **directorio**, la confirmación SHALL advertir de que la operación **retira del destino** lo que no está en la instantánea —la poda— y SHALL enviar `sobrescribir: true` solo cuando el operador la acepte; sin aceptarla, el directorio SHALL restaurarse con `sobrescribir: false`, y el destino SHALL **solo recibir**.

La bandera NO SHALL deducirse del tipo de target, ni deducirse tampoco del camino que se haya tomado en la fila: SHALL ser un **parámetro explícito** de la operación de restaurar —la fila llama a `alRestaurar(snapshot, sobrescribir)`— y el cuerpo del `POST` SHALL llevarla como booleano para todo tipo, no solo para las bases. Durante la operación, el panel SHALL reflejar la **fase** a partir del evento `restore_progress` sin recargar, y SHALL informar del resultado (éxito o error) con el mensaje de la API.

La acción NO SHALL ofrecerse para snapshots sin target conocido (`target_type` a `null`): sin destino no hay nada que restaurar, y SHALL explicarse la limitación. La limitación por tipo de target deja de existir para los cuatro tipos del esquema —`directory`, `mariadb`, `postgresql` y `sqlite` son todos restaurables—; SHALL quedar deshabilitada solo ante un `target_type` que esta versión no reconozca.

El progreso que el panel SHALL mostrar es el de la fase, no el de un avance incremental: los contadores del evento van a `0` en esta versión, así que el panel NO SHALL pintar un porcentaje ni una barra determinada, y SHALL limitarse a decir que la restauración está **en curso**, **terminada** o **fallida**.

El porqué: una restauración escribe sobre datos del operador, y en una base los **pisa**. Pedir que se teclee el nombre del target es lo que convierte un clic accidental en una decisión deliberada, y nombrar el destino evita restaurar en el sitio equivocado. En un directorio no se pisan filas pero se **retira** lo que no está en la copia, y lo retirado no vuelve: el efecto es igual de irreversible, así que la confirmación y el aviso son los mismos y la bandera viaja igual —lo que no se copia de una base a un directorio es la exigencia de teclear el nombre, porque lo que se pierde no es un dato vivo, es lo que sobra de la copia—. Y la bandera no se deduce del tipo porque el tipo no dice si ese destino puede permitirse perder lo que no está en la instantánea: un directorio de trabajo sí, un directorio de datos quizá no.

#### Scenario: Confirmar antes de restaurar

- **GIVEN** una fila de snapshot de un target de directorio
- **WHEN** se pulsa «Restaurar»
- **THEN** SHALL mostrarse una confirmación con el nombre y el tipo del target
- **AND** la restauración NO SHALL empezar hasta confirmarla

#### Scenario: Un directorio ofrece la confirmación y avisa de la poda

- **GIVEN** una fila de snapshot de un target de directorio
- **WHEN** se pulsa «Restaurar»
- **THEN** SHALL mostrarse la confirmación —no una aceptación directa sin aviso—
- **AND** SHALL advertir de que la operación **retira del destino** lo que no está en la instantánea
- **AND** el `POST` NO SHALL llevar `sobrescribir: true` hasta que el operador la acepte

#### Scenario: Un directorio que confirma la sobrescritura pide la poda

- **GIVEN** la confirmación abierta de un target de directorio
- **WHEN** el operador la acepta
- **THEN** el cuerpo del `POST` SHALL llevar `sobrescribir: true`

#### Scenario: Un directorio sin confirmación solo recibe

- **GIVEN** una fila de snapshot de un target de directorio
- **WHEN** se restaura sin aceptar la confirmación de sobrescritura
- **THEN** el cuerpo del `POST` SHALL llevar `sobrescribir: false`
- **AND** el destino SHALL recibir la instantánea **sin** retirar lo que no está en ella

#### Scenario: La bandera no se deduce del tipo de target

- **GIVEN** dos filas de snapshot de targets de directorio
- **WHEN** una se restaura aceptando la confirmación y la otra sin aceptarla
- **THEN** los dos `POST` SHALL llevar la bandera explícita, una a `true` y la otra a `false`
- **AND** la bandera NO SHALL depender del tipo del target ni de la rama que pintó la fila

#### Scenario: Progreso en vivo

- **GIVEN** una restauración en curso
- **WHEN** llega un evento `restore_progress` con fase `restaurando`
- **THEN** el panel SHALL reflejar sin recargar que la restauración está **en curso**
- **AND** NO SHALL mostrar un porcentaje ni una barra determinada a partir de los contadores

#### Scenario: Cierre de fase en vivo

- **GIVEN** una restauración en curso
- **WHEN** llega un evento `restore_progress` con fase `terminada` o `fallida`
- **THEN** el panel SHALL reflejar sin recargar ese desenlace de fase
- **AND** SHALL mostrar el mensaje con el que la API informa del resultado

#### Scenario: Fila sin target conocido

- **GIVEN** una fila de snapshot cuyo `target_type` es `null`
- **WHEN** el operador mira la fila
- **THEN** la acción de restaurar SHALL quedar deshabilitada
- **AND** SHALL explicarse que no se puede restaurar porque no se conoce el target

#### Scenario: Fila de un target no restaurable

- **GIVEN** una fila de snapshot cuyo `target_type` no es ninguno de los cuatro tipos que esta versión sabe restaurar
- **WHEN** el operador mira la fila
- **THEN** la acción de restaurar SHALL quedar deshabilitada
- **AND** SHALL explicarse que ese tipo de target no se reconoce

#### Scenario: Resultado

- **GIVEN** una restauración que termina
- **WHEN** el backend publica el resultado
- **THEN** el panel SHALL mostrarlo con el mensaje de la API

#### Scenario: Un target de base exige teclear su nombre

- **GIVEN** una fila de snapshot de un target `postgresql`
- **WHEN** se pulsa «Restaurar»
- **THEN** la confirmación SHALL pedir el nombre del target y advertir de la sobrescritura
- **AND** el `POST` NO SHALL enviar `sobrescribir: true` hasta que el nombre tecleado coincida

#### Scenario: El nombre que no coincide no autoriza

- **GIVEN** la confirmación de un target de base
- **WHEN** el nombre tecleado no coincide con `target_name`
- **THEN** la acción de confirmar SHALL quedar deshabilitada

### Requirement: El panel muestra el histórico de restauraciones

SHALL existir una pantalla de **Restauraciones**, accesible desde la navegación principal, que liste las ejecuciones de `GET /api/v1/restorations` con su estado, target, snapshot y fechas, con filtro por estado, filtro por target y paginación. El estado SHALL reflejar el desenlace real —no un «lanzada» optimista— y las que están en curso o pendientes SHALL poder refrescarse sin recargar la página.

La pantalla SHALL usar el paginador de Ant Design alimentado por el `total` de la respuesta, con el total de elementos, el número de páginas y un selector de tamaño con `10` por defecto y las opciones `10`, `20` y `50`. Al cambiar de página o de tamaño SHALL pedirse la página al servidor con el `offset` correspondiente; NO SHALL paginarse en cliente.

El filtro por target SHALL ofrecerse como un selector cuyas opciones provienen del catálogo de targets de `GET /api/v1/targets`, y SHALL enviarse a la API como el parámetro `target_id`. El selector SHALL poder limpiarse, y mientras esté activo la pantalla NO SHALL presentar el listado como si fuera completo: el total SHALL corresponder a la consulta acotada. Al cambiar el target SHALL reiniciarse la paginación, de modo que un `offset` heredado NO SHALL producir una página vacía sobre el resultado acotado. El filtro por target y el filtro por estado SHALL poder combinarse, y limpiar el selector SHALL volver a pedir el listado sin `target_id`.

Cuando la opción elegida ya no exista en el catálogo —el target se borró— la pantalla SHALL seguir mostrando el histórico que la API devuelva para ese `target_id`, o su estado vacío, y NO SHALL romperse ni quedarse en carga.

El porqué: la restauración deja de ser efímera; si el registro existe pero el panel no lo muestra, el operador no puede responder «¿se restauró?» ni «¿por qué falló?» sin mirar la base. Y con varias bases, el histórico sin filtro por target no responde a la pregunta que se hace de verdad, que es por una base concreta.

#### Scenario: El histórico lista las restauraciones
- **GIVEN** restauraciones persistidas
- **WHEN** se abre la pantalla de Restauraciones
- **THEN** SHALL listarlas con su estado, target y fechas
- **AND** SHALL poder filtrarse por estado
- **AND** el paginador SHALL mostrar el total devuelto por la API

#### Scenario: El filtro por target acota el listado
- **GIVEN** restauraciones de dos targets distintos
- **WHEN** se elige uno en el selector de target
- **THEN** SHALL pedirse `GET /api/v1/restorations` con su `target_id`
- **AND** la tabla SHALL mostrar solo las de ese target
- **AND** el total SHALL corresponder al resultado acotado

#### Scenario: Cambiar el filtro reinicia la paginación
- **GIVEN** un histórico paginado y una página distinta de la primera
- **WHEN** se cambia el target o el estado
- **THEN** SHALL pedirse el listado desde la primera página

#### Scenario: Limpiar el filtro vuelve al listado completo
- **GIVEN** un listado acotado por target
- **WHEN** se limpia el selector de target
- **THEN** SHALL pedirse el listado sin `target_id`
- **AND** la tabla SHALL volver a mostrar las restauraciones de todos los targets

#### Scenario: Los filtros se combinan
- **GIVEN** un target con restauraciones en estados distintos
- **WHEN** se eligen ese target y el estado `failed`
- **THEN** la petición SHALL llevar ambos parámetros
- **AND** la tabla SHALL mostrar solo las fallidas de ese target

#### Scenario: Una restauración fallida muestra su motivo
- **GIVEN** una restauración en estado `failed` con `error_message`
- **WHEN** se mira en el histórico
- **THEN** SHALL mostrarse su motivo

### Requirement: El panel muestra el estado de salud por repositorio sin disparar comprobaciones

La pantalla de **Ajustes** SHALL mostrar, por cada repositorio, su **estado de salud** con su texto (el color NO SHALL ser el único portador del estado) y su **fecha** de última comprobación, y SHALL ofrecer una acción para **comprobar** ese repositorio y otra para comprobar todos. Comprobar SHALL solicitar el endpoint correspondiente, y el resultado SHALL reflejarse en el indicador.

El **listado del Dashboard** SHALL mostrar el estado de salud **persistido** de cada repositorio SIN disparar ninguna comprobación al cargar: un repositorio sin comprobar SHALL pintarse como «desconocido», no como sano. Cuando un repositorio figure como caído, la información de los repositorios sanos NO SHALL ocultarse.

La interfaz NO SHALL inventar un estado: lo que muestra SHALL ser el que la API sirve, y una comprobación en curso SHALL indicarse sin bloquear el resto de la pantalla.

#### Scenario: Ajustes pinta el estado y la fecha de cada repositorio
- **GIVEN** repositorios con estados de salud distintos
- **WHEN** se abre Ajustes
- **THEN** cada repositorio SHALL mostrar su estado con su texto y su fecha
- **AND** el color NO SHALL ser el único indicador

#### Scenario: Comprobar un repositorio actualiza su indicador
- **GIVEN** un repositorio en Ajustes
- **WHEN** el operador pulsa «comprobar»
- **THEN** SHALL pedirse la comprobación de ese repositorio
- **AND** el indicador SHALL reflejar el estado devuelto

#### Scenario: El Dashboard muestra la salud sin comprobar
- **GIVEN** varios repositorios con estados persistidos
- **WHEN** se carga el Dashboard
- **THEN** SHALL mostrarse el estado persistido de cada uno
- **AND** NO SHALL dispararse ninguna comprobación contra los destinos

#### Scenario: Un repositorio sin comprobar no se pinta como sano
- **GIVEN** un repositorio nunca comprobado
- **WHEN** se pinta su estado
- **THEN** SHALL mostrarse como «desconocido»
- **AND** NO SHALL mostrarse como sano

### Requirement: Los listados paginados conservan la página anterior mientras llega la siguiente

Los tres hooks de listado paginado —`useEjecuciones` (Histórico), `useSnapshots` (Snapshots) y `useRestauraciones` (Restauraciones)— SHALL conservar los datos de la página anterior mientras se carga la nueva, de modo que la tabla NO SHALL desmontarse ni mostrar el estado «cargando» al cambiar de página.

Para ello SHALL usar `placeholderData: keepPreviousData` de React Query: mientras llega la página nueva, `data` SHALL seguir siendo la página anterior, `isPlaceholderData` SHALL ser `true` e `isPending` SHALL quedarse en `false`. La clave de la consulta (con su `offset`) NO SHALL cambiar: cada página sigue siendo una entrada de caché propia y se pide al servidor.

El `total` que alimenta el paginador SHALL seguir leyéndose de la respuesta: durante la transición es el de la página anterior, coherente con las filas que todavía se ven.

#### Scenario: Al cambiar de página no se vuelve a «cargando»
- **GIVEN** el Histórico con una página ya cargada
- **WHEN** se pasa a la página siguiente y su petición todavía no ha llegado
- **THEN** la tabla SHALL seguir mostrando la página anterior
- **AND** `isPending` SHALL ser `false` y `isPlaceholderData` SHALL ser `true`
- **AND** NO SHALL mostrarse el estado «cargando»

#### Scenario: Snapshots conserva la página anterior
- **GIVEN** Snapshots con una página ya cargada de un repositorio
- **WHEN** se pasa a la página siguiente y su petición todavía no ha llegado
- **THEN** la tabla SHALL seguir mostrando la página anterior
- **AND** `isPlaceholderData` SHALL ser `true`

#### Scenario: Restauraciones conserva la página anterior
- **GIVEN** Restauraciones con una página ya cargada
- **WHEN** se pasa a la página siguiente y su petición todavía no ha llegado
- **THEN** la tabla SHALL seguir mostrando la página anterior
- **AND** `isPlaceholderData` SHALL ser `true`

### Requirement: El documento del panel se sirve bajo la política de contenido del servidor

El documento del panel (`ui/index.html`) NO SHALL contener JavaScript en línea, para que la `Content-Security-Policy` del servidor pueda exigir `script-src 'self'` sin `'unsafe-inline'` ni hashes. El arranque de tema —que fija el tema antes del primer pintado— SHALL vivir en un fichero estático propio bajo `ui/public/`, servido por el mismo origen y cargado de forma síncrona en el `<head>`, para que no haya destello de tema.

El código de la aplicación NO SHALL usar `eval` ni `new Function`.

Los estilos en línea SÍ SHALL permitirse: la política del servidor declara `style-src 'self' 'unsafe-inline'` porque antd v5 inyecta estilos en tiempo de ejecución y los componentes usan atributos `style`.

#### Scenario: El documento no lleva script en línea
- **GIVEN** `ui/index.html`
- **WHEN** se inspecciona
- **THEN** NO SHALL contener ningún `<script>` con cuerpo en línea
- **AND** el arranque de tema SHALL cargarse desde un fichero estático propio

#### Scenario: El tema se aplica antes del primer pintado
- **GIVEN** el panel servido con el arranque de tema en un fichero propio, cargado de forma síncrona en el `<head>`
- **WHEN** se carga la aplicación bajo `script-src 'self'`
- **THEN** el tema del documento SHALL quedar fijado antes del primer pintado y NO SHALL haber destello

### Requirement: Los formularios del panel no ofrecen escribir más de lo que el borde acepta

Cada campo de formulario del panel que corresponda a un campo de texto de la API de `/targets` o `/repositories` SHALL llevar como `maxLength` la **misma** cota que el borde impone, de modo que el formulario no deje escribir un valor que el servidor rechazará con `400`:

- `name` → 120 · `container_name` → 128 · `db_user` → 63 · `db_name` → 63 · `bucket` → 63 · `region` → 64 · `access_key_id` → 128 · `endpoint` → 2048 · `db_path` → 1024 · `host_path` → 4096.

El porqué: es la convención del repositorio —la cota del `400` y el `maxLength` del campo son el mismo número, como documenta `COTA_NOMBRE` en `src/api/texto.rs`—. El panel es el consumidor principal del borde, y un formulario que deja escribir un valor que el servidor rechaza obliga al operador a un ida y vuelta para descubrir el tope.

#### Scenario: Un campo del formulario no deja pasar de su cota
- **GIVEN** el modal de alta de un repositorio con el campo `bucket` pintado
- **WHEN** se inspecciona el campo `bucket`
- **THEN** su `maxLength` SHALL ser 63
- **AND** el campo `name` del mismo formulario SHALL tener `maxLength` 120

#### Scenario: Los campos propios de SQLite llevan su cota
- **GIVEN** el modal de alta de fuente con el tipo `sqlite`
- **WHEN** se inspeccionan sus campos `container_name` y `db_path`
- **THEN** `container_name` SHALL tener `maxLength` 128
- **AND** `db_path` SHALL tener `maxLength` 1024
