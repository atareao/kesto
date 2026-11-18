# Tasks

## 1. Portada — prueba primero

- [ ] 1.1 `ui/src/pages/portada.test.tsx`: prueba en rojo de que en modo `oidc` sin sesión la portada se pinta, no enseña el panel y el botón navega a `/api/v1/auth/login` → verificación: `npx vitest run ui/src/pages/portada.test.tsx` falla por ausencia de la portada.
- [ ] 1.2 En la misma prueba: modo `disabled` y modo `oidc` con sesión muestran el Dashboard, y con la sesión en el aire se muestra la carga → verificación: la prueba sigue en rojo solo por los casos de la portada.
- [ ] 1.3 En la misma prueba: al abrir la portada no se añaden peticiones más allá de las de `useSesion()` → verificación: la prueba falla por la portada, no por peticiones inesperadas.

## 2. Portada — implementación mínima

- [ ] 2.1 `ui/src/pages/portada.tsx`: componente que decide con `useSesion()` entre carga, panel (modo `disabled` o con sesión) y portada pública → verificación: `npx vitest run ui/src/pages/portada.test.tsx` en verde.
- [ ] 2.2 Botón de entrada con navegación a `/api/v1/auth/login` (sin petición de datos) → verificación: prueba del botón en verde.

## 3. Rutas — prueba primero

- [ ] 3.1 `ui/src/App.test.tsx`: prueba en rojo de que la ruta índice `/` en modo `disabled` entra al panel y en modo `oidc` sin sesión pinta la portada → verificación: `npx vitest run ui/src/App.test.tsx` falla en los casos nuevos.
- [ ] 3.2 En la misma prueba: `/sesion` sigue en su ruta con su comportamiento y una ruta protegida sin sesión sigue desviándose a `/sesion` → verificación: rojo solo en los casos nuevos.

## 4. Rutas — implementación mínima

- [ ] 4.1 `ui/src/App.tsx`: la ruta índice pasa a `Portada` **fuera** de `SoloConSesion`, y las demás rutas protegidas conservan la guarda → verificación: `npx vitest run ui/src/App.test.tsx` en verde.
- [ ] 4.2 Comprobar que no se duplica la consulta de sesión al mover la ruta índice → verificación: prueba de peticiones de 1.3 en verde.

## 5. Cierre

- [ ] 5.1 `npx tsc --noEmit` y `pnpm run lint` sin errores en `ui/`.
- [ ] 5.2 `just check-all` verde.
- [ ] 5.3 `docs/FEATURE-029-portada-publica.md` escrito.
- [ ] 5.4 Change archivado y plan `plans/PLAN-029.md` renombrado a `-COMPLETED`.
