# api-resources — delta

## Purpose

El borde HTTP de los recursos: los recursos, sus campos y sus validaciones. Este delta cierra SEC-13 (campos de texto sin cota ni recorte), SEC-14 (retención que el motor no lee) y el NIT del `container_name` de SEC-22 (inyección de ruta en la API de Docker).

## ADDED Requirements

### Requirement: Los campos de texto se recortan y tienen una cota por campo

Las rutas de alta y edición de `/repositories` y `/targets` SHALL **recortar** los espacios de los extremos de cada campo de texto antes de guardarlo y de medirlo, y SHALL rechazar con `400` y la envoltura, **nombrando el campo** y **sin repetir el valor**, un campo que tras recortar supere su cota.

Las cotas SHALL ser: `name` 120 caracteres; `container_name` 128; `db_user`, `db_name` y `bucket` 63; `region` 64; `access_key_id` 128; `endpoint` 2048; `db_path` 1024; `host_path` 4096.

Un campo **obligatorio** que tras recortar quede vacío SHALL contestar `400` nombrando el campo —es la misma regla que la falta del campo—. Un campo **opcional** (por ejemplo `db_name`) que se informe vacío o de solo espacios SHALL guardarse como `NULL` en su columna, que es lo que el §3 significa para «ausente»; NO SHALL guardarse la cadena vacía.

Las cotas SHALL medirse en **caracteres** y no en bytes: contar bytes rechazaría un nombre con acentos que el formulario deja escribir. Los campos de **secreto** (`secret_access_key`, `passphrase`, `db_password`) NO SHALL tener cota de campo.

El porqué: la tabla del §3 no acota sus columnas `TEXT`, así que sin esta regla un `bucket` de miles de caracteres o un `name` de solo espacios entran en la base y reaparecen más tarde —en un listado, en una etiqueta de snapshot o en una orden del motor—. Y el recorte es lo que hace que el tope se mida sobre lo que de verdad se guarda: los espacios de los extremos no son parte del valor.

#### Scenario: Un name de solo espacios se rechaza
- **GIVEN** un alta de target con `name` a una cadena de solo espacios
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar el campo `name`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un name por encima de la cota se rechaza sin repetir el valor
- **GIVEN** un alta de repositorio con un `name` de 121 caracteres distinguible
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400`
- **AND** el motivo SHALL nombrar el campo `name`
- **AND** el cuerpo NO SHALL contener el valor recibido

#### Scenario: Un campo se guarda recortado
- **GIVEN** un alta válida con `container_name` con espacios a ambos lados
- **WHEN** se guarda y se lee la fila
- **THEN** el valor almacenado SHALL ser el recortado, sin los espacios de los extremos
- **AND** SHALL conservarse el resto de campos

#### Scenario: Un db_name en blanco se guarda como NULL
- **GIVEN** un alta de target de base de datos con `db_name` a una cadena de solo espacios
- **WHEN** se escribe la fila y se mira la columna `db_name` con SQL directo
- **THEN** el valor almacenado SHALL ser `NULL`
- **AND** NO SHALL ser la cadena vacía ni la cadena en blanco

#### Scenario: Un bucket por encima de su cota se rechaza
- **GIVEN** un alta de repositorio con un `bucket` de 64 caracteres
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar el campo `bucket`

### Requirement: El nombre de un contenedor se valida contra el juego de caracteres de Docker

`container_name`, cuando se informe en un alta o una edición de target, SHALL cumplir el juego de caracteres con el que Docker nombra y localiza contenedores —una primera letra alfanumérica seguida de letras, dígitos, `_`, `.` o `-` (`[A-Za-z0-9][A-Za-z0-9_.-]*`)—. Un valor que no lo cumpla SHALL contestar `400` con la envoltura, **nombrando `container_name`** y **sin repetir el valor**.

El porqué: el cliente de la API de Docker (bollard) interpola el nombre **sin codificar** en la ruta de la petición (`/containers/{container_name}/exec`). Un nombre con `/`, `..` o `?` no da un error de nombre: mueve la petición a otro punto de la API de Docker. El juego de caracteres de Docker no admite ninguno de esos caracteres, así que exigirlo cierra la inyección de ruta y, de paso, hace que el nombre que se escribe sea uno que un contenedor puede tener de verdad. Cubre también los identificadores —largos o cortos—, que son hexadecimales.

#### Scenario: Un container_name con una barra se rechaza
- **GIVEN** un alta de target `sqlite` con `container_name` a `../../v1.40/containers/x` y un `db_path` válido
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar el campo `container_name`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un container_name válido se acepta
- **GIVEN** un alta de target `mariadb` con `container_name` a `contenedor_de-la.base-01` y el resto de campos válidos
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `201` con la envoltura

### Requirement: El alta y la edición de una tarea rechazan una retención negativa que no sea -1

`POST /api/v1/jobs` y `PUT /api/v1/jobs/:id` SHALL aceptar, en cada una de las tres reglas `keep_daily`, `keep_weekly` y `keep_monthly`, un valor `>= 0` o el valor `-1` —que es la forma que tiene la biblioteca de retención de decir «conservar todas»—. SHALL rechazar con `400` y con la envoltura, nombrando la columna, cualquier valor menor que `-1` o mayor que el entero de 32 bits que la biblioteca usa, sin escribir la fila.

El porqué: el motor (`src/rustic/retencion.rs`) lee `-1` como «conservar todo» y rechaza cualquier otro negativo; el borde tiene que rechazar **lo mismo**, para que un valor que el motor no sabría leer se pare en el formulario en vez de aparecer como un fallo de la primera copia. La regla no sustituye al guardarraíl del motor —una fila escrita por debajo de la API no pasa por aquí—, y las dos capas existen porque protegen de dos caminos distintos. El `-1` se conserva porque hoy es la forma legítima de pedir «conservar todo» y rechazarlo rompería configuraciones válidas.

#### Scenario: Una regla negativa distinta de -1 se rechaza
- **GIVEN** un alta de tarea con `keep_daily` a `-5` y el resto de campos válidos
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar la columna `keep_daily`
- **AND** NO SHALL quedar fila en la base

#### Scenario: El -1 se acepta como «conservar todo»
- **GIVEN** un alta de tarea con `keep_daily` a `-1` y las otras dos reglas informadas
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `201` con la envoltura
- **AND** la fila SHALL quedar escrita con `keep_daily` a `-1`

#### Scenario: Una regla por encima del entero de la biblioteca se rechaza
- **GIVEN** un alta de tarea con `keep_weekly` a un valor mayor que el mayor entero de 32 bits
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL nombrar la columna `keep_weekly`
