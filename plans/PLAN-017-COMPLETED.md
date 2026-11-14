# PLAN-017 — Refactor de memoización en tema y acotado de `types: ["node"]`

**Issue:** #62 — ♻️ refactor(ui): eliminar useCallback/useMemo redundantes en tema y acotar types node
**Base:** `v0.6.1` (FEATURE-016 entregada)
**Rama:** `feature/refactor-tema-memoization` (desde `development`)
**Change SDD:** `openspec/changes/archive/2026-11-14-refactor-tema-memoization/`
**Estado:** Terminado

## 1. Objetivo

Cerrar dos deudas técnicas de FEATURE-016 sin cambiar el comportamiento visible del tema claro/oscuro:

1. Eliminar los `useCallback`/`useMemo` redundantes de `ui/src/services/tema.tsx` (y del contexto/hook asociado si aplica).
2. Acotar "types": ["node"] de `ui/tsconfig.json` a un tsconfig específico de tests, de forma que el código de aplicación no tenga visibilidad de tipos de Node.

## 2. Contexto y alcance

- El tema está implementado en `ui/src/services/tema.tsx`, `ui/src/services/useTema.ts` y consumido por `ui/src/components/ConmutadorDeTema/` y `ui/src/components/PanelConTema/`.
- La persistencia usa `localStorage` con la clave `kesto-tema`; el modo por defecto es oscuro y no sigue al sistema.
- `ui/tsconfig.json` declara "types": ["node"] de forma global. Se quiere que solo los tests (Vitest, `node` APIs) lo vean.
- **Fuera de alcance:** cambios de paleta, tokens, contraste WCAG, comportamiento del conmutador, code-splitting de Antd y cualquier otro follow-up de FEATURE-016.

## 3. Mapa técnico

| Fichero | Cambio previsto |
|---|---|
| `ui/src/services/tema.tsx` | Retirar `useCallback`/`useMemo` que no evitan re-renders ni estabilizan dependencias; conservar la API pública del provider/hook. |
| `ui/src/services/useTema.ts` | Revisar si conserva hooks redundantes; mantener la firma pública. |
| `ui/src/services/tema.test.tsx` | Ajustar/añadir tests de comportamiento si el refactor toca identidad de referencias. |
| `ui/tsconfig.json` | Quitar "types": ["node"] del tsconfig de aplicación. |
| `ui/tsconfig.test.json` (nuevo) | Extender del anterior y declarar "types": ["node"] solo para tests. |
| `ui/package.json` | Ajustar scripts de typecheck/test si hace falta para usar el tsconfig de tests. |
| `ui/vite.config.ts` | Revisar si el setup de tests necesita el nuevo tsconfig (solo si aplica). |

## 4. Change SDD (OpenSpec)

Crear `openspec/changes/refactor-tema-memoization/` con:

- `proposal.md`: intención, alcance, impacto y no-objetivos.
- `specs/ui/spec.md`: delta con contratos y escenarios BDD.
- `tasks.md`: checklist TDD de este plan.

Contratos que la delta debe fijar:

- El provider de tema mantiene su API pública (valor de tema, conmutador, persistencia en `kesto-tema`).
- El tipo `Tema` y el hook `useTema` no cambian de firma.
- El código de aplicación compila sin tipos de Node; los tests sí los ven.
- El comportamiento por defecto (oscuro) y el contraste WCAG AA se mantienen.

Escenarios BDD mínimos:

1. **Dado** un usuario sin preferencia guardada, **cuando** carga el panel, **entonces** se aplica el tema oscuro y se persiste `kesto-tema=oscuro`.
2. **Dado** un usuario con `kesto-tema=claro`, **cuando** carga el panel, **entonces** se aplica el tema claro sin destello.
3. **Dado** el conmutador, **cuando** se pulsa, **entonces** cambia el tema y actualiza `localStorage`.
4. **Dado** el código de aplicación, **cuando** se ejecuta `tsc --noEmit` con el tsconfig de app, **entonces** no se resuelven tipos de Node.
5. **Dado** los tests, **cuando** se ejecutan con el tsconfig de tests, **entonces** los tipos de Node están disponibles.

## 5. Tareas TDD

### Fase 0 — Preparación (si aplica)

- [x] Verificar que los tests actuales de tema pasan en verde antes de tocar código (`pnpm test`).
- [x] Revisar con `rg` los usos de `useCallback`/`useMemo` en `tema.tsx` y `useTema.ts` y anotar cuáles son prescindibles.

### Fase 1 — RED

- [x] Crear/ajustar tests que fijen el comportamiento actual del tema (persistencia, cambio, defecto oscuro) si no están cubiertos.
- [x] Añadir un test/check que falle si el tsconfig de aplicación expone `node` (p. ej. importar un tipo de Node en app y esperar error) o validar la configuración con un script.
- [x] Ejecutar `pnpm test` y `npx tsc --noEmit` y confirmar el fallo esperado.

### Fase 2 — GREEN

- [x] Eliminar `useCallback`/`useMemo` redundantes en `ui/src/services/tema.tsx` y `useTema.ts`.
- [x] Crear `ui/tsconfig.test.json` y quitar "types": ["node"] de `ui/tsconfig.json`.
- [x] Ajustar scripts de `ui/package.json` para que el typecheck de app y el de tests usen el tsconfig correcto.
- [x] Ejecutar `pnpm test`, `npx tsc --noEmit` y `pnpm run lint`; confirmar verde.

### Fase 3 — REFACTOR / verificación

- [x] Revisar que no quedan imports ni dependencias de Node en código de aplicación.
- [x] Ejecutar `just check-all` en la raíz.
- [x] Revisar con `@react-reviewer` el diff de UI.
- [x] Actualizar `docs/FEATURE-016-identidad-visual-y-tema.md` solo si el cambio altera lo documentado.

## 6. Criterios de aceptación

- [x] `pnpm run lint`, `npx tsc --noEmit` y `pnpm test` en verde en `ui/`.
- [x] `just check-all` verde en la raíz.
- [x] Sin cambios de comportamiento en tema claro/oscuro ni en `kesto-tema`.
- [x] `tsconfig.json` de aplicación sin "types": ["node"]; tests con su propio tsconfig.
- [x] Change SDD archivado y `tasks.md` marcado.
- [x] Plan renombrado a `plans/PLAN-017-COMPLETED.md` al cerrar.
- [x] PR abierto a `development` con gitmoji y issue #62 referenciado.

## 7. Riesgos

- **Bajo:** el refactor puede alterar identidad de referencias y provocar re-renders; se cubre con tests de comportamiento, no de implementación.
- **Bajo:** mover `types` puede romper imports de tests; se resuelve con el tsconfig de tests.
- **Nulo:** no hay cambios de esquema, API ni persistencia.

## 8. Flujo git

1. `git checkout development && git pull origin development`
2. `git checkout -b feature/refactor-tema-memoization`
3. Commits con gitmoji: `♻️ refactor(ui): ...`, `✅ test(ui): ...`, `🔧 chore(ui): ...`
4. PR a `development`; nada directo a `main`/`development`.