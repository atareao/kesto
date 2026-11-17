# ui — delta

## ADDED Requirements

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
