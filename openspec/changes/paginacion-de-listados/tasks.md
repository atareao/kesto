# Tasks

## 1. Backend: ejecuciones con envoltura y total

- [x] 1.1 Escribir en `src/api/executions.rs` los tests que exigen la envoltura: el listado sin filtros trae `executions` y `total`; `total` refleja el acotado por `status`/`job_id`; `total` no cambia con `limit`/`offset`. Verificado con `cargo test ejecuciones` (deben fallar: RED).
- [x] 1.2 Cambiar el DTO de respuesta de `src/api/executions.rs` a `{executions, total}`, calculando `total` con un conteo bajo los mismos filtros que la página. Verificado con `cargo test ejecuciones` (GREEN).
- [x] 1.3 Adaptar el resto de tests del fichero y el consumo del listado en `tests/e2e/arnes.rs`. Verificado con `cargo test` y `just check`.

## 2. Backend: snapshots con limit, offset y total exacto

- [x] 2.1 Tests en `src/api/snapshots.rs`: la ruta acepta `limit`/`offset`, `total` es exacto, `truncated` es `false` en el caso normal y `400` con `limit` fuera de rango. Verificado con `cargo test --bin kesto 'api::snapshots::tests::'` (RED).
- [x] 2.2 Añadir `limit`/`offset` a `ConsultaDelCable` y hacer que la ruta enumere el listado del repositorio, cuente el total exacto y sirva la página, conservando `truncated` como guarda de seguridad. Verificado con `cargo test --bin kesto 'api::snapshots::tests::'` (GREEN).
- [x] 2.3 Llevar `total` en el DTO `ListadoDelCable` y adaptar los tests de la ventana. Verificado con `cargo test` y `just check`.

## 3. Frontend: contrato, servicios y hooks

- [x] 3.1 Actualizar `ui/src/services/api/types.ts` y `ui/src/api/{ejecuciones,snapshots}.ts` al contrato nuevo (`{executions, total}` y `{snapshots, total, truncated}` con `limit`/`offset`). Verificado con `pnpm run typecheck`.
- [x] 3.2 Actualizar los hooks `ui/src/hooks/{useEjecuciones,useSnapshots}.ts` para aceptar y propagar `limit`/`offset` y devolver el `total`. Verificado con sus tests en `just ui-check`.

## 4. Frontend: paginador en las tres pantallas

- [x] 4.1 `ui/src/pages/historico.tsx`: sustituir los botones por el paginador de Antd (`total`, `showTotal`, `pageSizeOptions: [10,20,50]`, `10` por defecto) y recalcular `offset`. Verificado con los tests de `historico.test.tsx` en `just ui-check`.
- [x] 4.2 `ui/src/pages/snapshots.tsx`: añadir el paginador con el mismo contrato y pedir la página al servidor. Verificado con los tests de `snapshots.test.tsx`.
- [x] 4.3 `ui/src/pages/restauraciones.tsx`: sustituir los botones por el paginador alimentado por `total`. Verificado con los tests de `restauraciones.test.tsx`.

## 5. Integración y documentación

- [ ] 5.1 Repasar el e2e de backend y pasar `just check-all` (fmt, clippy, test, scripts-test, ui-check). Verificado en verde.
- [ ] 5.2 Escribir `docs/FEATURE-033-paginacion-de-listados.md` y cerrar `plans/PLAN-033.md`. Verificado por su presencia y contenido.
