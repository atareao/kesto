# Proposal

## Why

El operador no puede saber, desde el panel, si un repositorio S3 está disponible: `Repository` no tiene campo de salud y `/api/v1/repositories` solo expone CRUD, sin endpoint de comprobación. Con varios repositorios, el estado real solo se descubre de refilón al intentar listar instantáneas, y un repositorio caído degrada el resumen del Dashboard. Era la deuda **D12/B6**, diferida en el change *renders-y-resiliencia-del-panel*; aquí se cierra en su forma **completa**.

## What Changes

- Columna de salud en `repositories` (migración `0008`): estado, fecha de comprobación y motivo acotado.
- Endpoint de comprobación que pone en marcha el repositorio y **lista sus instantáneas** (bajo el tope de 10 s ya fijado), persiste el desenlace y lo devuelve: `POST /api/v1/repositories/{id}/health` y `POST /api/v1/repositories/health` (todos).
- El JSON de un repositorio (`GET` listado y `GET /:id`) incorpora `health`, `health_checked_at` y `health_detail` (saneado), derivados de la fila.
- Indicador de salud por repositorio en **Ajustes** (con acción de comprobar, por fila y global) y visibilidad del estado persistido en el **listado del Dashboard**, sin disparar comprobaciones.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `db`: la tabla `repositories` gana columnas de salud y la capa de acceso un método para anotar el desenlace.
- `repository`: nueva comprobación de salud de un repositorio, **de solo lectura** (no lo inicializa), que distingue los desenlaces con el tope ya acordado.
- `api-resources`: endpoint de comprobación y campos de salud en el recurso de repositorio.
- `ui`: indicador de salud por repositorio en Ajustes y en el listado del Dashboard.

## Impact

- Migración nueva `0008_...sql`; `src/db/entities/repository.rs`; `src/rustic/…`; `src/engine/mod.rs`; `src/api/repositories.rs`; `src/api/mod.rs`.
- Frontend: `ui/src/services/api/types.ts`, `ui/src/api/repositorios.ts`, hooks, `ui/src/pages/ajustes.tsx` y el listado del Dashboard.
- Specs: deltas en `db`, `repository`, `api-resources` y `ui`.
- Sin cambios incompatibles: las columnas son anulables y el campo de salud es aditivo en el JSON.
