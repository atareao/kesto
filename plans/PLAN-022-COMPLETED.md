---
# PLAN-022 — Filtro por target en la pantalla de Restauraciones

- **Issue**: #79 (`atareao/kesto`)
- **Change SDD**: `openspec/changes/filtro-por-target-en-restauraciones/` (schema `spec-driven`; `openspec validate --strict` → válido)
- **Rama**: `feature/filtro-target-en-restauraciones` (desde `development`)
- **Cierre**: `plans/PLAN-022-COMPLETED.md` + `docs/FEATURE-022-filtro-por-target-en-restauraciones.md`

## Objetivo

Añadir un selector de **target** a la pantalla de Restauraciones para acotar el histórico por base de datos. La pantalla solo filtra hoy por estado; el backend ya acepta y prueba `target_id` en `GET /api/v1/restoraciones` y el cliente del panel ya envía el parámetro. La pieza que falta es la interfaz.

## Alcance

- **Dentro**: selector de target con `allowClear` en `ui/src/pages/restauraciones.tsx`, alimentado por `useTargets()` (`GET /api/v1/targets`); envío de `target_id`; reinicio de la paginación al cambiar cualquier filtro; combinación con el filtro de estado; limpiar el selector vuelve al listado completo.
- **Fuera**: backend, esquema y contrato de la API (ya cumplen); filtro por rango de fechas o por snapshot; reconciliación de opciones cuando un target borrado desaparece del catálogo (decisión D4 del `design.md`); el resto de follow-ups de #67 (restauración parcial por rutas, copia previa automática, progreso incremental, e2e de los `#[ignore]` en CI).

## Capacidades SDD afectadas

- `ui` (modificada): el requisito «El panel muestra el histórico de restauraciones» pasa de «filtro por estado» a «filtro por estado y por target», con escenarios nuevos (acotar por target, reiniciar paginación, limpiar filtro, combinar filtros).
- `api-resources`: **sin cambios**. El filtro `target_id` ya está declarado y cubierto.

## Ficheros implicados

| Fichero | Cambio |
|---|---|
| `ui/src/pages/restauraciones.tsx` | Selector de target y reinicio de la paginación |
| `ui/src/pages/restauraciones.test.tsx` | Casos RED del filtro por target |
| `ui/src/hooks/useTargets.ts`, `ui/src/api/targets.ts` | Consumo del catálogo (ya existen) |
| `ui/src/api/restauraciones.ts`, `ui/src/hooks/useRestauraciones.ts` | Sin cambios: ya transportan `target_id` |
| `ui/src/pages/historico.tsx` | Referencia del patrón de filtro por catálogo (no se modifica) |

## Tareas (TDD: RED → GREEN → REFACTOR)

### 1. Preparación
- [x] 1.1 `just check-all` en verde (EXIT=0) antes de tocar código.
- [x] 1.2 Confirmar el punto de partida: el parámetro `target_id` ya viaja en la consulta y el catálogo de targets ya es consumible.

### 2. RED — tests que fallan
- [x] 2.1 Test UI: el selector de target existe, sus opciones salen del catálogo, y elegir una acota la petición y la tabla. Verificación: `pnpm test` en `ui/` → el caso nuevo falla y el resto sigue verde.
- [x] 2.2 Test UI: cambiar el filtro reinicia la paginación y el total refleja el resultado acotado. Verificación: `pn. test` en `ui/` → falla.
- [x] 2.3 Test UI: limpiar el selector pide el listado sin `target_id`; elegir target y estado envía ambos parámetros. Verificación: `pnpm test` en `ui/` → falla.
- [x] 2.4 Test UI: un `target_id` sin opción en el catálogo no rompe la pantalla (vacío, no error ni carga infinita). Verificación: `pnpm test` en `ui/` → falla.
- [x] 2.5 Registrar el estado RED: casos nuevos fallando, resto en verde, `npx tsc --noEmit` sin errores de tipos en el test.

