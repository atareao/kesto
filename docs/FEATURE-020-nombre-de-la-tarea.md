# FEATURE-020 — Nombre de la tarea en el histórico y en los snapshots

- **Issue:** [#65](https://github.com/atareao/kesto/issues/65).
- **Plan:** `plans/PLAN-020.md` (renombrado a `plans/PLAN-020-COMPLETED.md` al cerrar).
- **Change SDD:** `openspec/changes/archive/2026-11-15-nombre-de-la-tarea/` (archivado).

## Qué es

Da a cada tarea un **nombre legible** y lo muestra donde el operador lo necesita: el Histórico y los Snapshots. Antes `backup_jobs` no tenía nombre, así que el Histórico etiquetaba las tareas con el nombre del **target** —proxy que confunde dos tareas sobre el mismo target— y los Snapshots solo ofrecían la «Etiqueta», que es el `job_id` (un UUID). Ahora la tarea tiene `name` (opcional) y las ejecuciones y las instantáneas publican `job_name`.

## Ficheros implicados

- **Backend**
  - `migrations/0005_nombre_de_tarea.sql` — `ALTER TABLE backup_jobs ADD COLUMN name TEXT`, sin reconstruir la tabla.
  - `docs/DEFINITION.md` §3 — `backup_jobs` gana `name TEXT` (anulable), como añadido al SRS.
  - `src/db/entities/job.rs` — `Tarea.name`/`AltaDeTarea.name: Option<String>`, el `name` en `SQL_LEER`/`SQL_LISTAR`/`SQL_ALTA`/`SQL_ACTUALIZAR`, y `Tareas::nombres(&[Uuid])` (lectura por lotes con `json_each`).
  - `src/api/jobs.rs` — `name` en `AltaDeTareaDelCable` (opcional) y `TareaDelCable`; el validador `nombre_del_cuerpo` (recorte, blanco → `null`, tope de 120 caracteres).
  - `src/api/executions.rs` — `job_name` en el DTO y en el listado.
  - `src/api/snapshots.rs` — `job_name` en cada instantánea.
- **Frontend**
  - `ui/src/services/api/types.ts` — `Job.name`, `AltaJob.name?`, `Execution.job_name`, `Snapshot.job_name`.
  - `ui/src/components/ModalDeTarea/ModalDeTarea.tsx` — campo opcional «Nombre» y `cuerpoDeTarea()` que omite la clave en blanco.
  - `ui/src/pages/tareas.tsx` — columna «Nombre».
  - `ui/src/pages/historico.tsx` — columna «Tarea» y filtro etiquetado con el nombre de la tarea.
  - `ui/src/pages/snapshots.tsx` — columna «Tarea» entre «Fecha» y «Etiqueta».
  - `ui/tests/fixtures.ts` — las factories ganan `name`/`job_name` a `null`.

## Cómo funciona

- **El nombre es opcional y no único.** `POST`/`PUT /api/v1/jobs` aceptan `name`; se recorta; si queda en blanco se guarda `null`; si supera **120 caracteres** se contesta `400` nombrando `name`. El tope se mide en **caracteres**, no en bytes. `GET /jobs` publica `name` (la clave va siempre, con `null` si no lo tiene).
- **Histórico y Snapshots publican `job_name`** (anulable). La resolución del nombre es una **lectura por lotes** sobre `backup_jobs` (`Tareas::nombres`, con los identificadores como array JSON enlazado a `json_each`), no un `JOIN` en la consulta paginada: así no se duplican el `WHERE`, el `ORDER BY` ni el `LIMIT`. En los snapshots, el nombre sale del mismo vínculo que ya resuelve el target (`snapshot → job_executions → backup_jobs`).
- **La `label` de los snapshots no se toca**: sigue siendo el `job_id`. El panel pinta la columna «Tarea» con `job_name` y cae al identificador (`job_id` en el Histórico, `label` en los Snapshots) cuando no hay nombre.
- **En el panel**, el formulario de Tareas tiene un campo «Nombre» opcional (`maxLength={120}`, coherente con el tope del backend) que se omite del cuerpo cuando queda en blanco, y las tres tablas muestran el nombre.

## Variables / configuración

Ninguna. No hay flags ni variables de entorno.

## Cómo habilitar / desplegar

Sin pasos: la migración `0005` se aplica al arrancar (embebida) y el cambio va en el bundle del panel. `just check-all` y `openspec validate --all --strict` deben estar en verde.

## Decisiones

- **Columna `name` anulable por `ALTER TABLE ADD COLUMN`**, sin reconstruir: la tabla solo se reconstruye cuando cambia un `CHECK` (el caso de la `0003`). Un test desde cero no distingue las dos formas, así que `src/db/migrate.rs` monta la base hasta la `0004` **con tareas dentro** y exige que sobrevivan a la `0005`.
- **El nombre se expone como `job_name` en los listados**, siguiendo la convención de `target_*` del listado de snapshots.
- **No se toca la `label`** de los snapshots: cambiarla afectaría a las instantáneas ya escritas y al mapeo de la restauración (que resuelve por `snapshot_id`).
- **Sin backfill**: las tareas existentes se quedan con `name = null` y el panel cae al identificador.

## Follow-ups

- **Nombre de la columna «Tarea» en la pantalla de Tareas**: esa columna muestra en realidad el nombre del **target**; renombrarla a «Target» aclararía la tabla (fuera del alcance de este change).
- **Fallback de la columna «Tarea» en Snapshots**: cuando `job_name` es `null` y la `label` no procede de una tarea de Kesto (instantánea de otra herramienta), se muestra esa etiqueta. Es el único dato disponible y el contrato lo permite.
- **Acoplamiento del tope de 120**: el panel lo fija en `maxLength={120}` como literal propio; ningún test ata los dos valores. Un deslizamiento pasaría desapercibido.
