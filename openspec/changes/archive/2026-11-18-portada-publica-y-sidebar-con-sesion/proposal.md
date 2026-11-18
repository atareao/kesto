# Proposal: portada-publica-y-sidebar-con-sesion

## Why

Hoy la `Sider` de `Layout.tsx` se pinta en todas las rutas, incluida la portada pública y la pantalla de sesión. Eso deja un menú lateral visible cuando no hay ninguna sesión abierta: un chasis de producto (rutas protegidas, plegado, conmutador de tema) delante de un visitante que todavía no ha entrado. Además, el login vive en una ruta propia `/sesion`, lo que obliga a mantener un desvío extra y una pantalla que no aporta nada que la portada no pueda ofrecer.

Se quiere alinear la experiencia con el resto del producto: sin sesión, el visitante solo ve la portada pública; el login se ofrece desde ahí; el menú lateral solo existe cuando hay panel (modo `disabled` o `oidc` con sesión); y «Cerrar sesión» se agrupa con los demás mandos del chasis, al pie de la barra lateral. Sin cambios de backend: solo se reordena y se condiciona lo que el SPA ya hacía.

## What Changes

- **(a) Sin sesión, no se muestra la barra lateral.** La `Sider` del layout deja de pintarse en todas las rutas y pasa a montarse solo cuando hay panel (`auth_mode` `disabled` o `oidc` con sesión). Mientras el modo/sesión se resuelven, tampoco se monta.
- **(b) Se elimina la ruta `/sesion`.** El login pasa a ofrecerse únicamente desde la portada pública (`/`); la ruta y su página (`pages/sesion.tsx`) desaparecen, y el desvío de las rutas protegidas sin sesión apunta a `/` en vez de a `/sesion`.
- **(c) «Cerrar sesión» pasa al pie de la barra lateral**, junto al conmutador de tema, en lugar de vivir en la pantalla de sesión.
- **(d) Se rediseña la portada al estilo de shuul**: logo de marca + nombre «Kesto» (encabezado de nivel 1) + una línea de descripción (tagline) + botón «Entrar» que navega a `/api/v1/auth/login`. El logo es un único asset con fondo transparente, legible tanto en el tema claro como en el oscuro (sin variantes por tema).
- **Ajuste de spec acompañante:** el requisito de identidad de marca del panel (`ui`) incorpora que la portada pública también muestra el logo de Kesto, y el requisito del layout y el de sesión se adaptan a que la barra lateral solo existe con panel y el login/cierre ya no viven en `/sesion`.

## Non-goals

- No se toca el backend: no hay endpoints, contratos, variables de entorno ni migraciones nuevas.
- No se cambia el mecanismo de autenticación (OIDC se mantiene tal cual; el botón sigue siendo una navegación a `/api/v1/auth/login` y el cierre sigue siendo `POST /api/v1/auth/logout`).
- No se altera el comportamiento del modo `disabled` en cuanto a rutas `/api/v1/auth/*` (siguen sin llamarse).
- No se rediseñan las pantallas del panel (Dashboard, Tareas, Histórico, Snapshots, Ajustes) ni el stream SSE.
- No se introduce una pantalla de login propia: el login sigue delegando en el servidor.
