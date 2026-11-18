# PLAN-031 — Portada pública con logo y sidebar solo con sesión

- **Issue:** [#125](https://github.com/atareao/kesto/issues/125)
- **Change SDD:** `openspec/changes/portada-publica-y-sidebar-con-sesion/`
- **Feature doc (al cerrar):** `docs/FEATURE-031-portada-publica-y-sidebar-con-sesion.md`
- **Estado:** propuesto — pendiente de aprobación del usuario

## 1. Objetivo

Sin sesión, el panel no debe mostrar barra lateral y la ruta `/sesion` deja de existir: la única pantalla pública es la portada en `/`, rediseñada al estilo de shuul (logo + nombre + tagline + botón «Entrar»). «Cerrar sesión» se reubica en el pie de la barra lateral.

## 2. Matriz de comportamiento objetivo

| Estado de sesión | `/` | Rutas protegidas (`/tareas`, …) | Barra lateral |
|---|---|---|---|
| `disabled` (desarrollo) | Dashboard | accesibles | visible |
| `oidc` con sesión | Dashboard | accesibles | visible |
| `oidc` sin sesión | portada pública | desvían a `/` | oculta |
| `/health`/sesión resolviendo | carga | carga | oculta |

## 3. Decisiones de diseño

1. **Una sola `Layout` con la `Sider` condicionada**, no un `PublicLayout` aparte: conserva la estructura de rutas plana de Kesto y un único armazón. La `Sider` se monta solo cuando hay panel (`modo === 'disabled'` o `oid, con sesión). Mientras se resuelve el modo/sesión NO se monta (evita el parpadeo de la barra).
2. **Se elimina `/sesion`**: se borran la ruta (`App.tsx`) y `pages/sesion.tsx` con su test. El login es el botón «Entrar» de la portada (navega a `/api/v1/auth/login`; el `302` lo hace el servidor).
3. **«Cerrar sesión» en el pie de la barra lateral** (`.pie-del-panel`), junto al `ConmutadorDeTema`. Usa `useCerrarSesion()` y, tras el `POST /api/v1/auth/logout`, navega a `/`.
4. **La portada reutiliza `useSesion()`** (sin peticiones nuevas): con panel → `Dashboard`; sin sesión → landing pública.
5. **Marca**: nuevo emblema `ui/src/assets/kesto-emblema.png` —fondo transparente, paleta navy/teal/verde/naranja—, legible en ambos temas (un único asset, sin variantes). En la portada va el emblema + el nombre «Kesto» como `H1` + el tagline como párrafo (patrón shuul: emblema + nombre como texto).
6. **Sin cambios de backend**: `/sesion` es ruta de cliente; la API no cambia.

## 4. Ficheros afectados

| Fichero | Cambio |
|---|---|
| `ui/src/App.tsx` | Quitar la ruta `sesion` |
| `ui/src/components/Layout/Layout.tsx` | `Sider` condicional; quitar la entrada «Sesión» del menú; botón «Cerrar sesión» en el pie |
| `ui/src/pages/portada.tsx` | Landing pública con logo + nombre + tagline |
| `ui/src/pages/sesion.tsx` | **Borrar** |
| `ui/src/pages/sesion.test.tsx` | **Borrar** |
| `ui/src/components/SoloConSesion/SoloConSesion.tsx` | Desvío sin sesión → `/` |
| `ui/src/services/session.ts` | Solo comentarios (el `401` ya no lleva a `/sesion`) |
| `ui/src/assets/kesto-logo.png` · `kesto-logo-dark.png` | **Nuevos** (variantes claro/oscuro) |
| `ui/src/App.test.tsx`, `ui/src/components/Layout/Layout.test.tsx`, `ui/src/pages/portada.test.tsx` | Ajustar y ampliar |

## 5. Tareas (TDD)

### RED
- 5.1 `App.test.tsx`: eliminar el bloque «ruta de sesión»; ajustar el recuento de enlaces del menú (7 → 6); añadir escenario «una ruta protegida sin sesión desvía a `/`».
- 5.2 `Layout.test.tsx`: montar con `fetch` doblado. Escenarios: en `oidc` sin sesión no hay `navigation` «Principal» ni `Sider`; en `disabled`/`oidc` con sesión sí; hay un botón «Cerrar sesión» en `.pie-del-panel`.
- 5.3 `portada.test.tsx`: escenario «la portada muestra el logo de la marca» (`data-testid="logo-de-la-portada"`) y conserva el `H1` «Kesto».

### GREEN
- 5.4 `Layout.tsx`: `Sider` condicional + logout en el pie + quitar entrada «Sesión».
- 5.5 `App.tsx`: quitar ruta `/sesion`; borrar `pages/sesion.tsx` y su test.
- 5.6 `SoloConSesion.tsx`: `<Navigate to="/" replace />`.
- 5.7 `portada.tsx`: landing con el emblema `kesto-emblema.png`, el nombre «Kesto» como H1 y un tagline.

### REFACTOR
- 5.8 `just ui-check` (lint + `tsc --noEmit` + vitest + build) en verde.
- 5.9 `openspec archive portada-publica-y-sidebar-con-sesion` tras aprobar los escenarios.
- 5.10 `docs/FEATURE-031-*.md` al cerrar y renombrar el plan a `PLAN-031-COMPLETED.md`.

## 6. Verificación final

- `just ui-check` verde y `just check-spec` con el change activo.
- Manual (modo `oidc` sin sesión): `/` sin barra lateral, `/tareas` desvía a `/`; con sesión: panel con barra y logout operativo.