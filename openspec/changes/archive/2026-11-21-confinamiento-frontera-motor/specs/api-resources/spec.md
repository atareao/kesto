# api-resources — delta

## Purpose

El contrato HTTP de los recursos de Kesto: rutas, tipos y códigos. Este delta añade el alcance de las rutas del target al borde HTTP, que es donde el fallo se ve en el formulario y no en la primera copia.

## ADDED Requirements

### Requirement: Las rutas del target se aceptan solo dentro de su alcance

El alta y la edición de un target SHALL comprobar el **alcance** de sus rutas, además de su presencia y su coherencia con el tipo. Cuando la comprobación falle, la petición SHALL contestar `400` nombrando el campo y NO SHALL escribirse ninguna fila.

- `host_path`, de un target `directory`, SHALL ser una ruta **absoluta**, **sin `..`** y SHALL quedar **bajo** la raíz de `KESTO_BACKUP_ROOT`, comparando **por componentes** y no por prefijo de cadena: `/backup_sources_x` empieza por `/backup_sources` como texto y **no** está dentro de él.
- `db_path`, de un target `sqlite`, SHALL ser una ruta **absoluta**, sin `..` y sin segmento vacío.

La comprobación del borde SHALL ser **textual** y NO SHALL exigir que la ruta exista: al dar de alta un directorio puede no estar montado todavía, y exigir su existencia convertiría el alta en una comprobación del despliegue. La canonicalización —que resuelve los enlaces simbólicos y es la que de verdad cierra el agujero— la hace el motor, no este borde.

La comprobación SHALL aplicarse por igual al alta y a la edición, porque las dos comparten el mismo punto de validación.

#### Scenario: Un host_path fuera de la raíz se rechaza

- **GIVEN** un alta de target `directory` cuyo `host_path` es `/` y una raíz de copias `/backup_sources`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `host_path`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Una raíz que solo coincide como prefijo no vale

- **GIVEN** un alta de target `directory` cuyo `host_path` es `/backup_sources_x` y una raíz de copias `/backup_sources`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `host_path`

#### Scenario: Un host_path con `..` se rechaza

- **GIVEN** un alta de target `directory` cuyo `host_path` es `/backup_sources/../etc` y una raíz de copias `/backup_sources`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `host_path`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un host_path relativo se rechaza

- **GIVEN** un alta de target `directory` cuyo `host_path` es `backup_sources/var_www`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `host_path`

#### Scenario: Un host_path bajo la raíz se acepta aunque no exista todavía

- **GIVEN** un alta de target `directory` cuyo `host_path` es `/backup_sources/var_www` y ese directorio no existe en el sistema de ficheros
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `201`
- **AND** la fila escrita SHALL tener ese `host_path`

#### Scenario: Un db_path con `..` se rechaza

- **GIVEN** un alta de target `sqlite` cuyo `db_path` es `/datos/../etc/base.db`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `db_path`

#### Scenario: Un db_path relativo se rechaza

- **GIVEN** un alta de target `sqlite` cuyo `db_path` es `datos/base.db`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `db_path`

#### Scenario: La edición aplica la misma regla

- **GIVEN** un target `directory` existente y un `PUT` que le pone un `host_path` fuera de la raíz
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `host_path`
- **AND** la fila NO SHALL cambiar
