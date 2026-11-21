# config — delta

## Purpose

El contrato de configuración de Kesto: qué variables de entorno se leen y con qué nombre exacto, cuáles tienen valor por defecto, cuáles son obligatorias y cuándo la aplicación debe negarse a arrancar en lugar de degradarse en silencio. Este delta añade la raíz bajo la que viven los directorios que se copian.

## ADDED Requirements

### Requirement: La raíz de las copias de directorio se configura con `KESTO_BACKUP_ROOT`

`KESTO_BACKUP_ROOT` SHALL ser la raíz bajo la que el motor acepta el `host_path` de un target `directory`, y SHALL tener el valor por defecto `/backup_sources`, que es donde el `compose` del §8 monta los directorios a respaldar, para que el despliegue de referencia no tenga que declararla.

Su valor SHALL ser una ruta **absoluta**. Presente pero relativa o vacía SHALL ser un error de arranque que nombra la variable, en la línea de `RUST_LOG` y `BIND_ADDRESS`: un valor presente pero inutilizable no se degrada en silencio. Ausente SHALL tomar el valor por defecto.

La variable lleva el prefijo `KESTO_` que este repositorio reserva para todo lo que no son las tres del §8, `DOCKER_HOST` y las `OIDC_*`.

El porqué: la raíz no se cablea en código porque el e2e y el modo local copian directorios temporales que no viven bajo `/backup_sources`, y porque el operador debe poder moverla sin tocar el binario. Y no se deriva de los montajes del propio contenedor porque en el modo local no hay contenedor de Kesto que inspeccionar.

#### Scenario: Sin la variable gana el defecto

- **GIVEN** un entorno sin `KESTO_BACKUP_ROOT`
- **WHEN** se carga la configuración
- **THEN** la raíz de copias SHALL ser `/backup_sources`
- **AND** la configuración SHALL ser válida

#### Scenario: Una raíz relativa no se acepta

- **GIVEN** un entorno con `KESTO_BACKUP_ROOT=backup_sources`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_BACKUP_ROOT`

#### Scenario: Una raíz vacía no cae al defecto

- **GIVEN** un entorno con `KESTO_BACKUP_ROOT` presente pero vacía
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_BACKUP_ROOT`
