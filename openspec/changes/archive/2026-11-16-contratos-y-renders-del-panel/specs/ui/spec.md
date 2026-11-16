# ui — delta

## MODIFIED Requirements

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

### Requirement: Snapshots explora un repositorio y dice cuándo el listado está recortado

La pantalla de Snapshots SHALL exigir elegir un repositorio de los que devuelve `GET /api/v1/repositories` y SHALL listar sus instantáneas con `GET /api/v1/snapshots?repository_id=<id>`. La tabla SHALL mostrar el identificador, la fecha en hora local, la etiqueta, los ficheros y los bytes, y SHALL presentar `paths` sin romper la fila. Cuando `truncated` sea `true`, SHALL avisar de que puede haber más instantáneas y NO SHALL presentar la lista como completa.

La tabla SHALL mostrar además una columna «Tarea» **entre la fecha y la etiqueta**, con el `job_name` de la instantánea. Esa columna NO SHALL caer a `label`, ni al identificador de la tarea, ni al nombre del target: «Tarea» y «Etiqueta» NO SHALL mostrar el mismo dato, que es exactamente lo que las hacía indistinguibles —`label` es el identificador de la tarea, así que usarlo como sustituto dejaba las dos columnas diciendo lo mismo—. Cuando `job_name` llegue a `null` —el único caso que queda ahora que toda tarea tiene nombre es la instantánea **huérfana**, la que no casa con ninguna ejecución—, SHALL mostrarse un marcador explícito y atenuado («sin tarea»), no el contenido de `label`. La columna «Etiqueta» NO SHALL tocarse: sigue mostrando el identificador del snapshot.

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
