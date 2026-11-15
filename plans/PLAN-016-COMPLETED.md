# PLAN-016 — FEATURE-016: identidad visual y tema claro/oscuro

- **Issue de entrada:** [#60](https://github.com/atareao/kesto/issues/60) — «Mejora visual: identidad de marca y tema claro/oscuro (oscuro por defecto)».
- **Change SDD:** `openspec/changes/identidad-visual-y-tema/` (`proposal.md`, `design.md`, `specs/ui/spec.md`, `tasks.md`), validado con `openspec validate identidad-visual-y-tema --strict`.
- **Rama:** `feature/identidad-visual-y-tema` (desde `development`).

## Objetivo

Mejorar el aspecto visual del panel (`ui/`): incorporar la identidad de marca (favicons, manifest, logos, paleta) y ofrecer **tema claro/oscuro con oscuro por defecto** y un **toggle** que persiste la elección. El panel hoy usa el tema claro por defecto de Ant Design, con colores hardcodeados en `estilos.css` y sin favicon ni manifest.

## Alcance

- **Assets de marca** desde `~/Descargas/kesto-web-assets/`: favicons y `site.webmanifest` a `ui/public/`; logos a `ui/src/assets/` (importados). `ui/index.html` declara iconos, manifest, `theme-color` y meta de Open Graph.
- **Paleta de marca en tokens de Antd**: primario cyan `#2194AA`; apoyo navy `#123048`, teal `#33998D`, verde `#7EBC50`, naranja `#F08724`; fondos `#F8FAFC` (claro) y `#0D1B2A` (oscuro).
- **Tema claro/oscuro**: módulo `ui/src/services/tema.tsx` (`ModoDeTema`, `leerTemaGuardado`, `ProveedorDeTema`/`useTema`, `ThemeConfig` de Antd), `ConfigProvider` global en `main.tsx`, persistencia en `localStorage` (`kesto-tema`), **oscuro por defecto**.
- **Marca y pulido**: logo en el menú lateral (variante según el modo), cabecera sin colores a mano, y espaciados/tarjetas del Dashboard coherentes en ambos temas.

**Fuera de alcance:** cambios de contrato de API, esquema, backend o variables de entorno.

## Tareas (TDD)

1. **Assets**: copiar favicons y manifest a `ui/public/`; logos a `ui/src/assets/`; cabeceras en `ui/index.html`.
2. **RED/GREEN del tema**: tests de `tema` (oscuro por defecto, respeta lo guardado, `alternar()` cambia y persiste) y del componente de toggle (nombre accesible, activable con teclado, anuncia su estado).
3. **RED/GREEN del cableado**: `main.tsx` monta `ProveedorDeTema` + `ConfigProvider`; el documento refleja el tema (`data-tema`).
4. **REFACTOR**: quitar los colores hardcodeados de `estilos.css`; cabecera y lateral desde los tokens.
5. **Marca y pulido**: logo en el lateral y retoques de espaciado del Dashboard.
6. **Docs**: `docs/FEATURE-016-identidad-visual-y-tema.md`.

Detalle completo y checklist en `openspec/changes/identidad-visual-y-tema/tasks.md`.

## Verificación

- `pnpm run lint`, `npx tsc --noEmit`, `pnpm test` en `ui/` → verde.
- `just check-all` → verde. `just check-spec` → verde con el change activo.
- Revisión con `@react-reviewer`.
- **Banco de pruebas**: construir la imagen local con el panel nuevo, recrear el contenedor `kesto` y comprobar en `http://127.0.0.1:8080` el favicon, el tema oscuro por defecto, el toggle a claro y su persistencia, y la legibilidad en ambos temas.

## Cierre

- `docs/FEATURE-016-identidad-visual-y-tema.md`.
- Archivar el change (`openspec archive identidad-visual-y-tema`) al pasar todas las verificaciones.
- Renombrar este plan a `plans/PLAN-016-COMPLETED.md`.
---