# Proposal

## Why

En modo `oidc`, un visitante sin sesión que abre la raíz del sitio (`/`) no ve nada que explique qué es Kesto ni cómo entrar: la guarda de sesión lo desvía a `/sesion`, una pantalla pensada para quien ya conoce el producto. La raíz es la dirección que se comparte y la que se teclea, así que debería presentar el producto y ofrecer la entrada, sin exponer ninguna pantalla del panel. Es el issue **#118** y se entrega con el plan `plans/PLAN-029.md`.

## What Changes

- Nueva pantalla pública **portada** (`ui/src/pages/portada.tsx`) que se pinta en la raíz `/` cuando el modo es `oidc` y no hay sesión: presenta Kesto y ofrece un botón que **navega** a `/api/v1/auth/login`.
- La ruta índice de `ui/src/App.tsx` deja de estar dentro de la guarda `SoloConSesion` y pasa a decidir con `useSesion()`: en modo `disabled` y con sesión válida muestra el panel; en `oidc` sin sesión, la portada.
- La portada no añade peticiones: reutiliza `useSesion()`, que ya consulta `/api/v1/health` y la sesión.
- `/sesion` conserva su ruta y su comportamiento; la guarda sigue desviando ahí las rutas protegidas sin sesión.

## Capabilities

### New Capabilities

- `portada-publica`: portada pública en la raíz, sin sesión y sin peticiones nuevas a la API.

### Modified Capabilities

(none)

## Impact

- Frontend: `ui/src/pages/portada.tsx` (nuevo), `ui/src/pages/portada.test.tsx` (nuevo), `ui/src/App.tsx` (ruta índice) y `ui/src/App.test.tsx` (rutas).
- Sin cambios en `src/`: no hay endpoints ni contratos nuevos; el login sigue siendo la navegación existente a `/api/v1/auth/login`.
- Sin cambios incompatibles: en modo `disabled` y con sesión, `/` sigue llevando al panel; `/sesion` no se mueve.
