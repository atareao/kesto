# Tasks

## SDD

- [ ] Aprobar este change proposal (`identidad-visual-y-tema`) — issue #60.
- [ ] `openspec validate identidad-visual-y-tema --strict` en verde.

## Preparación

- [ ] Crear rama `feature/identidad-visual-y-tema` desde `development`.

## Assets de marca

- [ ] Copiar a `ui/public/` los favicons (`favicon.ico`, `favicon-16/32/48/64.png`, `apple-touch-icon.png`, `android-chrome-192/512.png`, `maskable-icon-512x512.png`, `mstile-150x150.png`), `site.webmanifest` y la imagen Open Graph.
- [ ] Copiar a `ui/src/assets/` los logos (horizontal claro/oscuro, icono claro/oscuro) e importarlos donde se usen.
- [ ] `ui/index.html`: declarar iconos, manifest, `theme-color` y meta de Open Graph (a partir de `head-tags.html`).

## TDD — tema claro/oscuro con oscuro por defecto

- [ ] RED: test de `ui/src/services/tema` — sin nada guardado el modo es `oscuro`; con `kesto-tema=claro` el modo es `claro`; `alternar()` cambia de modo y persiste en `localStorage`. Debe fallar.
- [ ] GREEN: implementar `ModoDeTema`, `leerTemaGuardado`, `ProveedorDeTema`/`useTema` y la construcción del `ThemeConfig` de Antd (algoritmo y tokens de marca según el modo).
- [ ] RED: test del componente de toggle — se pinta con nombre accesible, alterna al activarse con teclado y anuncia su estado. Debe fallar.
- [ ] GREEN: implementar el componente de toggle y montarlo en la cabecera de `Layout`.
- [ ] RED: test — `main.tsx`/raíz aplica el `ConfigProvider` con el tema por defecto oscuro. Debe fallar.
- [ ] GREEN: envolver el árbol con `ProveedorDeTema` + `ConfigProvider`.
- [ ] REFACTOR: quitar los colores hardcodeados de `estilos.css`; que cabecera y lateral tomen los colores de los tokens.

## Marca y pulido

- [ ] `Layout`: logo en el lateral (variante según el modo) y cabecera sin colores a mano.
- [ ] Pulir espaciados y tarjetas del Dashboard para ambos temas.
- [ ] REFACTOR: `eslint` y `tsc` limpios.

## Documentación

- [ ] `docs/FEATURE-016-identidad-visual-y-tema.md`.

## Verificación

- [ ] `pnpm run lint`, `npx tsc --noEmit` y `pnpm test` en verde en `ui/`.
- [ ] `just check-all` en verde.
- [ ] Puerta del protocolo (`just check-spec`) en verde con este change activo.
- [ ] Revisión con `@react-reviewer`.

## Verificación real (banco de pruebas)

- [ ] Construir la imagen local con el panel nuevo y recrear el contenedor `kesto`.
- [ ] Comprobar en `http://127.0.0.1:8080`: favicon en la pestaña, tema **oscuro** al primer arranque, el toggle pasa a claro, persiste tras recargar, y cabecera/menú/contenido legibles en ambos temas.
