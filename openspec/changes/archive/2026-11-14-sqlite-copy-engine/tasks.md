# Tasks

## SDD

- [x] Aprobar este change proposal (`sqlite-copy-engine`).
- [x] `openspec validate sqlite-copy-engine --strict` en verde.

## Preparación

- [x] Crear rama `feature/sqlite-copy-engine` desde `development`.

## TDD — base de datos y entidad

- [x] RED: test de esquema — `CHECK` de `target_type` admite `'sqlite'` y rechaza `'sqlite3'`; `backup_targets` tiene `db_path`; las filas previas sobreviven a la migración; los índices sobreviven.
- [x] GREEN: migración `0003_sqlite_target.sql` (reconstrucción de `backup_targets`) y variante `Sqlite` en `TipoDeTarget` con su `db_path`.
- [x] REFACTOR: clippy y fmt.

## TDD — coherencia y validación en el borde HTTP

- [x] RED: tests de `alta_desde` — `sqlite` exige `container_name` + `db_path`; rechaza `db_user`/`db_password`/`db_name`/`host_path`.
- [x] GREEN: variante `Sqlite` en `texto_del_tipo`, `tipo_del_cable` y `alta_desde`; campo `db_path` en los DTO del cable.
- [x] REFACTOR: clippy y fmt.

## TDD — extracción

- [x] RED: tests de `extraccion::ruta` — `sqlite` produce `Orden { cmd: ["sqlite3", "-readonly", db_path, ".dump"] }`.
- [x] GREEN: brazo `Sqlite` en `ruta` y `nombre_del_tipo`; `entorno_del_exec` sin variables para `Sqlite`.
- [x] REFACTOR: clippy y fmt.

## TDD — panel

- [x] RED: tests de `types.test.ts`, `ModalDeTarget.test.tsx` y `ajustes.test.tsx`.
- [x] GREEN: `TipoDeTarget`, `Target`, `AltaTarget` y `ValoresDeTarget` con `sqlite`/`db_path`; opción y campos condicionales en `ModalDeTarget`.
- [x] REFACTOR: lint y `tsc --noEmit`.

## Documentación

- [x] `docs/DEFINITION.md` §3 y §4.
- [x] Ejemplo de fuente SQLite en el `compose.yaml` de referencia.
- [x] `docs/FEATURE-014-sqlite-copy-engine.md`.

## Verificación

- [x] `just check` verde.
- [x] `just check-all` verde.
- [ ] Copia real de una base SQLite en uso (con WAL) que se restaura sin errores. **Pendiente de ejecución manual**: exige un motor de contenedores accesible, que este entorno no tiene.

## Cierre

- [ ] PR a `development`, CI en verde.
- [ ] Renombrado del plan a `plans/PLAN-014-COMPLETED.md`.
