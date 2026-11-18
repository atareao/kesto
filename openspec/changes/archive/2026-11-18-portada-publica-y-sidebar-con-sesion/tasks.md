# Tasks: portada-publica-y-sidebar-con-sesion

## 1. RED

- [x] Escribir en `ui/src/components/Layout.test.tsx` los tests que exigen que la barra lateral solo se monte cuando hay panel: sin sesión (`oidc` sin sesión) y mientras el modo/sesión se resuelven, `getByRole('navigation', { name: 'Principal' })` NO SHALL estar; con `disabled` o `oidc` con sesión SHALL estar.
- [x] Escribir en `ui/src/components/Layout.test.tsx` el test que exige que «Cerrar sesión» esté en el pie de la barra lateral, junto al conmutador de tema, realice `POST /api/v1/auth/logout`, vacíe la caché y vuelva a `/`.
- [x] Escribir en `ui/src/App.test.tsx` los tests que exigen que la ruta `/sesion` ya no exista y que, sin sesión, una ruta protegida distinta de `/` desvíe a `/` (no a `/sesion`).
- [x] Escribir en `ui/src/pages/portada.test.tsx` los tests que exigen la marca en la portada: logo visible con `data-testid="logo-de-la-portada"`, nombre «Kesto» como encabezado de nivel 1, línea de descripción y botón «Entrar» que navega a `/api/v1/auth/login`; y que el mismo asset del logo es legible tanto en tema claro como en oscuro.
- [x] Ejecutar los tests de UI desde el terminal y confirmar que fallan por la funcionalidad nueva (RED), mientras el resto de la suite sigue en verde.

## 2. GREEN

- [x] `ui/src/components/Layout.tsx`: montar la `Sider` (menú lateral «Principal») solo cuando haya panel (`disabled` o `oidc` con sesión), y no montarla sin sesión ni mientras el modo/sesión se resuelven.
- [x] `ui/src/components/Layout.tsx`: mover «Cerrar sesión» al pie de la barra lateral, junto al conmutador de tema.
- [x] Eliminar la ruta `/sesion` y su página (`ui/src/pages/sesion.tsx`).
- [x] Ajustar `SoloConSesion` para que, sin sesión, desvíe a `/` en lugar de a `/sesion`.
- [x] `ui/src/pages/portada.tsx`: rediseñar la portada con logo (`data-testid="logo-de-la-portada"`), nombre «Kesto» (encabezado de nivel 1), una línea de descripción y botón «Entrar» hacia `/api/v1/auth/login`, con un único asset de logo legible en ambos temas.
- [x] Ejecutar los tests de UI y el typecheck (`npx tsc --noEmit`) desde el terminal y confirmar que toda la suite pasa (GREEN).

## 3. REFACTOR

- [x] Limpiar el código tocado (imports, tipos, condiciones del layout y de la portada) sin alterar el comportamiento.
- [x] Ejecutar `just ui-check` (lint + typecheck + tests del panel) desde el terminal y confirmar que pasa.
- [x] Archivar el change en el spec consolidado: `openspec archive portada-publica-y-sidebar-con-sesion`.

## 4. Cierre

- [x] Marcar las tareas completadas de este plan.
- [x] Escribir `docs/FEATURE-031-*.md` con qué es, ficheros implicados, cómo funciona, decisiones y follow-ups.
