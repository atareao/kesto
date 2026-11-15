## MODIFIED Requirements

### Requirement: El panel ofrece restaurar un snapshot con confirmación y progreso

La pantalla de snapshots SHALL ofrecer, por cada fila con target conocido, una acción de restaurar. Antes de lanzarla SHALL mostrarse una confirmación que indique el target sobre el que se va a restaurar —nombre y tipo, tomados de `target_name` y `target_type`—. Cuando el target sea de **base de datos** (`mariadb`, `postgresql`, `sqlite`), la confirmación SHALL exigir además que el operador **teclee el nombre del target**, SHALL advertir de que la operación **sobrescribe los datos vivos**, y SHALL enviar `sobrescribir: true` solo cuando el nombre tecleado coincida. Durante la operación, el panel SHALL reflejar la **fase** a partir del evento `restore_progress` sin recargar, y SHALL informar del resultado (éxito o error) con el mensaje de la API.

La acción NO SHALL ofrecerse para snapshots sin target conocido (`target_type` a `null`): sin destino no hay nada que restaurar, y SHALL explicarse la limitación. La limitación por tipo de target deja de existir para los cuatro tipos del esquema —`directory`, `mariadb`, `postgresql` y `sqlite` son todos restaurables—; SHALL quedar deshabilitada solo ante un `target_type` que esta versión no reconozca.

El progreso que el panel SHALL mostrar es el de la fase, no el de un avance incremental: los contadores del evento van a `0` en esta versión, así que el panel NO SHALL pintar un porcentaje ni una barra determinada, y SHALL limitarse a decir que la restauración está **en curso**, **terminada** o **fallida**.

El porqué: una restauración escribe sobre datos del operador, y en una base los **pisa**. Pedir que se teclee el nombre del target es lo que convierte un clic accidental en una decisión deliberada, y nombrar el destino evita restaurar en el sitio equivocado.

#### Scenario: Confirmar antes de restaurar

- **GIVEN** una fila de snapshot de un target de directorio
- **WHEN** se pulsa «Restaurar»
- **THEN** SHALL mostrarse una confirmación con el nombre y el tipo del target
- **AND** la restauración NO SHALL empezar hasta confirmarla

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

## ADDED Requirements

### Requirement: El panel muestra el histórico de restauraciones

SHALL existir una pantalla de **Restauraciones**, accesible desde la navegación principal, que liste las ejecuciones de `GET /api/v1/restorations` con su estado, target, snapshot y fechas, con filtro por estado y paginación. El estado SHALL reflejar el desenlace real —no un «lanzada» optimista— y las que están en curso o pendientes SHALL poder refrescarse sin recargar la página.

El porqué: la restauración deja de ser efímera; si el registro existe pero el panel no lo muestra, el operador no puede responder «¿se restauró?» ni «¿por qué falló?» sin mirar la base.

#### Scenario: El histórico lista las restauraciones

- **GIVEN** restauraciones persistidas
- **WHEN** se abre la pantalla de Restauraciones
- **THEN** SHALL listarlas con su estado, target y fechas
- **AND** SHALL poder filtrarse por estado

#### Scenario: Una restauración fallida muestra su motivo

- **GIVEN** una restauración en estado `failed` con `error_message`
- **WHEN** se mira en el histórico
- **THEN** SHALL mostrarse su motivo
