# PLAN-029 — Portada pública para usuarios sin sesión

- **Issue:** #118
- **Estado:** en curso
- **Rama:** `feature/portada-publica`
- **Base:** `development`

## Objetivo

Que quien llega a Kesto sin sesión vea una portada de producto en `/` —logo, título, tagline y botón de entrada— en vez del desvío seco a `/sesion`. Referencia de diseño: la portada de shuul (`/data/rust/shuul/frontend/src/pages/public/home_page.tsx`), adaptada a Antd y a la marca de Kesto.

## Contexto actual

- `ui/src/App.tsx` declara las rutas: el índice (`Dashboard`) y el resto de pantallas van bajo `SoloConSesion`; `/sesion` queda fuera de la guarda.
- `SoloConSesion` redirige a `/sesion` cuando no hay sesión.
- `ui/src/pages/sesion.tsx` ya distingue `disabled` (panel abierto), `oidc` con sesión y `oidc` sin sesión (botón que navega a `/api/v1/auth/login`)
- Assets disponibles en `ui/public`: `kesto-icon.png`, `kesto-icon-dark.png`, `og-image-dark.png`, `site.webmanifest`

## Tareas

- [ ] **T1.** Crear `ui/src/pages/portada.tsx` con la portada: logo, título, tagline y botón. Sin peticiones nuevas a la API; reutiliza `useSesion()` para decidir el destino del botón.
- [ ] **T2.** Añadir `ui/src/pages/portada.test.tsx`: en `oidc` sin sesión el botón navega a `/api/v1/auth/login`; en `disabled` entra al panel; con sesión no se muestra la portada.
- [ ] **T3.** Cambiar la ruta índice de `App.tsx` para que `/` resuelva a la portada cuando no hay sesión y al panel cuando la hay. Mantener `/sesion` para el callback y los mensajes.
- [ ] **T4.** Revisar estilos: que la portada ocupe el alto disponible y se vea bien en móvil; sin dependencias nuevas.
- [ ] **T5.** Actualizar `ui/src/App.test.tsx` (o el test de rutas existente) con el nuevo comportamiento de `/`

## Criterios de aceptación

1. Sin sesión en modo `oidc`, `/` muestra la portada y el botón lleva a `/api/v1/auth/login`.
2. Sin sesión en modo `disabled`, `/` entra al panel sin pantalla intermedia.
3. Con sesión, `/` sigue mostrando el panel.
4. `/sesion` conserva su comportamiento actual.
5. `pnpm run lint`, `npx tsc --noEmit` y `pnpm test` en `ui/` pasan sin errores.
6. `just check-all` pasa en verde.

## Verificación

```bash
cd ui && pnpm run lint && npx tsc --noEmit && pnpm test
just check-all
```

## Riesgos

- Duplicar la lógica de `Sesion`: la portada debe reutilizar `useSesion()`, no reimplementar la comprobación.
- Romper el callback de OIDC si se mueve `/sesion`. No se mueve.
- Tests de rutas frágiles: comprobar comportamiento visible, no detalles de implementación.

## Documentación

Al cerrar: `docs/FEATURE-029-portada-publica.md` y renombrar este plan a `PLAN-029-COMPLETED.md`