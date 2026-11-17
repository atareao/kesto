# api-resources — delta

## ADDED Requirements

### Requirement: El estado de salud de un repositorio se comprueba bajo demanda y se sirve en el recurso

La API SHALL ofrecer una comprobación de salud por repositorio y una que comprueba todos: `POST /api/v1/repositories/{id}/health` y `POST /api/v1/repositories/health`. Cada una SHALL ejecutar la comprobación de la capacidad `repository`, **anotar el desenlace en la fila** y devolverlo en la envoltura del §5. La comprobación de todos SHALL recorrer los repositorios existentes y devolver un elemento por cada uno.

El recurso de repositorio que sirve `GET /api/v1/repositories` y `GET /api/v1/repositories/{id}` SHALL incluir el **estado de salud**, la **fecha** de la última comprobación y un **motivo** acotado, derivados de la fila. El estado NO SHALL ser un campo editable: NO SHALL admitirse en el alta ni en la actualización. Un repositorio nunca comprobado SHALL servirse con el estado «desconocido» y la fecha a nulo.

El motivo SHALL estar **saneado**: NO SHALL contener credenciales ni el motivo crudo de la dependencia, solo un texto legible para el operador.

#### Scenario: Comprobar un repositorio persiste y devuelve su estado
- **GIVEN** un repositorio dado de alta
- **WHEN** se pide `POST /api/v1/repositories/{id}/health`
- **THEN** la respuesta SHALL traer el estado resultante en la envoltura del §5
- **AND** el estado SHALL quedar persistido y visible en la lectura posterior del recurso

#### Scenario: El listado sirve el estado sin comprobar nada
- **GIVEN** varios repositorios con estados de salud anotados
- **WHEN** se pide `GET /api/v1/repositories`
- **THEN** cada elemento SHALL traer su estado y su fecha
- **AND** NO SHALL ejecutarse ninguna comprobación contra el destino

#### Scenario: Comprobar todos devuelve un elemento por repositorio
- **GIVEN** tres repositorios dados de alta
- **WHEN** se pide `POST /api/v1/repositories/health`
- **THEN** la respuesta SHALL traer tres elementos
- **AND** cada estado SHALL quedar persistido en su fila

#### Scenario: La salud no se puede escribir por el alta ni la actualización
- **GIVEN** un cuerpo de alta o de actualización que incluye un campo de salud
- **WHEN** se envía
- **THEN** el campo SHALL ignorarse
- **AND** la salud de la fila NO SHALL venir de ese cuerpo

#### Scenario: Un repositorio nunca comprobado se sirve como desconocido
- **GIVEN** un repositorio recién dado de alta
- **WHEN** se pide su lectura
- **THEN** su salud SHALL ser «desconocido»
- **AND** su fecha de comprobación SHALL ser nula
