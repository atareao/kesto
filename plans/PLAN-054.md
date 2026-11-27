# PLAN-054: feat manifest de cambios pre-backup para deduplicación segura

## Objetivo

Implementar un **manifest de cambios pre‑backup** que permita a Kesto deduplicar de forma segura los volcados de bases de datos contraídos por rustic. El problema actual es que los dumps por `stdout` carecen de metadatos reales (tamaño 0, sin mtime/inode), por lo que `rustic_core` los ve como idénticos y descarta la segunda copia, perdiendo datos nuevos. La solución pasa por comparar el contenido antes de cada backup y registrar un manifest hash que permita detectar si ha habido cambios reales.

## Problemática

- `archivar_volcado` usa `parent_opts.force(true)` para evitar que rustic dedupe dumps idénticos.
- Sin metadatos, cada volcado completo sube gigabytes a S3 aunque el contenido no haya cambiado.
- La deduplicación de rustic es por *chunks* y requiere metadatos (size, mtime) que los streams por stdout no tienen.

## Solución propuesta

1. **Antes** de invocar `archivar_volcado`, ejecuta un *diff* ligero por motor:
   - **PostgreSQL:** `pg_dump --schema-only` + checksum de datos (p.ej. `pg_checksum` o hash de `SELECT count(*)` por tabla).
   - **MySQL/MariaDB:** `mysqldump --no-data` + `CHECKSUM TABLE` o posición binlog.
2. **Genera** un hash del manifest (`kesto.change_manifest=<sha256>`) y lo adjunta a la instantánea como etiqueta (`SnapshotFile.labels`).
3. **Retención** (`src/rustic/retencion.rs`): si dos snapshots tienen el mismo `kesto.change_manifest`, tratarlas como duplicados lógicos: al retirar una, no liberar tamaño (los chunks se comparten); al retirar la última con ese manifest, liberar los chunks.
4. **Panel (`ui/`)**: columna "Cambios" que muestra si el último backup tuvo cambios o fue idéntico al anterior; badge "sin cambios" / "nuevos datos".

## Criterios de aceptación

| # | Criterio |
|---|----------|
| 1 | Con un dump idéntico al anterior: no se suben bytes nuevos a S3 y la snapshot lleva `kesto.change_manifest` igual. |
| 2 | Con un dump distinto: se sube el dump completo y la snapshot lleva un manifest nuevo (`kesto.change_manifest=<nuevo-hash>`). |
| 3 | La restauración de una snapshot "sin cambios" devuelve exactamente el contenido del último dump real guardado. |
| 4 | `just check-all` RC=0 y todos los tests TDD (RED→GREEN→REFACTOR) pasan. |
| 5 | No introducir `RUSTSEC-2026-0555` ni `chrono`/`chrono-tz`/`croner`/`tokio-cron-scheduler` como dependencias directas. |

## Alcance de la entrega (ficheros versionados)

- **`plans/PLAN-054.md`** — presente.
- **`openspec/changes/dedup-manifest/proposal.md`** — change proposal.
- **`openspec/changes/dedup-manifest/tasks.md`** — checklist TDD.
- **`openspec/changes/dedup-manifest/specs/engine/spec.md`** — spec delta motor.
- **`openspec/changes/dedup-manifest/specs/rustic/spec.md`** — spec delta retención.
- **`docs/FEATURE-dedup-manifest.md`** — documentación de la feature (opcional pero recomendado).
- **Issue GitHub #206** — punto de entrada (ya creado).

## Dependencias y restricciones

- **Motor:** fase 1 solo PostgreSQL (posible expansión a MySQL/MariaDB después).
- **Herramientas:** `pg_dump` ya está disponible en los runners; `mysqldump` instalar si no está.
- **Sin cambios** al algoritmo de chunks de `rustic_core` ni a su formato de snapshot.
- **Compatibilidad:** si el manifest no está presente, comportamiento anterior (`force(true)`) se mantiene por defecto.

## Próximos pasos

1. **Abrir issue** → ya creado #206.
2. **Change proposal** en `openspec/changes/dedup-manifest/` (ficheros generados por el subagente).
3. **PLAN-054.md** creado.
4. **Especificar `ChangeDetector` trait** en `src/engine/extraccion.rs`.
5. **Implementar `pg_dump` + hash** en `src/engine/backends/postgres.rs` (o módulo nuevo).
6. **Añadir `change_manifest`** a `archivar_volcado` y a `SnapshotFile.labels`.
7. **Lógica de duplicados lógicos** en `src/rustic/retencion.rs`.
8. **Panel**: columna "Cambios", badge visual.
9. **Tests TDD** RED → GREEN → REFACTOR.
10. `just check-all` RC=0 antes de merge a `main`.

## Historial

- Creado: `2026-11-27` — issue #206, plan PLAN-054.md, change OpenSpec dedup-manifest.
