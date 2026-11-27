# engine — Delta de especificación

## ADDED Requirements

### Requirement: El motor detecta cambios antes de archivar un volcado

El motor DEBE calcular, antes de archivar el volcado de una base de datos, un *manifest de
cambios* que identifique de forma estable el contenido de la BD. El manifest se compone de
la estructura (PostgreSQL: `pg_dump --schema-only`; MySQL/MariaDB: `mysqldump --no-data`) y
de un checksum de los datos por tabla (PostgreSQL: checksum de datos; MySQL/MariaDB:
`CHECKSUM TABLE`).

- Si el manifest coincide con el del último volcado archivado, el motor NO DEBE subir bytes
  de nuevo y DEBE crear una snapshot «sin cambios».
- Si el manifest difiere, el motor DEBE subir el volcado completo y etiquetarlo con
  `kesto.change_manifest=<hash>`.
- Si el manifest no se puede calcular, el motor DEBE degradar al comportamiento actual
  (subida completa forzada, sin etiqueta) en lugar de abortar el backup.

El motor NO DEBE escribir el volcado a disco ni a `/tmp`; el streaming en memoria se
mantiene.

#### Scenario: Mismo manifest produce una snapshot sin cambios

- **GIVEN** una base de datos cuyo contenido y esquema no han cambiado desde el último
  volcado archivado, con manifest `M`
- **WHEN** el motor archiva un nuevo volcado y el manifest calculado vuelve a ser `M`
- **THEN** el motor NO sube bytes del volcado de nuevo
- **AND** crea una snapshot «sin cambios» etiquetada con `kesto.change_manifest=M`
- **AND** restaurar esa snapshot devuelve el contenido del último volcado real con manifest `M`

#### Scenario: Manifest distinto produce una snapshot etiquetada con el nuevo manifest

- **GIVEN** una base de datos cuyo contenido ha cambiado desde el último volcado, con nuevo
  manifest `M2` distinto del anterior `M`
- **WHEN** el motor archiva el volcado y detecta `M2`
- **THEN** el motor sube el volcado completo
- **AND** la snapshot queda etiquetada con `kesto.change_manifest=M2`
- **AND** restaurar esa snapshot devuelve el contenido del volcado correspondiente a `M2`

#### Scenario: Fallo en la detección degrada a subida completa

- **GIVEN** una base de datos en la que el cálculo del manifest falla
- **WHEN** el motor intenta archivar un volcado
- **THEN** el motor sube el volcado completo forzando la escritura (sin dedup)
- **AND** la snapshot no lleva etiqueta `kesto.change_manifest`
- **AND** el backup finaliza sin abortar
