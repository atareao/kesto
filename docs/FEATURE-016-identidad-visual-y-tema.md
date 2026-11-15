# FEATURE-016 — Identidad visual y tema claro/oscuro

- **Issue:** [#60](https://github.com/atareao/kesto/issues/60).
- **Plan:** `plans/PLAN-016.md`.
- **Change SDD:** `openspec/changes/archive/2026-11-14-identidad-visual-y-tema/` (archivado).

## Qué es

Da al panel de Kesto su **identidad de marca** (favicons, manifiesto, imagen Open Graph y logos) y un **tema claro/oscuro con oscuro por defecto** y un **conmutador** que recuerda la elección. Antes el panel salía con el tema claro por defecto de Ant Design, sin favicon, y con los colores fijados a mano en `estilos.css`.

## Ficheros implicados

- **Assets**
  - `ui/public/` — `favicon.ico`, `favicon-16/32/48/64.png`, `apple-touch-icon.png`, `android-chrome-192/512.png`, `maskable-icon-512x512.png`, `mstile-150x150.png`, `og-image-dark.png` y `site.webmanifest`. Vite los sirve en la raíz.
  - `ui/src/assets/` — `kesto-icon.png` y `kesto-icon-dark.png` (logo del lateral, variante según modo), importados por el bundle.
  - `ui/index.html` — `<link rel="icon">`, `<link rel="manifest">`, `msapplication-*`, `<meta name="theme-color">`, meta Open Graph y un **script previo al pintado** que fija `data-tema` (oscuro o la elección guardada) para evitar el destello.
- **Tema**
  - `ui/src/services/useTema.ts` — núcleo: `ModoDeTema`, `CLAVE_DE_TEMA` (`kesto-tema`), `leerTemaGuardado()` (oscuro por defecto, ignora valores inválidos), `ContextoDelTema`, `useTema()` (lanza fuera del proveedor) y `temaDeAntd(modo)` (algoritmo + tokens de marca por modo, incluidos los *component tokens* de `Menu` y `Button`).
  - `ui/src/services/tema.tsx` — `ProveedorDeTema`: contexto `{modo, alternar}`, persiste en `localStorage`, sincroniza `document.documentElement.dataset.tema` y el `<meta name="theme-color">`.
- **UI**
  - `ui/src/components/PanelConTema/PanelConTema.tsx` — lee `useTema()` y monta `ConfigProvider` → `QueryClientProvider` → `ProveedorDeEventos` → `BrowserRouter` → `App`.
  - `ui/src/main.tsx` — solo arranque (`#root` → `StrictMode` → `ProveedorDeTema` → `PanelConTema`).
  - `ui/src/components/ConmutadorDeTema/ConmutadorDeTema.tsx` — `Switch` accesible; el nombre accesible anuncia el tema **destino**.
  - `ui/src/components/Layout/Layout.tsx` — sin `theme="light"` ni colores a mano; cabecera y lateral desde `theme.useToken()`; logo según modo; conmutador en la cabecera.
  - `ui/src/estilos.css` — variables CSS + `:root[data-tema]`; fuera los colores fijos.
  - `ui/src/components/ConsolaDeEjecucion/ConsolaDeEjecucion.tsx` — superficie «de terminal» derivada del tema (`colorBgSpotlight`/`colorTextLightSolid`).
  - `ui/src/pages/dashboard.tsx` — espaciados con `Flex vertical gap`.

## Cómo funciona

1. **Arranque.** El script de `index.html` fija `data-tema` antes de pintar (oscuro salvo `kesto-tema=claro`). Ya en React, `ProveedorDeTema` inicializa el modo con `leerTemaGuardado()` (oscuro por defecto) y `PanelConTema` monta el `ConfigProvider` con `temaDeAntd(modo)`.
2. **Tema.** `temaDeAntd` aplica `darkAlgorithm`/`defaultAlgorithm` y los tokens de marca: `colorPrimary #2194AA`; fondos `colorBgLayout #0D1B2A`/`#F8FAFC` y `colorBgContainer #123048`/`#FFFFFF`. El contraste de los textos de marca se resuelve con acentos por modo (`colorLink` `#5CC8DC` en oscuro, `#18788C` en claro) y *component tokens* del `Menu`, de modo que **todos los pares de texto cumplen WCAG AA (≥4.5:1)**; hay un test que lo comprueba desde los tokens.
3. **Conmutador.** `alternar()` cambia el modo, lo guarda en `localStorage` y el efecto del proveedor sincroniza `data-tema` y el `theme-color`. `estilos.css` ajusta el fondo del `body` por variable CSS.

## Variables / configuración

- Ninguna variable de entorno nueva. La única «configuración» es la clave de `localStorage` **`kesto-tema`** (`oscuro`/`claro`); su ausencia significa **oscuro**.

## Cómo habilitar / desplegar

Viene dentro del panel; no hay que activar nada. El panel se construye con `pnpm run build` en `ui/` y la imagen de producción lo copia a `/app/ui/dist` (Dockerfile, etapa 1). En el banco de pruebas se sirvió el panel nuevo montando `ui/dist` sobre `/app/ui/dist` del contenedor (el binario no cambia en esta feature).

## Decisiones

- **Oscuro por defecto y no seguir al sistema**: la spec lo pide así; el arranque solo respeta una elección **explícita**.
- **Logo = icono de marca** en el lateral (no el logotipo horizontal): el lateral de 220 px (y plegado a ~80 px) no da para el logotipo con tagline; el icono se lee en ambos estados. Se eliminaron los `.png` horizontales que quedaban sin uso.
- **Contraste sin tocar el primario**: se mantiene `colorPrimary #2194AA` (marca) y el contraste se resuelve con acentos y *component tokens* por modo, no cambiando el color base.
- **`useTema.ts` separado de `tema.tsx`**: `react-refresh/only-export-components` exige que un módulo de componente no exporte además funciones/contexto; el núcleo vive en un fichero sin componentes.

## Follow-ups

- **Cerrados con `refactor-tema-memoization`** (`plans/PLAN-017-COMPLETED.md`, issue #62): los antiguos **BAJO 8** (`useCallback`/`useMemo` redundantes en `tema.tsx`) y **BAJO 9** (`"types": ["node"]` en `ui/tsconfig.json`) se resuelven en ese refactor —se retiran las memoizaciones redundantes y los tipos de Node se acotan al tooling y a los tests, dejando el `tsconfig.json` de aplicación con `"types": []`—.
- El bundle del panel ronda los 1.3 MB (403 kB gzip) por Antd en un único chunk; `manualChunks`/code-splitting es una mejora futura.
