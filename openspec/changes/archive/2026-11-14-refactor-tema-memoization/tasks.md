# Tasks — refactor-tema-memoization

## 0. Preparación

- [x] 0.1 Verificar que los tests actuales de tema pasan en verde antes de tocar código (`pnpm test` en `ui/`).
- [x] 0.2 Localizar con `rg` los `useCallback`/`useMemo` de `ui/src/services/tema.tsx` y `ui/src/services/useTema.ts` y anotar los prescindibles.

## 1. RED

- [x] 1.1 Revisar que los tests de tema cubren defecto oscuro, persistencia en `kesto-tema`, conmutación y accesibilidad; añadir los que falten.
- [x] 1.2 Añadir una comprobación que falle si el tsconfig de aplicación expone `"types": ["node"]` (script o test de configuración).
- [x] 1.3 Ejecutar `pnpm test` y `npx tsc --noEmit` y confirmar el fallo esperado.

## 2. GREEN

- [x] 2.1 Eliminar `useCallback`/`useMemo` redundantes en `ui/src/services/tema.tsx` y `ui/src/services/useTema.ts`.
- [x] 2.2 Crear `ui/tsconfig.test.json` que extienda de `ui/tsconfig.json` y declare `"types": ["node"]`.
- [x] 2.3 Quitar `"types": ["node"]` de `ui/tsconfig.json`.
- [x] 2.4 Ajustar scripts de `ui/package.json` para separar typecheck de aplicación y de tests, si hace falta.
- [x] 2.5 Ejecutar `pnpm test`, `npx tsc --noEmit` y `pnpm run lint`; confirmar verde.

## 3. REFACTOR y verificación

- [x] 3.1 Revisar que no quedan imports ni referencias a Node en código de aplicación.
- [x] 3.2 Ejecutar `just check-all` en la raíz.
- [x] 3.3 Revisar el diff de UI con `@react-reviewer`.
- [x] 3.4 Actualizar `docs/FEATURE-016-identidad-visual-y-tema.md` solo si el cambio altera lo documentado.

## 4. Cierre

- [x] 4.1 Archivar el change con `openspec archive refactor-tema-memoization`.
- [x] 4.2 Renombrar `plans/PLAN-017.md` a `plans/PLAN-017-COMPLETED.md`.
- [x] 4.3 Abrir PR a `development` con gitmoji y cerrar el issue #62.
