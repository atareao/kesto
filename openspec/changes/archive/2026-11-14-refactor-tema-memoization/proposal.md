# Proposal — refactor-tema-memoization

## Intención

Cerrar dos deudas técnicas de FEATURE-016 sin cambiar el comportamiento visible del tema claro/oscuro:

1. Eliminar los `useCallback`/`useMemo` redundantes en `ui/src/services/tema.tsx` y `ui/src/services/useTema.ts`.
2. Acotar `"types": ["node"]` de `ui/tsconfig.json` a un tsconfig específico de tests.

## Alcance

- Refactor interno de memoización en el provider/hook de tema.
- Separación de configuración TypeScript entre aplicación y tests.
- Tests de comportamiento que protejan el tema actual (defecto oscuro, persistencia en `kesto-tema`, conmutador accesible).

## No-objetivos

- No cambia la paleta, los tokens, el contraste WCAG AA ni el comportamiento del conmutador.
- No se toca el backend, la API ni el esquema de datos.
- No se aborda el code-splitting de Antd ni otros follow-ups de FEATURE-016.

## Impacto

- Solo UI. Riesgo bajo: sin cambios de contrato público ni de persistencia.
- El código de aplicación deja de ver tipos de Node; los tests conservan acceso a ellos.

## Referencias

- Issue #62
- `plans/PLAN-017.md`
- `docs/FEATURE-016-identidad-visual-y-tema.md`
- `openspec/specs/ui/spec.md`
