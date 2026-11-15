# Tasks — nombre-de-la-tarea

## 0. Preparación
- [ ] 0.1 `just check` y `just ui-check` en verde antes de tocar código.
- [ ] 0.2 Confirmar el siguiente número de migración libre (`0005`) y el plan asociado (`plans/PLAN-020.md`).

## 1. RED
- [ ] 1.1 Rust: con las migraciones aplicadas, la tabla `backup_jobs` tiene la columna `name` de tipo `TEXT` y admite `NULL` en un `INSERT` directo sin especificarla.
- [ ] 1.2 Rust: el alta de tarea guarda el `name` recortado y el listado lo publica.
- [ ] 1.3 Rust: un `name` en blanco se guarda como `null` (no como cadena vacía).
- [ ] 1.4 Rust: un `name` de más de 120 caracteres contesta `400` con la envoltura y nombra `name`.
- [ ] 1.5 Rust: `GET /api/v1/executions` publica `job_name` por fila y lo deja a `null` cuando la tarea no tiene nombre.
- [ ] 1.6 Rust: `GET /api/v1/snapshots` publica `job_name` por instantánea, sin tocar la `label`.
- [ ] 1.7 UI: el formulario de Tareas tiene el campo «Nombre» y la tabla la columna «Nombre» (vacía si no lo tiene).
- [ ] 1.8 UI: el Histórico muestra la columna «Tarea» con `job_name` o el identificador, y el filtro de tarea se etiqueta con el nombre de la tarea.
- [ ] 1.9 UI: Snapshots muestra la columna «Tarea» entre la fecha y la etiqueta, con la «Etiqueta» intacta.

## 2. GREEN
- [ ] 2.1 Migración `migrations/0005_nombre_de_tarea.sql` (`ALTER TABLE backup_jobs ADD COLUMN name TEXT`, sin reconstruir la tabla).
- [ ] 2.2 `docs/DEFINITION.md`: columna `name` en el §3.
- [ ] 2.3 `src/db/entities/job.rs`: campo `name` anulable.
- [ ] 2.4 `src/api/jobs.rs`: alta, edición y listado con `name`; recorte, blanco a `null` y tope de 120 caracteres.
- [ ] 2.5 `src/api/executions.rs`: `job_name` por fila, resuelto por el vínculo con `backup_jobs`.
- [ ] 2.6 `src/api/snapshots.rs`: `job_name` por instantánea, por el vínculo `snapshot → job_executions → backup_jobs`.
- [ ] 2.7 `ui/src`: tipos y pantallas de Tareas, Histórico y Snapshots (campo, columnas y filtro por nombre).

## 3. REFACTOR
- [ ] 3.1 `just check` en verde (fmt, clippy, test).
- [ ] 3.2 `just ui-check` en verde (typecheck, tests, lint, test, build).
- [ ] 3.3 `openspec validate --all --strict`.

## 4. Cierre
- [ ] 4.1 Archivar el change y fundir su delta en `openspec/specs/` (`just spec-archive nombre-de-la-tarea`).
- [ ] 4.2 Documentar la feature en `docs/FEATURE-020-nombre-de-la-tarea.md`.
- [ ] 4.3 Commit y PR a `development`.
