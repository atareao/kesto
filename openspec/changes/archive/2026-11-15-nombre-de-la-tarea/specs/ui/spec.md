## MODIFIED Requirements

### Requirement: Tareas permite dar de alta, editar, disparar y borrar sin salir del panel

La pantalla de Tareas SHALL listar las tareas y SHALL permitir crear y editar en un `Modal` con `Form`, borrar con confirmación y disparar manualmente cada tarea. El formulario SHALL exigir `target_id`, `repository_id`, `cron_expression` y las tres reglas de retención, y SHALL usar `Select` para el target y el repositorio. El campo `container_name` del target SHALL ser un `AutoComplete` que sugiere los contenedores ya usados por los targets existentes y admite texto libre; NO SHALL introducirse un endpoint de contenedores en este change.

El formulario SHALL incluir además un campo de texto opcional `name` («Nombre»), y la tabla de Tareas SHALL mostrar una columna «Nombre», vacía cuando la tarea no lo tenga. El nombre es lo que distingue dos tareas sobre el mismo target en la lista; sin él, dos tareas que copian al mismo sitio se leen igual.

La expresión cron SHALL poder construirse con un generador visual de **seis** campos —segundo, minuto, hora, día del mes, mes y día de la semana— y SHALL poder editarse como texto. La validación del cliente SHALL ser una ayuda y NO SHALL sustituir a la del servidor: un `400` que nombre `cron_expression` SHALL mostrarse en el campo. Las tres reglas de retención SHALL ser `InputNumber` no negativos y el formulario NO SHALL enviar una tarea con las tres a cero.

Cuando haya que crear un target o un repositorio desde el propio formulario, las credenciales —`db_password`, `secret_access_key` y `passphrase`— SHALL escribirse con `Input.Password`, NO SHALL prerrellenarse nunca y, al editar, dejarlas vacías SHALL significar «conservar la que hay».

El disparo manual SHALL llamar a `POST /api/v1/jobs/:id/run`. Un `202` SHALL incorporar la ejecución devuelta con estado `pending` y refrescar el Histórico; un `409` SHALL explicar que la tarea ya tiene una ejecución en curso; un `404` SHALL retirar la tarea de la lista tras refrescarla.

#### Scenario: El alta envía los campos del §3 y refresca la lista
- **GIVEN** un formulario con target, repositorio, cron `0 0 3 * * *`, retención `7/4/6` y `enabled` marcado
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
- **GIVEN** un formulario con una expresión que el servidor rechaza con `400`
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
- **WHEN** se escribe `copia nocturna` en el campo «Nombre» y se guarda
- **THEN** la petición SHALL llevar `name` con ese valor
- **AND** la tabla de Tareas SHALL mostrar una columna «Nombre» con ese valor en la fila de la tarea
- **AND** una tarea sin nombre SHALL mostrar la columna «Nombre» vacía

### Requirement: El Histórico pagina, filtra y muestra la consola de la ejecución en vivo

El Histórico SHALL listar las ejecuciones con `GET /api/v1/executions`, SHALL paginar con `limit` y `offset` —`50` por defecto y `200` como tope—, y SHALL filtrar por `job_id` y por `status`. Como la respuesta es un array sin total, la paginación SHALL avanzar cuando la página venga llena y SHALL deshabilitar «siguiente» cuando venga vacía o incompleta; NO SHALL inventarse un total.

Cada fila SHALL mostrar las fechas en hora local, la duración cuando haya `finished_at`, los dos contadores de bytes y el estado con su `Badge` y su texto. La consola SHALL mostrar en vivo el `current_action` y el avance de la ejecución seleccionada a partir del stream SSE, SHALL conservar solo un número acotado de líneas y SHALL indicar cuando la conexión está caída.

Cada fila SHALL mostrar además una columna «Tarea» con el `job_name` de la ejecución y, cuando la tarea no tenga nombre, el identificador de la tarea: nunca el nombre del target. El filtro por tarea SHALL etiquetar cada opción con el nombre de la tarea —o con su identificador si no lo tiene—, y NO SHALL usar el nombre del target, que es lo que hoy hace indistinguibles dos tareas sobre el mismo target.

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

### Requirement: Snapshots explora un repositorio y dice cuándo el listado está recortado

La pantalla de Snapshots SHALL exigir elegir un repositorio de los que devuelve `GET /api/v1/repositories` y SHALL listar sus instantáneas con `GET /api/v1/snapshots?repository_id=<id>`. La tabla SHALL mostrar el identificador, la fecha en hora local, la etiqueta, los ficheros y los bytes, y SHALL presentar `paths` sin romper la fila. Cuando `truncated` sea `true`, SHALL avisar de que puede haber más instantáneas y NO SHALL presentar la lista como completa.

La tabla SHALL mostrar además una columna «Tarea» **entre la fecha y la etiqueta**, con el `job_name` de la instantánea y, cuando la tarea no tenga nombre, el identificador de la tarea. La columna «Etiqueta» NO SHALL tocarse: sigue mostrando el identificador del snapshot.

La pantalla NO SHALL ofrecer una acción de purga manual mientras la API no exponga una ruta para ella: un botón que no puede cumplir lo que promete es peor que su ausencia.

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
- **GIVEN** instantáneas listadas, una de una tarea con nombre y otra de una tarea sin él
- **WHEN** se pinta la tabla
- **THEN** SHALL verse una columna «Tarea» entre la fecha y la etiqueta
- **AND** SHALL mostrar el nombre de la tarea, o su identificador cuando no lo tenga
- **AND** la columna «Etiqueta» SHALL seguir mostrando el identificador del snapshot
