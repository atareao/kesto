# verificacion-e2e — delta

## Purpose

La batería que comprueba que la copia y la restauración funcionan de verdad, contra contenedores reales. Este delta añade a la restauración de punta a punta la comprobación de que un volcado **no confiable** —con metacomandos de cliente dentro— no ejecuta nada en el contenedor destino, que es el extremo observable del endurecimiento de `restauracion`.

## MODIFIED Requirements

### Requirement: Restauración hacia contenedor de punta a punta

La restauración hacia un contenedor SHALL estar cubierta por la batería de punta a punta, desde el snapshot en S3 hasta el `stdin` del proceso de restauración, sin fichero temporal intermedio. La batería SHALL cubrir además el trato del volcado como contenido **no confiable**: un volcado que traiga metacomandos de cliente SHALL restaurarse sin que ninguno se ejecute, y la restauración global de PostgreSQL SHALL repartir cada base en su base.

#### Scenario: El snapshot viaja de S3 al stdin del contenedor
- **GIVEN** un snapshot en S3 con un volcado conocido
- **WHEN** se restaura hacia un contenedor
- **THEN** el volcado llega completo al `stdin` del proceso y el destino contiene los datos esperados

#### Scenario: Un contenedor que no drena su stdin no cuelga la restauración
- **GIVEN** un contenedor cuyo proceso de restauración no lee su `stdin`
- **WHEN** transcurre el plazo de inactividad
- **THEN** la restauración termina en error acotado y no queda bloqueada

#### Scenario: Un volcado con metacomandos no ejecuta nada en el contenedor
- **GIVEN** un snapshot cuyo volcado lleva un metacomando de cliente que escribiría un fichero de prueba en el contenedor
- **WHEN** se restaura hacia el contenedor
- **THEN** el fichero de prueba NO SHALL existir
- **AND** el resto del volcado SHALL restaurarse con los datos esperados

#### Scenario: La restauración global de PostgreSQL reparte cada base en su base
- **GIVEN** un snapshot de PostgreSQL con dos bases y sus datos
- **WHEN** se restaura el volcado global hacia el contenedor
- **THEN** cada base SHALL contener los datos que le corresponden
- **AND** ninguna base SHALL recibir los datos de otra
