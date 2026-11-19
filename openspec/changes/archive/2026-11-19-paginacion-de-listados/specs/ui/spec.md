# ui Specification

## MODIFIED Requirements

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
