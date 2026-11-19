# Design

## Context

Ver `proposal.md — Why`. Puntos de partida verificados:

- `src/api/executions.rs`: `FiltrosDelCable` con `limit`/`offset` (defecto 50, tope 200) y respuesta **array pelado** (`contenido(&vistas)`).
- `src/api/restorations.rs`: ya tiene `limit`/`offset` y `ListadoDelCable { restorations, total }`; es el contrato de referencia.
- `src/api/snapshots.rs`: `ConsultaDelCable` solo con `repository_id`; ventana fija `LA_VENTANA = 200` y `truncated`.
- `ui/src/pages/{historico,snapshots,restauraciones}.tsx`: `Table` de Antd con `pagination={false}`, paginación manual (Histórico, 20 por página) o inexistente (Snapshots).
- El motor de snapshots enumera el listado del repositorio; hoy lo corta en `LA_VENTANA` antes de mapear.

## Goals / Non-Goals

**Goals:**

- Un contrato de paginación único y coherente en los tres listados: `limit` + `offset` de entrada, `total` de salida.
- Que el panel pinte total, número de páginas y selector de tamaño (10 por defecto; 10/20/50) sin heurísticas.
- Mantener el tope de `limit` (200) como guarda contra materializar tablas enteras.

**Non-Goals:**

- No se introduce `page`/`page_size`: el contrato sigue en `limit`/`offset`, que ya usan los tres recursos.
- No se tocan los filtros existentes ni el orden.
- No se cambia el contrato de restauraciones (ya conforme).
- No se añade ruta de detalle de ejecución ni agregados.

## Decisions

**1. Envoltura `{executions, total}` en ejecuciones (BREAKING).**
Alternativas: cabecera `X-Total-Count` (rompe la simetría con restauraciones y con la envoltura JSON del proyecto) y un modo `with_total=true` (añade un estado al contrato). Se elige la envoltura porque restauraciones ya la usa: el coste es adaptar el DTO de ejecuciones, sus tests y el e2e, no un cambio de paradigma.

**2. `total` exacto en ejecuciones = `COUNT(*)` con los mismos filtros, en la misma transacción que la página.** Sin esto el total podría desincronizarse entre dos consultas.

**3. Snapshots: `limit`/`offset` + enumeración completa para el total exacto, con guarda de seguridad alta.**
Alternativas: ventana acotada con total aproximado (el paginador no llegaría a las últimas páginas y mentiría) y total exacto sin guarda (coste sin freno). Se elige enumerar el listado del repositorio y contar, dejando `truncated` como aviso si se toca una guarda de seguridad alta (muy por encima del tope de `limit`), de modo que en el caso normal `truncated` es `false` y el total es exacto.

**4. `limit` acota lo devuelto, no lo enumerado.** La página se corta con `offset`/`limit` sobre la lista enumerada y ordenada; el conteo es sobre el total enumerado.

**5. El backend conserva `50` como `limit` por defecto; el panel envía siempre su tamaño de página.** El defecto del servidor no es el de la UI: el panel pide `10` explícito. Así el contrato del backend no cambia más de lo necesario y el MCP/programas que no pasen `limit` siguen viendo 50.

**6. La UI no pagina en cliente.** El `Table` de Antd recibe `pagination` controlada (`current`, `pageSize`, `total`, `pageSizeOptions: [10, 20, 50]`, `showSizeChanger`, `showTotal`) y en `onChange` se recalcula `offset = (página − 1) × tamaño` y se refetchea. Cambiar filtro o tamaño reinicia a la primera página.

## Risks / Trade-offs

- [Enumerar el repositorio por página puede ser caro con muchísimos snapshots] → la guarda de seguridad + `truncated` acotan el caso patológico; el caso normal (decenas/cientos) es despreciable frente a la latencia del S3.
- [El cambio de contrato de ejecuciones rompe a los consumidores del array pelado (MCP, e2e, tests)] → cambio coordinado backend + frontend + tests en el mismo PR; el MCP consume `total` como campo extra, no lo necesita.
- [Doble consulta (página + count) sobre la tabla de ejecuciones] → ambas con los mismos filtros y el mismo índice `started_at`; el coste ya existía en restauraciones.
- [Cambiar el tamaño de página sin reiniciar `offset` daría páginas vacías] → la UI reinicia a la primera página en todo cambio de filtro/tamaño (exigido por spec).

## Migration Plan

1. Backend (`executions`, `snapshots`) con sus tests primero (TDD).
2. Frontend: tipos, servicios y hooks, luego las tres pantallas con su paginador.
3. Adaptar e2e (`tests/e2e/arnes.rs`) al nuevo sobre de ejecuciones y a `snapshots` paginado.
4. Un solo PR a `development`; sin migración de datos ni de esquema. Rollback = revertir el PR (la API vuelve al array pelado, el panel a los botones).

## Open Questions

_(ninguna)_
