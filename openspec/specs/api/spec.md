# api Specification

## Purpose
TBD - created by archiving change dedup-manifest. Update Purpose after archive.

## Requirements

### Requirement: El listado de snapshots publica el manifest de cambios

`GET /api/v1/snapshots` DEBE incluir en cada elemento un campo
`change_manifest: Option<String>` con el hash del manifest de cambios de esa snapshot,
sin el prefijo `kesto.change_manifest=`. El valor DEBE extraerse del tag
`kesto.change_manifest=<hash>` de la snapshot y DEBE ser `null` cuando la snapshot no
lo lleve —incluidas las anteriores a esta feature y las escritas por otras herramientas—.

El campo existente `label` NO DEBE cambiar de significado: sigue siendo el identificador
de la tarea. Por tanto el manifest DEBE almacenarse en `SnapshotFile.tags` y NO en
`SnapshotFile.label`.

#### Scenario: Snapshot con manifest

- **GIVEN** una snapshot con el tag `kesto.change_manifest=abc123def456`
- **WHEN** un cliente consulta `GET /api/v1/snapshots`
- **THEN** el elemento correspondiente devuelve `change_manifest: "abc123def456"`
- **AND** `label` sigue siendo el identificador de la tarea, sin cambios

#### Scenario: Snapshot sin manifest

- **GIVEN** una snapshot sin el tag `kesto.change_manifest`
- **WHEN** un cliente consulta `GET /api/v1/snapshots`
- **THEN** `change_manifest` es `null`
- **AND** el panel puede mostrar «—» sin inventar un valor
