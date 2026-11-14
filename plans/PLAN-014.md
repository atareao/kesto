# PLAN-014 — SQLite como motor de copia (FEATURE-014)

- **Issue:** [#11](https://github.com/atareao/kesto/issues/11)
- **Change SDD:** `openspec/changes/sqlite-copy-engine/`
- **Estado:** borrador del proposal — a la espera de aprobación

## Objetivo

Añadir **SQLite** como tipo de origen de copia, junto a `mariadb`, `postgresql` y `directory`, con copia **consistente en caliente** (sin `cp`, válida con WAL y con la base en uso) y restaurable sin errores.

## Alcance

1. **Esquema y entidad:** migración `0003` que añade `db_path` y amplía el `CHECK` de `target_type` (reconstruye la tabla); variante `Sqlite` en `TipoDeTarget `.
2. **API:** coherencia de campos para `sqlite` (`container_name` + `db_path`); `db_path` en los DTO.
3. **Extracción:** `Ruta::PorContenedor` con el comando `sqlite3 <db_path> .dump`, streaming a `rustic`.
4. **Panel:** opción `sqlite` y campos condicionales en el formulario de fuente.
5. **Docs:** §3/§4 del SRS, `compose.yaml` y documento de feature.

## Decisión pendiente

Mecanismo de copia: `.dump` (recomendado; sin ficheros temporales, restauración trivial) frente a `.backup` (binario fiel, pero exige un temporal dentro del contenedor). Se confirma en la aprobación del proposal.

## Fases (SDD + TDD)

1. `openspec validate sqlite-copy-engine --strict `.
2. Rama `feature/sqlite-copy-engine` desde `development`.
3. Por bloque (db/entidad, API, extracción, panel): RED → GREEN → REFACTOR con `just check`.
4. Documentación y verificación (copia real de una base en uso).
5. PR a `development`, CI en verde.

## Verificación

- `just check` (fmt + clippy + tests) y `just check-all` verdes.
- Copia de una base SQLite en uso, con WAL, restaurada sin errores.