# panel Specification

## Purpose
TBD - created by archiving change dedup-manifest. Update Purpose after archive.

## Requirements

### Requirement: La vista de snapshots muestra la columna «Cambios»

La tabla de snapshots DEBE incluir una columna «Cambios» alimentada por el campo
`change_manifest` del cable. Para cada snapshot:

- si `change_manifest` no es una cadena —`null` o ausente—, la celda muestra «—» y
  ningún badge;
- si la snapshot anterior de la misma tarea (`job_name`) tiene el mismo
  `change_manifest`, la celda muestra el badge «sin cambios»;
- si la anterior tiene otro manifest de cadena y no existe ninguna otra posibilidad
  indeterminada, la celda muestra el badge «nuevos datos»;
- si la anterior no existe en el conjunto cargado o su `change_manifest` no es una
  cadena, el estado es indeterminado: la celda muestra «—» y ningún badge;
- en los dos casos con badge, la celda muestra el hash abreviado a sus 8 primeros
  caracteres y ofrece el valor completo como título accesible (`title`).

Cuando una snapshot no tiene `job_name`, se compara con la anterior por `time`.

Una fila es indeterminada cuando su predecesora no está cargada: ocurre en la fila más
antigua de una página distinta de la primera, y también en la fila más antigua de la
primera página cuando el listado tiene más filas (`total > snapshots.length`) o viene
truncado. Llamar «nuevos datos» a esa fila sería inventar un cambio.

#### Scenario: Dos snapshots seguidas con el mismo manifest

- **GIVEN** dos snapshots consecutivas de la misma tarea con `change_manifest=M`
- **WHEN** se muestra la vista de snapshots
- **THEN** la segunda muestra el badge «sin cambios» y `M` abreviado a 8 caracteres

#### Scenario: El manifest cambia entre dos snapshots

- **GIVEN** una snapshot con `change_manifest=M` seguida de otra con `change_manifest=M2`, `M2` distinto de `M`
- **WHEN** se muestra la vista de snapshots
- **THEN** la segunda muestra el badge «nuevos datos» y `M2` abreviado a 8 caracteres

#### Scenario: Snapshot sin manifest

- **GIVEN** una snapshot con `change_manifest: null`
- **WHEN** se muestra la vista de snapshots
- **THEN** la celda «Cambios» muestra «—» y no muestra ninguno de los dos badges

#### Scenario: Predecesora sin manifest

- **GIVEN** una snapshot con `change_manifest=M` cuya predecesora inmediata tiene `change_manifest: null`
- **WHEN** se muestra la vista de snapshots
- **THEN** la celda de la segunda muestra «—» y ninguno de los dos badges

#### Scenario: Límite de paginación sin predecesor cargado

- **GIVEN** una fila más antigua de una página distinta de la primera, o de la primera página de un listado con más filas o truncado, con `change_manifest=M`
- **WHEN** se muestra la vista de snapshots
- **THEN** esa fila muestra «—» y ninguno de los dos badges
- **AND** el resto de las filas de la página se clasifican contra su predecesora visible
