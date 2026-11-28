# rustic Specification

## Purpose
TBD - created by archiving change dedup-manifest. Update Purpose after archive.

## Requirements

### Requirement: La retención trata snapshots con el mismo manifest como duplicados lógicos

Cuando varias snapshots comparten el mismo valor de `kesto.change_manifest`, la retención
DEBE tratarlas como **duplicados lógicos** del mismo contenido. Mientras quede al menos una
snapshot con ese manifest, retirar otra con el mismo manifest NO DEBE liberar el tamaño de
los chunks compartidos, porque siguen en uso. Solo al retirar la última snapshot con ese
manifest se DEBE liberar el tamaño de los chunks asociados.

Las snapshots sin `kesto.change_manifest` se tratan como contenido único y no se agrupan.

#### Scenario: Retirar una de dos snapshots con el mismo manifest no libera tamaño

- **GIVEN** dos snapshots etiquetadas con el mismo `kesto.change_manifest=M` que comparten
  los mismos chunks
- **WHEN** la retención retira una de las dos snapshots
- **THEN** el tamaño reportado como liberado NO incluye los chunks compartidos
- **AND** los chunks siguen disponibles para la snapshot restante con manifest `M`

#### Scenario: Retirar la última snapshot con un manifest libera los chunks

- **GIVEN** una única snapshot etiquetada con `kesto.change_manifest=M` que aún conserva
  chunks en uso
- **WHEN** la retención retira esa última snapshot con manifest `M`
- **THEN** el tamaño liberado incluye los chunks asociados a `M`
- **AND** ninguna snapshot con manifest `M` permanece en el repositorio
