# Proposal — restauracion-de-snapshots

## Why

Hoy el panel solo lista snapshots; no hay forma de devolver sus datos. El operador necesita poder restaurar un snapshot completo sobre el target del que procede, empezando por los targets de directorio.

## What Changes

- Nuevo endpoint `POST /api/v1/snapshots/{id}/restore` que mapea `snapshot -> ejecución -> target` y restaura sobre el target de origen.
- Evento SSE `restore_progress` en `/api/v1/events`.
- Acción de restaurar en la pantalla de snapshots, con confirmación y progreso.
- v1 solo targets `directory` (a `host_path`, sin borrado destructivo). Los targets de BD se difieren a la fase 2 (issue #67).

## Fuera de alcance

- Restauración de targets de BD (`mariadb`, `postgresql`, `sqlite`).
- Restauración parcial por rutas y borrado/purga de snapshots.

## Impacto

- Backend: `engine`, `rustic` y `api-resources` (nuevo endpoint y evento).
- Frontend: `ui` (pantalla de snapshots).
- Sin cambios de esquema: el mapeo usa `job_executions`, `backup_jobs` y `backup_targets` existentes.

## Referencias

- Issue #64 (v1), #67 (fase 2 de BD)
- `plans/PLAN-018.md`
- `docs/DEFINITION.md` §3-§5
