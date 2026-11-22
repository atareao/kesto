# db Specification (delta)

## ADDED Requirements

### Requirement: El motor de la base es el SQLite empaquetado y las migraciones aplicadas no se invalidan

La base SHALL seguir siendo **SQLite empaquetado** —compilado dentro del binario, sin depender de una biblioteca del sistema—, y la declaración de features del cliente SHALL encender solo lo que el producto usa: la carga de extensiones, el aviso de desbloqueo y la deserialización de tipos NO SHALL encenderse de rebote.

El algoritmo del checksum de una migración NO SHALL cambiar: un fichero de base creado por una versión anterior SHALL abrirse con la versión nueva sin error de versión y sin reaplicar ninguna migración. El nombre y la forma de la tabla de control NO SHALL cambiar.

#### Scenario: El checksum de una migración no cambia
- **GIVEN** el `Migrator` del proyecto y una migración conocida
- **WHEN** se calcula su checksum con la biblioteca vigente
- **THEN** SHALL coincidir con el valor fijado en la prueba

#### Scenario: Una base creada antes se abre sin reaplicar migraciones
- **GIVEN** un fichero de base con las migraciones ya aplicadas
- **WHEN** lo abre el binario nuevo
- **THEN** las migraciones SHALL estar al día
- **AND** NO SHALL haber error de versión

#### Scenario: No se encienden capacidades que el producto no usa
- **GIVEN** la declaración de dependencias del crate raíz
- **WHEN** se leen las features del cliente de base
- **THEN** SHALL declararse el SQLite empaquetado
- **AND** NO SHALL declararse la carga de extensiones, el aviso de desbloqueo ni la deserialización
