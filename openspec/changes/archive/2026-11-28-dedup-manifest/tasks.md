# Tareas — Manifest de cambios pre-backup para deduplicación segura de volcados

## Fase 1 — Detección de cambios PostgreSQL

### RED — tests de contrato (deben fallar antes de implementar)

- [x] **T-1.1** Test de contrato del trait `ChangeDetector` (PostgreSQL):
      mismo manifest entre dos ejecuciones → `archivar_volcado` **no** sube bytes y crea una
      snapshot «sin cambios».
- [x] **T-1.2** Test de contrato: manifest distinto → `archivar_volcado` sube el volcado
      completo y la snapshot queda etiquetada con `kesto.change_manifest=<hash>` nuevo.
- [x] **T-1.3** Test de contrato: restaurar una snapshot «sin cambios» devuelve el contenido
      del **último volcado real** con ese manifest (no un vacío ni el contenido inicial).
- [x] **T-1.4** Ejecutar `cargo test` y confirmar que los tests nuevos fallan (RED) y que la
      suite existente sigue en verde.

### GREEN — implementación mínima

- [x] **T-1.5** Definir el trait `ChangeDetector` en `src/engine/extraccion.rs` con el método
      que calcula el manifest de cambios para un repositorio/base de datos.
- [x] **T-1.6** Implementar la detección PostgreSQL: `pg_dump --schema-only` + checksum de
      datos por tabla → hash estable del manifest.
- [x] **T-1.7** Extender `archivar_volcado` en `src/rustic/repositorio.rs` con un parámetro
      `change_manifest: Option<...>` y aplicar la etiqueta `kesto.change_manifest=<hash>`.
- [x] **T-1.8** Ajustar `parent_opts.force(true)`: forzar escritura completa solo cuando el
      manifest cambia o no se puede calcular; deduplicar cuando el manifest es idéntico.
- [x] **T-1.9** Implementar en `src/rustic/retencion.rs` la lógica de duplicados lógicos por
      `kesto.change_manifest` (agrupar y ajustar el espacio liberado).
- [x] **T-1.10** Ejecutar `cargo test` y confirmar 100% verde.

### REFACTOR — limpieza y validación

- [x] **T-1.11** Limpiar nombres y estructura del trait/implementaciones; documentar el
      manifest en comentarios y en `docs/FEATURE-NNN-dedup-manifest.md`.
- [x] **T-1.12** `just check` en verde (fmt + clippy + tests).
- [x] **T-1.13** `just check-all` en verde (incluye `cargo deny` y `just ui-check`).
- [x] **T-1.14** `npx tsc --noEmit` y `pnpm test` en `ui/` en verde (columna «Cambios»).

- [x] **T-1.15** GREEN: almacenar el manifest como tag `kesto.change_manifest=<hash>` en `SnapshotFile.tags` preservando `label` de tarea; añadir `change_manifest: Option<String>` a `GET /api/v1/snapshots` y su test de contrato.

## Fase 2 — Detección de cambios MySQL / MariaDB

- [x] **T-2.1** RED: tests de contrato del `ChangeDetector` para MySQL/MariaDB (mismo
      manifest → no sube; manifest distinto → sube y etiqueta).
- [x] **T-2.2** GREEN: implementar detección con `mysqldump --no-data` + `CHECKSUM TABLE`.
- [x] **T-2.3** GREEN: reutilizar la lógica de etiquetado y retención de la Fase 1.
- [x] **T-2.4** REFACTOR: `just check-all` en verde y documentación de la Fase 2.

## Panel

- [x] **T-3.1** Añadir la columna «Cambios» a la vista de snapshots en `ui/`.
- [x] **T-3.2** Test de componente (`@testing-library/react`) para la columna «Cambios».
- [x] **T-3.3** Crear `docs/FEATURE-NNN-dedup-manifest.md` con qué es, ficheros implicados,
      cómo funciona, variables/config, decisiones y follow-ups.

## Cierre

- [x] **T-4.1** Marcar todas las tareas y archivar el change (`openspec archive dedup-manifest`).
