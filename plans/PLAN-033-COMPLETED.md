# PLAN-033 — Paginación con total en Histórico, Snapshots y Restauraciones

- **Issue**: [#140](https://github.com/atareao/kesto/issues/140) — «✨ Paginador con total en Histórico, Snapshots y Restauraciones».
- **Change de OpenSpec**: `openspec/changes/paginacion-de-listados/` (APROBADO por el usuario; deltas en `specs/{api-resources,repository,ui}/spec.md`).
- **Rama**: `feature/paginacion-de-listados` (base `development`, commit previo `a3230ed` solo con los artefactos del change).
- **Protocolo**: SDD ya cerrado → TDD (RED → GREEN → REFACTOR) con `just check` como puerta local.

## Objetivo

Que los tres listados paginen **en servidor**, con **total de elementos** ajeno a la página, y que el panel use el paginador de Antd con selector 10/20/50 (**10 por defecto**). Se mantiene `limit`/`offset` (sin `page`/`page_size`); el backend conserva su `limit` por defecto de 50 y el panel manda `10` explícito.

## Alcance

- **BREAKING** — `GET /api/v1/executions` pasa de array pelado a `{executions, total}` (`total` = filas que casan con los filtros, ajeno a `limit`/`offset`). Afecta a `src/api/executions.rs` y al e2e `tests/e2e/arnes.rs`.
- `GET /api/v1/snapshots` gana `limit`/`offset` (tope 200) y `total` exacto enumerando el listado del repositorio; `truncated` queda como guarda de seguridad. Afecta a `src/api/snapshots.rs` y al motor.
- **UI**: `ui/src/pages/{historico,snapshots,restauraciones}.tsx` usan el paginador de Antd (total, nº de páginas, 10/20/50) y piden la página al servidor (`limit` + `offset`); nunca paginan en cliente.
- Fuera de alcance: cambiar la forma de `limit`/`offset` por `page`/`page_size`; paginación en cliente; code-splitting y WebP/AVIF (follow-ups aparte).

## Tareas (checklist del change, `openspec/changes/paginacion-de-listados/tasks.md`)

### 1. Backend: ejecuciones con envoltura y total
- [ ] 1.1 Tests en `src/api/executions.rs` exigiendo la envoltura: listado sin filtros trae `executions` y `total`; `total` refleja el acotado por `status`/`job_id`; `total` no cambia con `limit`/`offset` (`cargo test ejecuciones`, RED).
- [ ] 1.2 Cambiar el DTO de respuesta a `{executions, total}`, calculando `total` con un conteo bajo los mismos filtros que la página (`cargo test ejecuciones`, GREEN).
- [ ] 1.3 Adaptar el resto de tests del fichero y el consumo del listado en `tests/e2e/arnes.rs` (`cargo test` y `just check`).

### 2. Backend: snapshots con limit, offset y total exacto
- [ ] 2.1 Tests en `src/api/snapshots.rs`: la ruta acepta `limit`/`offset`, `total` exacto, `truncated: false` en el caso normal y `400` con `limit` fuera de rango (`cargo test instantaneas`, RED).
- [ ] 2.2 Añadir `limit`/`offset` a `ConsultaDelCable`, enumerar el listado del repositorio, contar el total exacto y servir la página, conservando `truncated` como guarda (`cargo test instantaneas`, GREEN).
- [ ] 2.3 Llevar `total` en el DTO `ListadoDelCable` y adaptar los tests de la ventana (`cargo test` y `just check`).

### 3. Frontend: contrato, servicios y hooks
- [ ] 3.1 Actualizar `ui/src/services/api/types.ts` y `ui/src/api/{ejecuciones,snapshots}.ts` al contrato nuevo (`{executions, total}` y `{snapshots, total, truncated}` con `limit`/`offset`) (`pnpm run typecheck`).
- [ ] 3.2 Actualizar los hooks `ui/src/hooks/{useEjecuciones,useSnapshots}.ts` para aceptar y propagar `limit`/`offset` y devolver el `total` (tests en `just ui-check`).

### 4. Frontend: paginador en las tres pantallas
- [ ] 4.1 `ui/src/pages/historico.tsx`: sustituir los botones por el paginador de Antd (`total`, `showTotal`, `pageSizeOptions: [10,20,50]`, `10` por defecto) y recalcular `offset` (`just ui-check`).
- [ ] 4.2 `ui/src/pages/snapshots.tsx`: añadir el paginador con el mismo contrato y pedir la página al servidor.
- [ ] 4.3 `ui/src/pages/restauraciones.tsx`: sustituir los botones por el paginador alimentado por `total`.

### 5. Integración y documentación
- [ ] 5.1 Repasar el e2e de backend y pasar `just check-all` (fmt, clippy, test, scripts-test, ui-check) en verde.
- [ ] 5.2 Escribir `docs/FEATURE-033-paginacion-de-listados.md` y cerrar `plans/PLAN-033.md`.

## Puertas de calidad

- Por cada grupo: `just check` verde antes de pasar al siguiente.
- Cierre: `just check-all` verde, `openspec archive paginacion-de-listados`, `docs/FEATURE-033-paginacion-de-listados.md`.
- Gitflow: commits gitmoji en `feature/paginacion-de-listados`; PR a `development` al cerrar.

## Cierre

Al terminar todo: marcar tareas, archivar el change de OpenSpec, escribir la doc de la feature, renombrar este plan a `plans/PLAN-033-COMPLETED.md` con `git mv` y abrir PR a `development`.
