# FEATURE-031 — Portada pública con logo y barra lateral solo con sesión

- **Plan:** `plans/PLAN-031-COMPLETED.md`
- **Issue:** #125
- **Change SDD:** `openspec/changes/archive/2026-11-18-portada-publica-y-sidebar-con-sesion/`
- **Capacidades afectadas:** `portada-publica`, `ui`

## Qué es

Sin sesión, el panel ya no muestra barra lateral y la ruta `/sesion` ha dejado de existir. La única pantalla pública es la portada en `/`, rediseñada al estilo de shuul —emblema, nombre «Kesto», tagline y botón «Entrar»—. «Cerrar sesión» se ha reubicado en el pie de la barra lateral.

## Ficheros implicados

| Fichero | Cambio |
|---|---|
| `ui/src/components/Layout/Layout.tsx` | La `Sider` se monta solo cuando hay panel (`auth_mode` `disabled` o `oidc` con sesión) y nunca mientras el modo/sesión se resuelven; identidad de la sesión (email + `sub`) en el pie, visible con la barra desplegada; «Cerrar sesión» en el pie, solo en modo `oidc` con sesión |
| `ui/src/components/PanelConTema/PanelConTema.tsx` | Envuelve el árbol en el `App` de antd para proveer `message` |
| `ui/src/App.tsx` | Eliminada la ruta `/sesion` |
| `ui/src/pages/sesion.tsx` · `ui/src/pages/sesion.test.tsx` | Borrados |
| `ui/src/components/SoloConSesion/SoloConSesion.tsx` | Sin sesión, las rutas protegidas desvían a `/` |
| `ui/src/pages/portada.tsx` | Landing con el emblema (`data-testid="logo-de-la-portada"`), `H1` «Kesto», tagline y botón «Entrar» |
| `ui/src/assets/kesto-emblema.png` | Nuevo emblema (fondo transparente, 256×225, paleta navy/teal/verde/naranja), legible en ambos temas |
| `ui/src/estilos.css` | Estilos del pie del panel y de la identidad de la sesión |
| `ui/src/services/session.ts` | Comentarios: el `401` devuelve a la portada |

## Cómo funciona

- La raíz `/` usa `useSesion()` (compartido por claves de caché con `Layout` y `SoloConSesion`, sin peticiones extra): en modo `disabled` o con sesión muestra el Dashboard; en `oidc` sin sesión muestra la portada pública.
- La `Sider` solo se monta cuando `!cargando` y (`disabled` o `oidc` con sesión); se evita así el parpadeo de la barra mientras se resuelve el modo.
- El login es el botón «Entrar» de la portada, que **navega** a `/api/v1/auth/login` (navegación del navegador; el `302` lo hace el servidor).
- «Cerrar sesión» hace `POST /api/v1/auth/logout`, descarta la caché y navega a `/`; si falla, muestra el mensaje de error de la envoltura con `message` de antd.
- Sin cambios de backend: la API no se toca.

## Variables/config

Ninguna nueva. Aplica en modo `oidc`; en modo `disabled` el panel sigue abierto y la barra lateral siempre visible.

## Cómo habilitar/desplegar

Sin pasos especiales: es un cambio de cliente. `just ui-check` valida el panel; el `build` se sirve como siempre.

## Decisiones

- Se elimina `/sesion` en lugar de conservarla: el login vive en la portada y no aporta nada una pantalla aparte.
- Una sola `Layout` con la `Sider` condicionada, no un `PublicLayout` aparte (se conserva la estructura de rutas plana de Kesto).
- La identidad de la sesión se oculta al plegar la barra (la columna de 80 px no admite un email largo).
- «Cerrar sesión» solo se ofrece en modo `oidc` con sesión: en `disabled` las rutas `/api/v1/auth/*` no existen (`404`).

## Follow-ups

- El chunk del panel supera 500 kB (aviso de build, preexistente): valorar code-splitting.
- El emblema se sirve como PNG; valorar WebP/AVIF si se necesita más ahorro.
