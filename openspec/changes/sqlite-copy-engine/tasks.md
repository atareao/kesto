# Tasks

## SDD

- [ ] Aprobar este change proposal (`sqlite-copy-engine`).
- [ ] `openspec validate sqlite-copy-engine --strict` en verde.

## Preparación

- [ ] Crear rama `feature/sqlite-copy-engine` desde `development`.

## TDD — base de datos y entidad

- [ ] RED: test de esquema — `CHECK` de `target_type` admite `'sqlite'` y rechaza `'sqlite3'`; `backup_targets` tiene `db_path`; las filas previas sobreviven a la migración; los índices sobreviven. Deben fallar.
- [ ] GREEN: migración `0003_sqlite_target.sql` (reconstrucción de `backup_targets` con `-- no-transaction` si hace falta para `PRAGMA foreign_keys`) y variante `Sqlite` en `TipoDeTarget` con su `db. db_path`.
- [ ] REFACTOR: clippy y fmt.

## TDD — coherencia y validación en el borde HTTP

- [ ] RED: tests de `alta_desde` — `sqlite` exige `container_name` + `db_path`; rechaza `db_user`/`db_password`/`db_name`/`host_path`; `sqlite` sin `db_path` contesta `400` nombrando el campo. Deben fallar.
- [ ] GREEN: variante `Sqlite` en `texto_del_tipo`, `tipo_del_cable` y el brazo de `alta_desde`; campo `db_path` en los DTO del cable.
- [ ] REFACTOR: clippy y fmt.

## TDD — extracción

- [ ] RED: tests de `extraccion::ruta` — `sqlite` produce `Orden { cmd: ["sqlite3", db_path, ".dump"] }`; `db_path` ausente o en blanco devuelve `CampoQueFalta`/`CampoEnBlanco`. Deben fallar.
- [ ] GREEN: brazo `Sql. db_path` en los DTO del cable.
- [ ] REFACTOR: clippy y fmt.

## TDD — panel

- [ ] RED: tests de `types.test.ts` y `ModalDeTarget.test.tsx` — el tipo incluye `'sqlite'` y `db_path`; el selector ofrece las cuatro opciones; elegir `sqlite` muestra `container_name`/`db_path` y oculta los demás. Deben fallar.
- [ ] GREEN: `TipoDeTarget`, `Target`, `AltaTarget` y `ValoresDeTarget` con `sqlite`/`db_path`; opción y campos condicionales en `ModalDeTarget`.
- [ ] REFACTOR: lint y `tsc --noEmit `.

## Documentación

- [ ] `docs/DEFINITION.md` §3 (CHECK y columna) y §4 (comando SQLite).
- [ ] Ejemplo de fuente SQLite en el `compose.yaml` de referencia.
- [ ] `docs/FEATURE-014-sqlite-copy-engine.md `.

## Verificación

- [ ] `just check` verde.
- [ ] `just check-all` verde.
- [ ] Copia real de una base SQLite en uso (con WAL) que se restaura sin errores.

## Cierre

- [ ] PR a `development`, CI en verde.
- [ ] `docs/FEATURE-014-sqlite-copy-engine.md` y renombrado del plan a `plans/PLAN-014-COMPLETED.md`.