# Proposal

## Why

Los tres listados del panel —Histórico, Snapshots y Restauraciones— no ofrecen una paginación de verdad: no muestran el total de elementos, ni el número de páginas, ni dejan elegir cuántos elementos ver por página. Histórico y Restauraciones arrastran paginación manual con botones «Anterior/Siguiente» y heurísticas frágiles (el «siguiente» se apaga cuando la página viene incompleta), y Snapshots no pagina en absoluto: corta a una ventana fija de 200 y solo avisa con `truncated`. Se quiere el paginador típico, alimentado por el backend: total, número de páginas y selector de tamaño de página (10 por defecto; 10/20/50), sin paginar nunca en cliente.

## What Changes

- **BREAKING**: `GET /api/v1/executions` deja de devolver un array pelado y pasa a una envoltura `{ "executions": [...], "total": N }`, coherente con el listado de restauraciones. `total` es el número de filas que casan con los filtros, independiente de `limit` y `offset`.
- `GET /api/v1/snapshots` admite `limit` y `offset` (hoy no admite ninguno) y devuelve `total` exacto además de `snapshots`. `truncated` se conserva como aviso si se alcanza un tope alto de seguridad.
- `GET /api/v1/restorations` no cambia de contrato: ya publica `total`.
- La UI de las tres pantallas pasa a usar el paginador de Ant Design con `showTotal`, el número de páginas y el selector de tamaño (10 por defecto; 10/20/50), alimentado por `limit` + `offset` + `total` del backend.

## Capabilities

### New Capabilities

_(ninguna)_

### Modified Capabilities

- `api-resources`: el listado de ejecuciones pasa de array pelado a envoltura con `total`.
- `repository`: el listado de snapshots pasa de ventana acotada sin paginación a paginación real con `limit`/`offset` y `total` exacto.
- `ui`: Histórico, Snapshots y Restauraciones adoptan el paginador con total; se deroga la heurística del Histórico que avanzaba con la página llena y prohibía inventarse un total.
