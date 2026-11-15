# Tasks — filtro-por-target-en-restauraciones

> El backend y el transporte del parámetro ya están hechos: `GET /api/v1/restorations` acepta `status` y `target_id` (con test `el_filtro_por_target_acota_el_listado` en `src/api/restorations.rs`) y `ui/src/api/restauraciones.ts` ya envía `target_id`. Todo el trabajo es de interfaz: el selector que falta en `ui/src/pages/restauraciones.tsx`.

## 1. Preparación

- [x] 1.1 `just check-all` en verde antes de tocar código; verificar que termina con EXIT=0.
- [x] 1.2 Confirmar el punto de partida: `rg -n 'target_id' ui/src/api/restauraciones.ts ui/src/hooks/useRestauraciones.ts` y verificar que el parámetro ya viaja en la consulta; `rg -n 'useTargets' ui/src -l` y verificar el catálogo disponible.

## 2. RED — el comportamiento que falta, fijado con tests que fallan

- [x] 2.1 Test UI en `ui/src/pages/restauraciones.test.tsx`: la pantalla ofrece un selector de target con las opciones del catálogo; al elegir uno **SHALL** pedirse `GET /api/v1/restorations` con su `target_id` y la tabla **SHALL** mostrar solo esas filas. Verificación: `pnpm test` en `ui/` → el caso nuevo falla y los existentes siguen en verde.
- [x] 2.2 Test UI: cambiar el filtro (target o estado) reinicia la paginación a la primera página y el `total` refleja el resultado acotado. Verificación: `pnpm test` en `ui/` → falla por la ausencia del selector y de su reinicio.
- [x] 2.3 Test UI: limpiar el selector **SHALL** pedir el listado sin `target_id`, y elegir target y estado a la vez **SHALL** enviar ambos parámetros. Verificación: `pnpm test` en `ui/` → los dos casos fallan.
- [x] 2.4 Test UI: un `target_id` cuyo target ya no está en el catálogo no rompe la pantalla —se ve el vacío, no un error ni una carga infinita—. Verificación: `pnpm test` en `ui/` → falla.
- [x] 2.5 Registrar el estado RED: `pnpm test` en `ui/` con los casos nuevos fallando y el resto en verde; `npx tsc --noEmit` sin errores de tipos en el test.

## 3. GREEN — el selector, mínimo y suficiente

- [x] 3.1 Añadir a `ui/src/pages/restauraciones.tsx` el estado del filtro de target y el `Select` con `allowClear`, con las opciones de `useTargets()` (etiqueta = nombre del target, valor = su `id`), integrado con el filtro de estado existente. Verificación: `pnpm test` en `ui/` → 2.1 y 2.3 (combinación y limpieza) pasan.
- [x] 3.2 Pasar el `target_id` elegido a `useRestauraciones` y reiniciar `offset` al cambiar el filtro, por el mismo camino que ya usa el filtro de estado. Verificación: `pnpm test` en `ui/` → 2.2 pasa.
- [x] 3.3 Mantener el comportamiento ante un target ausente del catálogo (D4 del `design.md`). Verificación: `pnpm test` en `ui/` → 2.4 pasa.
- [x] 3.4 Comprobar el efecto en la `queryKey` y el refresco periódico: `useRestauraciones` ya deriva la clave de los filtros, así que cambiar el target **SHALL** traer consulta nueva y no la cacheada. Verificación: `pnpm test` en `ui/` → el caso de refresco del histórico sigue en verde y no se pide la clave anterior.

## 4. REFACTOR y cierre del panel

- [x] 4.1 Dejar el componente sin duplicar el patrón del Histórico: si el selector de filtro y su reinicio pueden compartir la forma ya usada en `ui/src/pages/historico.tsx`, aplicarlo; si no, justificar en el PR por qué difiere. Verificación: revisión del diff y `pnpm run lint` sin avisos.
- [x] 4.2 Verificación del panel completa: `pnpm run lint` → `npx tsc --noEmit` → `pnpm test` → `pnpm run build`, cada uno en verde.
- [x] 4.3 Auditoría `@react-reviewer` sobre el diff, sin bloqueantes.
- [x] 4.4 Documentar la entrega: `docs/FEATURE-022-filtro-por-target-en-restauraciones.md` (qué es, ficheros, cómo funciona, cómo probarlo, decisiones D1–D4 y follow-ups).

## 5. Verificación final (integración)

- [x] 5.1 `just check-all` con EXIT=0 (backend `fmt`+`clippy -D warnings`+`test` y panel `lint`+`typecheck`+`typecheck:test`+`test`+`build`).
- [x] 5.2 Escenarios de `specs/ui/spec.md` recorridos uno a uno contra los tests que los fijan; ninguno sin cobertura.
- [x] 5.3 `openspec validate filtro-por-target-en-restauraciones --strict` sin hallazgos.
- [x] 5.4 Marcar las casillas del plan, renombrar `plans/PLAN-022.md` a `plans/PLAN-022-COMPLETED.md` y archivar el change: `openspec archive filtro-por-target-en-restauraciones`.