### 3. GREEN — el selector, mínimo
- [x] 3.1 Añadir el estado y el `Select` de target con `allowClear` en `ui/src/pages/restauraciones.tsx` (etiqueta = nombre del target, valor = su `id`), junto al filtro de estado. Verificación: `pnpm test` en `ui/` → 2.1 y 2.3 pasan.
- [x] 3.2 Pasar `target_id` a `useRestauraciones` y reiniciar `offset` al cambiar el filtro, por el mismo camino que el estado. Verificación: `pnpm test` en `ui/` → 2.2 pasa.
- [x] 3.3 Sostener el comportamiento ante un target ausente del catálogo (D4). Verificación: `pnpm test` en `ui/` → 2.4 pasa.
- [x] 3.4 Comprobar que la `queryKey` cambia con el filtro y que el refresco periódico no sirve la consulta cacheada anterior. Verificación: `pnpm test` en `ui/` → el caso de refresco del histórico sigue verde.

### 4. REFACTOR y cierre del panel
- [x] 4.1 Evitar duplicar el patrón del Histórico o justificar en el PR por qué difiere. Verificación: revisión del diff y `pnpm run lint` sin avisos.
- [x] 4.2 Panel completo en verde: `pnpm run lint` → `npx tsc --noEmit` → `pnpm test` → `pnpm run build`.
- [x] 4.3 Auditoría `@react-reviewer` sobre el diff, sin bloqueantes.
- [x] 4.4 `docs/FEATURE-022-filtro-por-target-en-restauraciones.md` (qué es, ficheros, cómo funciona, cómo probarlo, decisiones D1–D4, follow-ups).

### 5. Verificación final (integración)
- [x] 5.1 `just check-all` con EXIT=0 (backend `fmt`+`clippy -D warnings`+`test`; panel `lint`+`typecheck`+`typecheck:test`+`test`+`build`).
- [x] 5.2 Recorrer uno a uno los escenarios de `openspec/changes/filtro-por-target-en-restauraciones/specs/ui/spec.md` y comprobar que cada uno tiene test que lo fija.
- [x] 5.3 `openspec validate filtro-por-target-en-restauraciones --strict` sin hallazgos.
- [x] 5.4 Marcar casillas, renombrar a `plans/PLAN-022-COMPLETED.md` y `openspec archive filtro-por-target-en-restauraciones`.

## Decisiones de diseño

- **D1** — Las opciones del selector salen del catálogo completo (`GET /api/v1/targets`), no de los targets de la página actual: derivarlas de la página haría desaparecer un target sin restauraciones en esa página y cambiaría las opciones al paginar.
- **D2** — El selector muestra el nombre del target y envía su `id`, como el filtro de tarea del Histórico: etiqueta legible, valor identificador.
- **D3** — Reiniciar `offset` al cambiar de filtro: sin ello, un target con menos filas que la página en curso devolvería una página vacía sobre un listado que sí tiene filas.
- **D4** — Si el `target_id` pedido ya no existe, no se reconcilian opciones: se pinta lo que devuelva la API o su vacío.

## Riesgos

- Restaurar una base **pisa datos vivos** (decisión D de #67); este plan no lo mitiga, solo la interfaz del histórico.
- La selección de una opción que desaparece del catálogo queda «sin valor visible»; el operador puede limpiar el filtro (D4).
- Una petición extra de catálogo al abrir la pantalla, ya cacheada y compartida con Ajustes e Histórico.

## Criterios de aceptación

- La pantalla ofrece un selector de target; al elegirlo se pide `GET /api/v1/restorations?target_id=<uuid>` y la tabla se acota con su total.
- El filtro convive con el de estado y reinicia la paginación.
- Limpiar el selector vuelve al listado completo.
- `just check-all` EXIT=0, auditoría `@react-reviewer` sin bloqueantes, change SDD archivado y plan renombrado a `-COMPLETED`.
---