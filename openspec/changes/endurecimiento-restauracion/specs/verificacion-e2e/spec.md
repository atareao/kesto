# verificacion-e2e — delta

## Purpose

La batería que comprueba que la copia y la restauración funcionan de verdad, contra contenedores reales. Este delta añade a la restauración de punta a punta la comprobación de que un volcado **no confiable** —con metacomandos de cliente dentro— no ejecuta nada en el contenedor destino, que es el extremo observable del endurecimiento de `restauracion`.

## MODIFIED Requirements

### Requirement: Restauración hacia contenedor de punta a punta

La restauración hacia un contenedor SHALL estar cubierta por la batería de punta a punta, desde el snapshot en S3 hasta el `stdin` del proceso de restauración, sin fichero temporal intermedio. La batería SHALL cubrir además dos consecuencias del trato del volcado como contenido **no confiable**: la restauración global de PostgreSQL SHALL repartir cada base en su base, y los **datos** de un volcado que *parezcan* un metacomando SHALL sobrevivir al ciclo íntegros —lo que fija que el descarte de metacomandos no se come contenido—.

Que un metacomando del volcado **no se ejecute** se comprueba en las pruebas de sonda de la capability `restauracion`, no aquí: la batería no puede inyectar un volcado hostil en el repositorio, porque el volcado lo produce la extracción a partir de una base real. Aquí se comprueba lo que sí viaja de punta a punta.

#### Scenario: El snapshot viaja de S3 al stdin del contenedor
- **GIVEN** un snapshot en S3 con un volcado conocido
- **WHEN** se restaura hacia un contenedor
- **THEN** el volcado llega completo al `stdin` del proceso y el destino contiene los datos esperados

#### Scenario: Un contenedor que no drena su stdin no cuelga la restauración
- **GIVEN** un contenedor cuyo proceso de restauración no lee su `stdin`
- **WHEN** transcurre el plazo de inactividad
- **THEN** la restauración termina en error acotado y no queda bloqueada

#### Scenario: Los datos que parecen un metacomando sobreviven al ciclo
- **GIVEN** un origen con una tabla cuyos valores son texto que parece un metacomando de cliente (`\! …`, `\.shell`, `\connect …`)
- **WHEN** se copia y se restaura hacia el contenedor
- **THEN** esos valores SHALL estar en el destino **íntegros**, sin una línea de menos
- **AND** NO SHALL ejecutarse ninguna orden de shell

#### Scenario: La restauración global de PostgreSQL reparte cada base en su base
- **GIVEN** un snapshot de PostgreSQL con dos bases y sus datos
- **WHEN** se restaura el volcado global hacia el contenedor
- **THEN** cada base SHALL contener los datos que le corresponden
- **AND** ninguna base SHALL recibir los datos de otra

#### Scenario: Un contenedor con un `psql` sin `\restrict` falla la restauración de punta a punta
- **GIVEN** un contenedor de PostgreSQL cuyo `psql` es anterior al mínimo de su rama
- **WHEN** se restaura un snapshot global
- **THEN** la restauración SHALL acabar en `failed` nombrando el mínimo
- **AND** NO SHALL reponerse ninguna base del snapshot
