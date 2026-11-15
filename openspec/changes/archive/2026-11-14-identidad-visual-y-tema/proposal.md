# Proposal

## Why

Issue de entrada: [#60](https://github.com/atareao/kesto/issues/60). Plan: `plans/PLAN-016.md`.

El panel se sirve hoy con el **tema claro por defecto de Ant Design** y **sin identidad de marca**. `ui/src/estilos.css` fija colores a mano (`#ffffff`, `#f0f0f0`, `#1677ff`), `Layout` monta el lateral con `theme="light"` y la marca es solo el texto «Kesto». `ui/index.html` no declara ningún favicon ni manifest, así que la pestaña muestra el icono genérico del navegador. El usuario pidió una mejora visual: **assets de marca** (favicons y logos), **tema claro/oscuro con oscuro por defecto** y un **pulido** coherente en ambos temas.

## What Changes

- **Assets de marca servidos por el panel.** Los favicons (`favicon.ico`, pngs 16/32/48/64, `apple-touch-icon`, `android-chrome-192/512`, `maskable-icon-512`, `mstile-150x150`), el `site.webmanifest` y la imagen Open Graph se sirven desde `ui/public/`, y `ui/index.html` declara `<link rel="icon">`, el manifest, `theme-color` y las meta de Open Graph.
- **Paleta de marca en los tokens de Antd.** Primario cyan `#2194AA`; apoyo navy `#123048`, teal `#33998D`, verde `#7EBC50`, naranja `#F08724`; fondos `#F8FAFC` (claro) y `#0D1B2A` (oscuro). Se acabaron los colores hardcodeados de `estilos.css`.
- **Tema claro/oscuro con oscuro por defecto.** Un `ConfigProvider` global aplica el algoritmo de Antd según el modo; el panel arranca **siempre oscuro** salvo que haya una elección guardada. Un **toggle** en el layout cambia de tema y la elección persiste en `localStorage`.
- **Marca en el layout.** Logo en el menú lateral (variante clara/oscura según el modo), cabecera y estados que siguen el tema.
- **Pulido de páginas.** Espaciados y tarjetas del Dashboard coherentes en ambos temas.

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `ui`: nuevos requisitos — el panel declara su identidad de marca y ofrece tema claro/oscuro con oscuro por defecto y elección persistida.

## Impact

- **Ficheros frontend:** `ui/index.html`, `ui/public/` (nuevo), `ui/src/main.tsx`, `ui/src/estilos.css`, `ui/src/components/Layout/Layout.tsx`, un módulo de tema nuevo en `ui/src/services/`, un componente de toggle nuevo, y retoques de espaciado en las pantallas.
- **Sin cambios** de contrato de API, esquema, backend ni variables de entorno.
- **Docs:** `docs/FEATURE-016-identidad-visual-y-tema.md`.
