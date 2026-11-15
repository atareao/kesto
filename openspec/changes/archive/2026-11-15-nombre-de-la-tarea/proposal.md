# Proposal — nombre-de-la-tarea

## Why

Hoy la tarea no tiene nombre: `backup_jobs` guarda target, repositorio, cron y retención, pero ningún texto que la identifique. El Histórico tapa ese hueco con el nombre del **target** como proxy —así que dos tareas sobre el mismo target se leen igual— y los Snapshots muestran la «Etiqueta», que es el `job_id`, un UUID que no dice nada al operador. Las dos pantallas que existen para saber **qué se copió y cuándo** no permiten saber **qué tarea** lo hizo.

## What Changes

- Añadir a `backup_jobs` una columna `name TEXT` **anulable**, por migración `0005_nombre_de_tarea.sql` con un `ALTER TABLE backup_jobs ADD COLUMN name TEXT` —**sin reconstruir** la tabla: se reconstruye solo cuando cambia un `CHECK`— y declararla en el §3 del SRS como añadido.
- El nombre es **opcional y no único**: se recorta (`trim`); en blanco queda a `null`; más de **120** caracteres se rechaza con `400` nombrando `name`.
- Publicar `name` (anulable) en el alta, la edición y los listados de tareas (`GET /api/v1/jobs` y `GET /api/v1/jobs/:id`).
- Publicar **`job_name`** (anulable) en **cada fila** de `GET /api/v1/executions`, resuelto por el vínculo con `backup_jobs`.
- Publicar **`job_name`** (anulable) en **cada instantánea** de `GET /api/v1/snapshots`, por el mismo vínculo que ya resuelve el target: `snapshot → job_executions → backup_jobs → backup_targets`.
- Panel: campo de texto opcional «Nombre» en el formulario de Tareas, columna «Nombre» en la tabla de Tareas, columna «Tarea» en el Histórico y en Snapshots, y filtro de tarea etiquetado con el nombre de la tarea.
- NO SHALL tocarse la `label` de los snapshots: sigue siendo el `job_id`.

## Capabilities

**Modified Capabilities**:

- `db` — `backup_jobs` gana la columna `name` (`TEXT`, anulable), declarada como añadido al SRS.
- `api-resources` — la tarea lleva un nombre legible, y las ejecuciones y los snapshots publican el nombre de su tarea (`job_name`).
- `ui` — Tareas, Histórico y Snapshots muestran y aceptan el nombre de la tarea.

## Fuera de alcance

- Cambiar la `label` de los snapshots: sigue siendo el `job_id`, porque cambiarla afectaría a los snapshots ya escritos y al mapeo de la restauración, que resuelve por `snapshot_id`.
- Rellenar con un nombre las tareas ya existentes (backfill): las que no tengan nombre se quedan a `null` y el panel cae al identificador.
- Exigir unicidad del nombre o permitir renombrarlo por una ruta aparte.
- Cambios en el motor, el planificador o la restauración.

## Impacto

- Backend: `migrations/0005_nombre_de_tarea.sql`, `docs/DEFINITION.md` (§3), `src/db/entities/job.rs`, `src/api/jobs.rs`, `src/api/executions.rs`, `src/api/snapshots.rs`.
- Frontend: los tipos y las pantallas de `ui/src` (Tareas, Histórico, Snapshots y su servicio de API).
- El motor y el planificador NO cambian: el nombre no interviene en la copia ni en el disparo programado.

## Referencias

- Issue #65
- `plans/PLAN-020.md`
- `docs/FEATURE-020-nombre-de-la-tarea.md`
