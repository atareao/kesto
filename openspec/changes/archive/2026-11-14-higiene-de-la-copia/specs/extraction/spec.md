## ADDED Requirements

### Requirement: El nodo del volcado declara un modo de fichero utilizable

El nodo que representa el volcado dentro de la instantánea SHALL declarar un modo de fichero que permita **leerlo al restaurarlo**, y NO SHALL quedarse con el modo por defecto del `Metadata` de la biblioteca. Concretamente el modo SHALL ser `0o644`.

El porqué: el volcado no existe como fichero en ningún sistema de ficheros de Kesto —llega en streaming a la biblioteca—, así que su nodo se construye a mano y su `Metadata` por defecto no describe un fichero utilizable. Ese modo queda grabado en la instantánea y lo hereda el fichero al restaurarlo, de modo que un `.sql` restaurado sale sin permisos de lectura y no se puede usar sin intervenir a mano. Los ficheros de un target `directory` sí conservan sus permisos porque son ficheros de verdad; el volcado es la única entrada cuya metadata la elige Kesto.

#### Scenario: Un volcado restaurado es legible
- **GIVEN** una instantánea de un volcado
- **WHEN** se restaura
- **THEN** el fichero restaurado SHALL ser legible por su dueño, su grupo y el resto
- **AND** su modo SHALL ser `0o644`

#### Scenario: El nodo del volcado no lleva el modo por defecto de la biblioteca
- **GIVEN** un origen de volcado
- **WHEN** se inspecciona el nodo con el que se entrega a la biblioteca
- **THEN** su modo NO SHALL ser el modo por defecto del `Metadata`
- **AND** SHALL ser `0o644`
