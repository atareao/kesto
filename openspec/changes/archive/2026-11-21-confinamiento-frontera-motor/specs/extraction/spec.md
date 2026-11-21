# extraction — delta

## Purpose

Cómo se extrae el contenido de un target: los comandos por tipo, el camino del volcado hasta la biblioteca de copia y las garantías de consistencia. Este delta añade el alcance de la ruta de un target de directorio, que hoy se lee sin más límite que su forma.

## MODIFIED Requirements

### Requirement: Un target de directorio no pasa por Docker

Un target de tipo `directory` SHALL copiarse leyendo su `host_path` directamente desde el sistema de ficheros del proceso, sin ejecutar ningún `exec`, y su `host_path` SHALL interpretarse como una ruta del sistema de ficheros **de Kesto**.

El `host_path` NO SHALL leerse fuera de la raíz de copias. El motor SHALL **canonicalizar** la ruta —resolviendo los enlaces simbólicos de todos sus componentes— y SHALL rechazar la copia cuando el resultado no quede bajo la raíz de `KESTO_BACKUP_ROOT`, comparando **por componentes**, antes de leer un solo byte. La regla completa, con su porqué, vive en la capability `confinamiento`; aquí se declara porque es este camino el que la aplica.

El §4 fija los comandos de las dos bases y **no describe el flujo de un directorio**, así que esta parte se declara como añadido al SRS. Y la lectura de `host_path` es la única que tiene datos detrás: la alternativa —que fuera una ruta del host— exigiría que Kesto conociera un mapeo host→contenedor que nadie le ha dado, mientras que el `compose` del §8 ya monta los directorios del host dentro de Kesto en solo lectura, en `/backup_sources/...`. Lo que Kesto ve es lo que copia.

Que un directorio no necesite Docker tiene una consecuencia útil: un target de directorio se puede copiar aunque el socket de Docker esté caído.

#### Scenario: Un directorio se copia sin ejecutar nada en un contenedor

- **GIVEN** un target `directory` con un `host_path` que existe, es legible y queda bajo la raíz de copias
- **WHEN** se ejecuta la copia
- **THEN** SHALL completarse
- **AND** NO SHALL haberse creado ningún `exec`

#### Scenario: El host_path se lee en el espacio de ficheros de Kesto

- **GIVEN** un target `directory` cuyo `host_path` apunta a un directorio visible desde el proceso y bajo la raíz de copias
- **WHEN** se ejecuta la copia
- **THEN** el snapshot SHALL contener el contenido de ese directorio
- **AND** SHALL contenerlo con la ruta con la que se montó, no con la del host

#### Scenario: Un directorio se copia con el socket de Docker caído

- **GIVEN** el socket de Docker sin responder y un target de tipo `directory` cuyo `host_path` queda bajo la raíz
- **WHEN** se ejecuta la copia
- **THEN** SHALL completarse

#### Scenario: Un host_path fuera de la raíz no se lee

- **GIVEN** un target `directory` cuyo `host_path` no queda bajo la raíz de copias
- **WHEN** se ejecuta la copia
- **THEN** SHALL acabar en `failed`
- **AND** el motivo SHALL nombrar el campo `host_path`
- **AND** NO SHALL leerse ningún byte del directorio
