# PLAN-038 — Conservar la página anterior al paginar (`keepPreviousData` en el panel) (#146)

- **Issue:** #146 · **Change OpenSpec:** `keep-previous-data-146` · **Rama:** `feature/keep-previous-data-146` (desde `development`).
- **Tipo:** panel (React + TypeScript). **No** toca la API, las claves de caché ni la política de reintento.

## 1. Objetivo

Que al cambiar de página en Histórico, Snapshots y Restauraciones la tabla no parpadee. Hoy React Query trata cada página como una consulta distinta (el `offset` forma parte de la clave): al cambiar de página la consulta nueva nace en `pending`, la pantalla pinta «cargando» y la tabla se desmonta hasta que llega la respuesta.

## 2. Contexto

- Los tres hooks —`ui/src/hooks/useEjecuciones.ts`, `useSnapshots.ts` y `useRestauraciones.ts`— no conservan los datos de la página anterior.
- Las pantallas (`ui/src/pages/historico.tsx`, `snapshots.tsx`, `restauraciones.tsx`) gatean el «cargando» con `isPending`; con datos de reserva `isPending` se queda en `false` y la tabla no se desmonta.
- Anotado como follow-up en `docs/FEATURE-033-paginacion-de-listados.md`. `@tanstack/react-query` ^5.107.1 ya está instalado.

## 3. Decisión de arquitectura

- Añadir `placeholderData: keepPreviousData` de React Query a los tres `useQuery`. Mientras llega la página nueva, `data` conserva la anterior, `isPlaceholderData` es `true` e `isPending` es `false`.
- No se cambian las claves de caché (cada página sigue siendo su propia entrada) ni se toca la API.
- El `showTotal` del paginador sigue leyendo el `total` de la respuesta; durante la transición es el de la página anterior, coherente con las filas que se ven.

## 4. Alcance

- `ui/src/hooks/useEjecuciones.ts`, `useSnapshots.ts`, `useRestauraciones.ts`: import de `keepPreviousData` + `placeholderData`.
- Tests: `ui/src/hooks/useEjecuciones.test.tsx`, `useSnapshots.test.tsx` y `ui/src/hooks/useRestauraciones.test.tsx` (nuevo).
- Spec: requisito nuevo en la capability `ui`. Doc: `docs/FEATURE-038-keep-previous-data-146.md`.

## 5. Tareas (TDD)

### 0. Preparación
- [x] 0.1 `cd ui && pnpm run typecheck && pnpm run lint && pnpm test` en verde antes de tocar nada.
- [x] 0.2 Localizar los tres hooks y sus tests.

### 1. Conservar la página anterior al paginar
- **RED**
  - [ ] 1.1 Test en `useEjecuciones.test.tsx`: cambiar de página con la segunda petición pendiente → la página anterior se conserva, `isPlaceholderData` `true`, `isPending` `false`.
  - [ ] 1.2 El mismo caso en `useSnapshots.test.tsx`.
  - [ ] 1.3 El mismo caso en `useRestauraciones.test.tsx` (nuevo).
- **GREEN**
  - [ ] 1.4 Añadir `placeholderData: keepPreviousData` a los tres hooks (con su import).
- **REFACTOR**
  - [ ] 1.5 JSDoc de los tres hooks anotando la conservación y el porqué.
  - [ ] 1.6 `cd ui && pnpm run lint && pnpm run typecheck && pnpm test` en verde.

### 2. Verificación
- [x] 2.1 `cd ui && pnpm test` en verde.
- [x] 2.2 `cd ui && pnpm run lint && pnpm run typecheck` limpios.

### 3. Cierre
- [x] 3.1 PR a `development` con `Closes #146`.
- [x] 3.2 `docs/FEATURE-038-keep-previous-data-146.md`.
- [x] 3.3 Renombrar `plans/PLAN-038.md` → `plans/PLAN-038-COMPLETED.md` con `git mv`.
- [x] 3.4 `openspec archive keep-previous-data-146`.

## 6. Criterios de aceptación

- Al cambiar de página en los tres listados, la tabla no se desmonta (no se entra en la rama de «cargando»): `isPending` es `false` e `isPlaceholderData` es `true` mientras llega la página nueva.
- Las claves de caché y el contrato de `limit`/`offset` no cambian; cada página se sigue pidiendo al servidor.
- `pnpm test`, `pnpm run lint` y `pnpm run typecheck` en verde.
