# PLAN-014 — SQLite como motor de copia (FEATURE-014)

- **Issue:** [#11](https://github.com/atareao/kesto/issues/11)
- **Change SDD:** `openspec/changes/sqlite-copy-engine/`
- **Estado:** implementado en `feature/sqlite-copy-engine`; PR en curso

## Objetivo

Añadir **SQLite** como tipo de origen de copia, junto a `mariadb`, `postgresql` y `directory`, con copia **consistente en caliente** (sin `cp`, válida con WAL y con la base en uso) y restaurable sin errores.

## Alcance

1. **Esquema y entidad:** migración `0003` que añade `db_path` y amplía el `CHECK` de `target_type` (reconstruye la tabla); variante `Sqlite` en `TipoDeTarget`.
2. **API:** coherencia de campos para `sqlite` (`container_name` + `db_path`); `db_path` en los DTO.
3. **Extracción:** `Ruta::PorContenedor` con el comando `sqlite3 -readonly <db_path> .dump`, streaming a `rustic`.
4. **Panel:** opción `sqlite` y campos condicionales en el formulario de fuente.
5. **Docs:** §3/§4 del SRS, `compose.yaml` y documento de feature.

## Decisión

Mecanismo de copia: **`.dump`** (sin ficheros temporales, restauración trivial), con `-readonly` para que una ruta inexistente falle en vez de crear una base vacía y salir con código 0.

## Fases (SDD + TDD) — completadas

1. `openspec validate sqlite-copy-engine --strict` en verde.
2. Rama `feature/sqlite-copy-engine` desde `development`.
3. Por bloque (db/entidad, API, extracción, panel): RED → GREEN → REFACTOR con `just check`.
4. Documentación y code review (rust-reviewer + react-reviewer, sin bloqueantes).
5. `just check-all` en verde.

## Verificación

- `just check` (fmt + clippy + tests) y `just check-all` verdes.
- **Pendiente (manual, requiere motor de contenedores):** copia de una base SQLite en uso, con WAL, restaurada sin errores.
