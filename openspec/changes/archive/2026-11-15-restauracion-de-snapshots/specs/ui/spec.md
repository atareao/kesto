## ADDED Requirements

### Requirement: El panel ofrece restaurar un snapshot con confirmación y progreso

La pantalla de snapshots SHALL ofrecer, por cada fila, una acción de restaurar. Antes de lanzarla SHALL mostrarse una confirmación que indique el target sobre el que se va a restaurar —nombre y tipo, tomados de `target_name` y `target_type`, que el listado publica por fila—. Durante la operación, el panel SHALL reflejar la **fase** a partir del evento `restore_progress` sin recargar, y SHALL informar del resultado (éxito o error) con el mensaje de la API.

La acción NO SHALL ejecutarse para snapshots de targets no restaurables en esta versión; en su lugar SHALL explicarse la limitación. El panel SHALL deshabilitar la acción y explicarla cuando `target_type` sea `null` —la fila no tiene target conocido, así que no hay destino sobre el que restaurar— o sea distinto de `directory` —los targets de base de datos quedan para la fase 2—.

El progreso que el panel SHALL mostrar es el de la fase, no el de un avance incremental: los contadores del evento van a `0` en esta versión porque `rustic_core` 0.13 no ofrece callback de progreso, así que el panel NO SHALL pintar un porcentaje ni una barra determinada a partir de ellos, y SHALL limitarse a decir que la restauración está **en curso**, **terminada** o **fallida**.

El porqué: una restauración escribe sobre datos del operador; pedir confirmación con el target a la vista es lo que evita una restauración en el sitio equivocado, y el aviso de fase evita que una operación larga parezca colgada. Ofrecer la acción para una fila sin target conocido —o para un target que esta versión no sabe restaurar— llevaría al operador a confirmar algo que la API va a rechazar, así que la limitación se declara en la propia fila y no después del error.

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
- **GIVEN** una fila de snapshot cuyo `target_type` es `mariadb`
- **WHEN** el operador mira la fila
- **THEN** la acción de restaurar SHALL quedar deshabilitada
- **AND** SHALL explicarse que ese tipo de target no se restaura todavía en esta versión

#### Scenario: Resultado
- **GIVEN** una restauración que termina
- **WHEN** el backend publica el resultado
- **THEN** el panel SHALL mostrarlo con el mensaje de la API
