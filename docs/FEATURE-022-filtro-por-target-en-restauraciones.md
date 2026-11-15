# FEATURE-022 — Filtro por target en la pantalla de Restauraciones

- **Issue**: #79 · **Plan**: `plans/PLAN-022.md` · **Change SDD**: `openspec/changes/filtro-por-target-en-restauraciones/` (archivado el 2026-11-15)
- **Rama**: `feature/filtro-target-en-restauraciones`

## Qué es

Follow-up de #67 (fase 2 de restauración). La pantalla **Restauraciones** solo permitía acotar el histórico por **estado**. Con varias bases —varios `mariadb`, `postgresql` o `sqlite`— la pregunta real del operador es «¿se restauró **esta** base?», y no había forma de responderla sin leer la columna Target de una lista paginada de todas.

Esta entrega añade un **selector de target** que acota el listado por base de datos. **No toca el backend**: `GET /api/v1/restorations` ya aceptaba y probaba el filtro `target_id`, el índice `idx_restore_executions_target_id` ya existía y el cliente del panel ya transportaba el parámetro. Lo que faltaba era, únicamente, poder elegirlo en la pantalla.

## Ficheros implicados

| Fichero | Cambio |
|---|---|
| `ui/src/pages/restauraciones.tsx` | Estado `targetId`, `useTargets()` como catálogo, `Select` con `aria-label="Target"` y `allowClear`, `target_id` en los filtros, `cambiarTarget` que reinicia `offset` |
| `ui/src/pages/restauraciones.test.tsx` | 6 casos nuevos (bloque `describe('filtro por target en Restauraciones')`) + helpers `elegirTarget`, `limpiarTarget`, `targetIdPedido` y el catálogo servido por `manejadorCon()` |
| `ui/src/hooks/useTargets.ts`, `ui/src/api/targets.ts` | Consumo del catálogo `GET /api/v1/targets` (ya existían, sin cambios) |
| `ui/src/api/restauraciones.ts`, `ui/src/hooks/useRestauraciones.ts` | Sin cambios: ya declaraban y enviaban `target_id` |
| `ui/src/pages/historico.tsx` | Referencia del patrón de filtro por catálogo (no se modifica) |

Nada de `src/` (Rust), nada de `migrations/`.

## Cómo funciona

1. La pantalla pide el catálogo con `useTargets()` y monta las opciones del selector como `{ value: target.id, label: target.name }`: el operador ve el **nombre** y la API recibe el **identificador**.
2. El `target_id` elegido vive en **estado propio** (`useState`) y no se deriva del catálogo. Es deliberado: si el target se borra en otra pestaña, la pantalla conserva el filtro y pinta su vacío en lugar de reconciliar opciones y romperse.
3. Los filtros (`status`, `target_id`, `limit`, `offset`) se pasan completos a `useRestauraciones`, cuya `queryKey` ya se deriva de ellos: cambiar de target es consulta nueva, no caché.
4. Cambiar cualquier filtro reinicia `offset` a 0, por el mismo camino en los dos casos. Sin ese reinicio, un target con menos restauraciones que la página en curso devolvería una página vacía sobre un listado que sí tiene filas.
5. Limpiar el selector (`allowClear`) manda `target_id` como `undefined`, así que la petición vuelve a ser la del listado completo.
6. El selector de target y el de estado son independientes y se combinan: elegir los dos envía ambos parámetros.

## Contrato de API

Sin cambios. Se sigue usando `GET /api/v1/restorations` con los filtros `status`, `target_id`, `limit` y `offset` (`src/api/restorations.rs`), y `GET /api/v1/targets` para el catálogo.

## Variables y configuración

Ninguna nueva.

## Cómo habilitar/desplegar

No hay nada que habilitar: es interfaz de panel. Al desplegar la nueva imagen del panel, el selector aparece en la pantalla de Restauraciones.

## Decisiones

- **D1 — Opciones del catálogo completo, no de la página.** Derivarlas de las filas ya listadas haría que un target sin restauraciones en la página en curso desapareciera del selector, y que sus opciones cambiaran al paginar. El coste es una petición de catálogo ya cacheada y compartida con Ajustes e Histórico. Se descartó un `Select` de texto libre: obligaría a conocer un UUID.
- **D2 — Etiqueta = nombre, valor = id.** Mismo criterio que el filtro por tarea del Histórico.
- **D3 — Reiniciar `offset` al cambiar de filtro.** Evita la página vacía sobre un resultado que sí tiene filas.
- **D4 — Un target ausente del catálogo no rompe la pantalla.** No se reconcilian las opciones del selector; se pinta lo que devuelva la API o su estado vacío, y el operador puede limpiar el filtro.
- **D5 — No se extrae el patrón de filtro a un componente compartido.** El patrón (`Select` + reinicio de `offset`) ya se repite en Histórico y aquí son pocas líneas; extraerlo ahora divergiría de las dos pantallas existentes sin reducir complejidad real. Se rehará si aparece una tercera pantalla con el mismo filtro.

## Verificación

- **RED**: `6 failed | 217 passed` (`223`), solo fallando `ui/src/pages/restauraciones.test.tsx` por ausencia del selector.
- **GREEN**: `pnpm test` → `30 ficheros / 223 tests passed`; el fichero objetivo, `16 passed (16)`. `npx tsc --noEmit` → 0. `pnpm run lint` → sin avisos.
- **Auditoría** `@react-reviewer` sobre el diff: **sin bloqueantes**. Un hallazgo *recomendado* (el caso de reinicio de paginación por estado salía de la página 1 antes de filtrar) quedó cerrado en `1fbb6a5`, verificado por mutación: con `cambiarEstado` privado de su `setOffset(0)`, el caso falla.
- **Integración**: `just check-all` con EXIT=0 (backend `fmt`+`clippy -D warnings`+`test`; panel `lint`+`typecheck`+`typecheck:test`+`test`+`build`).
- **SDD**: `openspec validate filtro-por-target-en-restauraciones --strict` sin hallazgos; `openspec validate --specs --strict` → `13 passed, 0 failed`; change archivado en `openspec/changes/archive/2026-11-15-filtro-por-target-en-restauraciones/` y requisito fusionado en `openspec/specs/ui/spec.md:545`.

## Follow-ups

- El resto de follow-ups de #67 siguen abiertos: **restauración parcial por rutas**, **copia previa automática**, **progreso incremental real** (bloqueado por `rustic_core`) y **e2e de los `#[ignore]` en CI**.
- Si aparece una tercera pantalla con filtro por catálogo, reconsiderar **D5**.
- La auditoría `@react-reviewer` señala duplicación **más allá del filtro**: el pie de paginación, `TAMANO_DE_PAGINA` y `ESTADOS` se repiten entre `historico.tsx` y `restauraciones.tsx`. Extraerlos merece **ticket aparte** cuando entre una tercera pantalla paginada; mezclarlo aquí ampliaría el diff de una feature con un refactor.
- Conocido y transversal (no de esta entrega): `allowClear` de Ant Design es **solo ratón**, así que quitar el filtro por teclado no es posible; un botón visible «Limpiar filtros» lo resolvería en las pantallas afectadas.
