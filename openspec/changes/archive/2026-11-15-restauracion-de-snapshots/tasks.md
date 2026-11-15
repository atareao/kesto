# Tasks — restauracion-de-snapshots

> Enmiendas tras auditoría (D1–D3) incorporadas: listado ampliado con el target, `404` real si la instantánea no está, y progreso declarado como fase. Fases 0–3 cerradas y verificadas en verde; 4.2 y 4.3 las ejecuta el director al crear el PR.

## 0. Preparación
- [x] 0.1 `just check` y `just ui-check` en verde antes de tocar código.
- [x] 0.2 Fijar el contrato del evento `restore_progress` y el mapeo con un test que falle.
- [x] 0.3 Fijar con tests que fallen el contrato del listado ampliado (D1) y el `404` real antes de responder (D2).

## 1. RED
- [x] 1.1 Test de API: `POST /api/v1/snapshots/{id}/restore` de un target `directory` devuelve `data` con `execution_id` y `target_id`.
- [x] 1.2 Test de API: snapshot sin vínculo devuelve `409`; target de BD devuelve `422`.
- [x] 1.3 Test de UI: el botón «Restaurar» abre confirmación y llama al cliente.
- [x] 1.4 Test de API (D1): el listado trae `target_id`, `target_name` y `target_type` de una instantánea con ejecución, y los tres a `null` para una sin vínculo, sin que la fila desaparezca del listado.
- [x] 1.5 Test de API (D2): `POST /api/v1/snapshots/{id}/restore` con un `id` que no está en el almacenamiento devuelve `404`, no publica `restore_progress` y no lanza ejecución.
- [x] 1.6 Test de UI (D3): el evento `restore_progress` refleja la fase (en curso / terminada / fallida) sin recargar, y sin porcentaje ni barra determinada.
- [x] 1.7 Test de UI (D1): con `target_type` a `null` o distinto de `directory`, la acción de restaurar queda deshabilitada y se explica la limitación.
- [x] 1.8 Test: dos restauraciones concurrentes sobre el mismo destino — la segunda se rechaza.

## 2. GREEN
- [x] 2.1 Implementar el mapeo `snapshot_id -> target` en la capa de datos, tolerante a la ausencia de vínculo.
- [x] 2.2 Implementar `restaurar_snapshot` en `rustic` con `prepare_restore` + `restore`.
- [x] 2.3 Implementar `Motor::restaurar` con emisión de `restore_progress` al hub.
- [x] 2.4 Añadir la ruta y el handler con la envoltura.
- [x] 2.5 Añadir acción y modal en `ui/src/pages/snapshots.tsx`.
- [x] 2.6 Extender el listado con los tres campos nullable (D1).
- [x] 2.7 Comprobar la instantánea contra el almacenamiento antes de responder y contestar `404` si no está (D2).
- [x] 2.8 Publicar la fase terminal fuera del cierre bloqueante (terminada/fallida) (D3).
- [x] 2.9 Guarda contra restauraciones concurrentes sobre el mismo destino.
- [x] 2.10 Migración con índice `snapshot_id` sobre `job_executions` (solo índice, sin cambio de columnas).

## 3. REFACTOR y verificación
- [x] 3.1 `just check` y `just ui-check` verdes.
- [x] 3.2 Auditar con `@rust-reviewer` y `@react-reviewer`.
- [x] 3.3 Documentar en `docs/FEATURE-018-restauracion-de-snapshots.md`.
- [x] 3.4 Comprobar que la delta, el plan, estas tasks y el doc de la feature dicen lo mismo tras D1–D3.

## 4. Cierre
- [x] 4.1 Archivar el change.
- [ ] 4.2 Renombrar `plans/PLAN-018.md` a `plans/PLAN-018-COMPLETED.md`.
- [ ] 4.3 PR a `development` con gitmoji y `Closes #64`.
