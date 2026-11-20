# Tasks — keep-previous-data-146

> Change del **panel** (React + TypeScript): los tres listados paginados conservan la página
> anterior mientras llega la siguiente, para no parpadear al paginar. Issue #146. No toca la API,
> las claves de caché ni la política de reintento. TDD: primero los tests que hoy fallan, luego el
> mínimo, luego el refactor. Sin dependencias nuevas (`@tanstack/react-query` ya está).

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `cd ui && pnpm run typecheck && pnpm run lint && pnpm test` (verificación: sin error antes de tocar nada).
- [x] 0.2 Localizar los tres hooks (`ui/src/hooks/useEjecuciones.ts`, `useSnapshots.ts`, `useRestauraciones.ts`) y sus tests (`useEjecuciones.test.tsx`, `useSnapshots.test.tsx`; no hay `useRestauraciones.test.tsx`) (verificación: se citan rutas).

## 1. Conservar la página anterior al paginar

### RED

- [x] 1.1 `useEjecuciones.test.tsx`: caso que cambia de página con la segunda petición **pendiente** y exige que `data` conserve la página anterior, `isPlaceholderData` sea `true` e `isPending` sea `false` (verificación: en rojo hoy; se ejecuta con `cd ui && pnpm test useEjecuciones`).
- [x] 1.2 `useSnapshots.test.tsx`: el mismo caso para Snapshots (verificación: en rojo hoy).
- [x] 1.3 `useRestauraciones.test.tsx` (**nuevo**): el mismo caso para Restauraciones (verificación: en rojo hoy).

### GREEN

- [x] 1.4 `useEjecuciones.ts`, `useSnapshots.ts`, `useRestauraciones.ts`: importar `keepPreviousData` de `@tanstack/react-query` y añadir `placeholderData: keepPreviousData` a cada `useQuery` (verificación: 1.1–1.3 en verde).

### REFACTOR

- [x] 1.5 Rustdoc/JSDoc de los tres hooks: anotar que conservan la página anterior y por qué (verificación: comentario presente; comportamiento intacto).
- [x] 1.6 `cd ui && pnpm run lint && pnpm run typecheck && pnpm test` en verde (verificación: sin errores).

## 2. Verificación

- [x] 2.1 `cd ui && pnpm test` en verde (verificación: 0 fallos).
- [x] 2.2 `cd ui && pnpm run lint && pnpm run typecheck` limpios (verificación: 0 errores).

## 3. Cierre

- [x] 3.1 Marcar las tareas hechas y abrir el PR a `development` con `Closes #146` (verificación: PR creado).
- [x] 3.2 `docs/FEATURE-038-keep-previous-data-146.md` con qué es, ficheros, decisiones y cómo verificar (verificación: el documento existe).
- [x] 3.3 Renombrar `plans/PLAN-038.md` a `plans/PLAN-038-COMPLETED.md` con `git mv` (verificación: renombrado).
- [x] 3.4 Archivar el change: `openspec archive keep-previous-data-146` (verificación: pasa a `openspec/changes/archive/` y el requisito nuevo entra en `openspec/specs/ui/`).
