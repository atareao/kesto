# PLAN-020 — Nombre de la tarea en el histórico y en los snapshots

**Issue:** #65 — ✨ feat: mostrar la tarea en el histórico y en los snapshots
**Base:** `development` tras el merge de #72 (release v0.7.0 publicada)
**Rama:** `feature/tarea-en-historico-y-snapshots` (desde `development`)
**Change SDD:** `openspec/changes/nombre-de-la-tarea/`
**Estado:** Completado — implementado y verificado (TDD RED→GREEN; `just check-all` y `openspec validate --all --strict` en verde), auditoría de backend y UI sin bloqueantes; PR a `development`

## 1. Objetivo

Que el operador sepa a qué **tarea** pertenece cada ejecución y cada snapshot. Hoy no puede: `backup_jobs` no tiene nombre, así que el Histórico etiqueta cada tarea con el nombre del **target** —proxy que confunde dos tareas sobre el mismo target— y los Snapshots muestran la «Etiqueta», que es el `job_id` en UUID.

## 2. Contexto

- `backup_jobs` (`migrations/0001_init.sql:34-46`, `docs/DEFINITION.md` §3) NO tiene `name`; la entidad `Tarea` (`src/db/entities/job.rs:21-40`) tampoco.
- El alta y la edición de tarea **comparten** struct: `AltaDeTareaDelCable` (`src/api/jobs.rs:69-86`) con `#[serde(deny_unknown_fields)]`; un campo nuevo hay que añadirlo ahí o el `POST`/`PUT` lo rechaza con `400`.
- El Histórico construye el filtro «Tarea» con `nombresDeTarget.get(tarea.target_id)` (`ui/src/pages/historico.tsx:48-51`) y no tiene columna de tarea.
- Los Snapshots muestran `label` (el `job_id`) como «Etiqueta» (`ui/src/pages/snapshots.tsx:364-373`).
- El vínculo instantánea→tarea ya existe y se usa para el target: `snapshot → job_executions → backup_jobs → backup_targets` (`src/api/snapshots.rs`, requisito «El listado de snapshots declara… el target»).
- El formulario de tarea vive en `ui/src/components/ModalDeTarea/ModalDeTarea.tsx` (no en `pages/tareas.tsx`).
- Siguiente migración libre: `0005`.

## 3. Decisiones de diseño (aprobadas por el usuario)

- **D1 — Se añade `name` a `backup_jobs`, anulable.** Alternativa descartada: exponer solo el UUID (no es legible) o etiquetar con el target + id corto (no distingue dos tareas sobre el mismo target y no da un nombre propio). `name TEXT` anulable por migración `0005` con `ALTER TABLE ... ADD COLUMN`, **sin reconstruir la tabla** (se reconstruye solo cuando cambia un `CHECK`, ver `0003_sqlite_target.sql`). Se añade al §3 del SRS.
- **D2 — El nombre es opcional y no único.** Sin nombre, el panel identifica la tarea por su identificador. `name` se recorta; en blanco → `null`; más de **120** caracteres → `400` nombrando `name`.
- **D3 — No se toca la `label` de los snapshots.** Sigue siendo el `job_id`; cambiar la etiqueta afectaría a snapshots ya existentes y al mapeo de la restauración (que resuelve por `snapshot_id`). El nombre de la tarea se resuelve por el vínculo.
- **D4 — Los listados publican `job_name`.** `GET /executions` añade `job_name` por fila (anulable, por vínculo con `backup_jobs`); `GET /snapshots` añade `job_name` por instantánea (anulable, por el vínculo). El sufijo `job_` sigue la convención de `target_*` del listado de snapshots.

## 4. Alcance

Backend:
- `migrations/0005_nombre_de_tarea.sql` — `ALTER TABLE backup_jobs ADD COLUMN name TEXT`.
- `docs/DEFINITION.md` §3 — `backup_jobs` gana `name TEXT` (anulable).
- `src/db/entities/job.rs` — `Tarea.name: Option<String>`, `AltaDeTarea.name: Option<String>`, `SQL_LEER`/`SQL_LISTAR`/`SQL_ALTA`/`SQL_ACTUALIZAR` y `desde_fila`.
- `src/api/jobs.rs` — `name` opcional en `AltaDeTareaDelCable`, en `TareaDelCable`, en `alta_desde` y en la reconstrucción de la edición; validación de longitud y blanco.
- `src/api/executions.rs` — `job_name: Option<String>` en el DTO y en la consulta.
- `src/api/snapshots.rs` — `job_name: Option<String>` en el DTO de la instantánea y en la consulta del vínculo.

Frontend:
- `ui/src/services/api/types.ts` — `Job.name`, `AltaJob.name?`, `Execution.job_name`, `Snapshot.job_name`.
- `ui/src/components/ModalDeTarea/ModalDeTarea.tsx` — `Form.Item name="name"` (texto opcional).
- `ui/src/pages/tareas.tsx` — columna «Nombre».
- `ui/src/pages/historico.tsx` — columna «Tarea» y filtro etiquetado por nombre.
- `ui/src/pages/snapshots.tsx` — columna «Tarea».

### Fuera de alcance

- Cambiar la `label` de los snapshots; backfill de nombres en tareas existentes; unicidad o renombrado masivo; cambios en el motor, el planificador o la restauración.

## 5. Contrato SDD (deltas)

- `db/spec.md` — MODIFIED «Las cuatro tablas del §3…»: `backup_jobs.name` como añadido al SRS; escenario de migración.
- `api-resources/spec.md` — MODIFIED «El listado de ejecuciones…» (añade `job_name`); ADDED «La tarea lleva un nombre legible»; MODIFIED «El listado de snapshots declara…» (añade `job_name`).
- `ui/spec.md` — MODIFIED Tareas, Histórico y Snapshots.

## 6. Tareas

Ver `openspec/changes/nombre-de-la-tarea/tasks.md`. Resumen:

1. **RED:** tests de API (migración y columna; `name` en alta/edición/lectura; validaciones de longitud y blanco; `job_name` en ejecuciones y snapshots) y de UI (campo Nombre; columnas en las tres tablas; filtro por nombre).
2. **GREEN:** migración; entidad; DTOs y consultas; formulario y columnas.
3. **REFACTOR:** `just check`, `just ui-check` y `openspec validate --all --strict`.
4. **Cierre:** archivar el change, documentar la feature y abrir PR.

## 7. Criterios de aceptación

- [x] El Histórico y los Snapshots muestran la tarea de forma legible (nombre si lo tiene; si no, el identificador).
- [x] El contrato se actualiza en las specs `db`, `api-resources` y `ui`, y en `docs/DEFINITION.md` §3.
- [x] Tests de API y de UI actualizados y en verde.
- [x] `just check`, `just ui-check` y `openspec validate --all --strict` en verde.

## 8. Referencias

- Issue #65
- `docs/DEFINITION.md` §3, §5
- `src/api/jobs.rs`, `src/api/executions.rs`, `src/api/snapshots.rs`, `src/db/entities/job.rs`
- `ui/src/pages/{tareas,historico,snapshots}.tsx`, `ui/src/components/ModalDeTarea/ModalDeTarea.tsx`
