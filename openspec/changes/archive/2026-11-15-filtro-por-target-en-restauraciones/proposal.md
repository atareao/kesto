# Proposal — filtro-por-target-en-restauraciones

## Why

La pantalla de **Restauraciones** solo permite acotar el histórico por **estado**. Cuando en un despliegue hay varias bases —varios `mariadb`, `postgresql` o `sqlite`—, la pregunta que el operador se hace de verdad es «¿se restauró **esta** base?», y hoy no puede responderla sin leer la columna Target de una lista paginada de todas. El backend ya acepta y prueba el filtro `target_id` y el cliente del panel ya transporta el parámetro: la pieza que falta es, únicamente, **poder elegir el target en la pantalla**. Es el follow-up de #67 que cierra la última interfaz ya soportada por la API.

## What Changes

- La pantalla de Restauraciones SHALL ofrecer un selector de **target**, alimentado por el catálogo de targets, que envíe `target_id` al listado.
- El filtro de target SHALL convivir con el de estado y con la paginación: al cambiar cualquiera de los dos SHALL reiniciarse la página, y el total SHALL reflejar la consulta acotada.
- Limpiar el selector SHALL volver a pedir el listado sin `target_id`.
- **Sin cambios de backend ni de esquema**: `GET /api/v1/restorations` ya acepta `status` y `target_id`, el índice `idx_restore_executions_target_id` ya existe y el comportamiento está cubierto por test.

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `ui`: el requisito «El panel muestra el histórico de restauraciones» pasa de «filtro por estado» a «filtro por estado y por target», con sus escenarios.

## Impact

- **Frontend**: `ui/src/pages/restauraciones.tsx` (selector nuevo), y el consumo del catálogo de targets ya existente (`ui/src/hooks/useTargets.ts`, `ui/src/api/targets.ts`). El transporte del parámetro (`ui/src/api/restauraciones.ts`, `ui/src/hooks/useRestauraciones.ts`) ya lo soporta y no cambia.
- **Backend, esquema y API**: sin cambios.
- **SRS**: sin enmienda: es interfaz de una ruta ya existente.
