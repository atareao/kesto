# PLAN-019 — Ajustes de sidebar (marca, plegado y tema)

**Issue:** #66 — 💄 style(ui): ajustes de sidebar (marca, plegado y tema)
**Base:** `development` tras la release v0.7.0
**Rama:** `feature/ajustes-de-sidebar` (desde `development`)
**Change SDD:** `openspec/changes/ajustes-de-sidebar/`
**Estado:** Completado — implementado y verificado (TDD RED→GREEN, `just ui-check` verde), auditoría de UI sin bloqueantes; PR a `development`

## 1. Objetivo

Corregir tres detalles de presentación del layout que hoy estorban al operador:

1. Al plegar la barra lateral, el texto «Kesto» **sigue renderizándose** y solo lo recorta el CSS (`overflow: hidden` en `.marca-del-panel`, `estilos.css:64-71`): no desaparece de verdad.
2. El **control de plegado** del menú vive en la cabecera (`Layout.tsx:124-131`), no en la barra lateral que gobierna.
3. El **conmutador de tema** vive también en la cabecera (`Layout.tsx:132`).

El resultado esperado: los dos controles al pie de la barra lateral —junto al menú que controlan— y la marca oculta de verdad al plegar, conservando la accesibilidad.

## 2. Contexto

- Todo el layout es JSX inline en `ui/src/components/Layout/Layout.tsx`; no hay subcomponentes (`Cabecera`, etc.).
- Estado de plegado: `const [plegado, setPlegado] = useState(false)` (`Layout.tsx:58`). La `Sider` es `collapsible`, `collapsed={plegado}`, `trigger={null}` (el trigger propio de Antd está desactivado), `breakpoint="lg"` y `onBreakpoint={setPlegado}` (`:89-98`).
- Marca: `div.marca-del-panel` con `img.logo-del-panel` (`alt=""`, decorativo, `data-testid="logo-del-panel"`) y el `Typography.Text strong` con el literal «Kesto» (`:99-104`).
- Cabecera: `LayoutDeAntd.Header.cabecera-del-panel` contiene **solo** el botón de plegar y `<ConmutadorDeTema className="conmutador-de-tema" />` (`:117-133`). Al mover ambos, la cabecera queda vacía.
- CSS implicado: `.marca-del-panel` (L64-71), `.cabecera-del-panel` (L81-86) y `.conmutador-de-tema { margin-inline-start: auto }` (L89-91).
- `ConmutyadorDeTema` es un `Switch` de Antd con `aria-label` dinámico («Cambiar a tema claro/oscuro»); su única prop pública es `className`. No depende de la cabecera.
- Tests: `Layout.test.tsx:67-71` exige el switch **dentro** de `.cabecera-del-panel` (se romperá al moverlo); `App.test.tsx:74` usa `getByText('Kesto')` con el layout desplegado.

## 3. Decisiones de diseño

- **D1 — La marca se oculta por renderizado condicional, no por CSS.** El nombre se monta solo cuando la barra lateral está desplegada (`{!plegado && …}`); el logo permanece siempre. Ocultar con CSS no sirve: `overflow: hidden` ya está y no retira el texto del árbol. Consecuencia asumida: con la barra plegada el nombre desaparece del árbol accesible; el logo es decorativo (`alt=""`) y el menú lleva su propio `aria-label`, así que no se pierde información esencial.
- **D2 — Los dos controles van a un pie dentro de la `Sider`, a continuación del menú.** Se añade un contenedor (`div.pie-del-panel`) tras `nav#menu-principal`, dentro de la `Sider`, pegado al fondo. Se conservan íntegros `aria-label` («Plegar el menú» / «Desplegar el menú»), `aria-expanded` y `aria-controls="menu-principal"` del botón, y el nombre accesible del conmutador. Los tests existentes dependen de esos nombres y deben seguir pasando.
- **D3 — La cabecera vacía se retira.** `LayoutDeAntd.Header` no aporta nada una vez movidos los controles; se elimina y el `Content` pasa a ocupar todo el alto. Se retiran `.cabecera-del-panel` y el `margin-inline-start: auto` del conmutador (pensado para el flex de la cabecera).
- **D4 — Pie adaptable al plegado.** Con la barra plegada la `Sider` mide 80 px: el pie pasa a columna centrada para que el botón y el interruptor no desborden; desplegado, van en fila con `justify-content: space-between`.

## 4. Alcance

- `ui/src/components/Layout/Layout.tsx`: renderizado condicional de la marca; pie con los dos controles; retirada de la cabecera.
- `ui/src/estilos.css`: regla del pie, retirada de `.cabecera-del-panel` y del `margin-inline-start: auto`.
- `ui/src/components/Layout/Layout.test.tsx`: reescribir las aserciones que situaban el switch en la cabecera; añadir el caso de la marca oculta al plegar.
- Comentario menor en `ConmutadorDeTema.tsx`: retirar la prop `className`, que ya no pasa nadie (la colocación la da `.pie-del-panel`); el componente queda sin props.
- `Layout.test.tsx`: el barrido con Tab se acota con el ciclo real de tabulación (no con un número de pasos a ojo) y el ancho de ventana se guarda y se restaura por prueba.

### Fuera de alcance

- Rediseño del menú de navegación o de las pantallas.
- Persistir la preferencia de plegado (sigue empezando desplegada).
- Cambios de backend, esquema o API.

## 5. Escenarios (contrato SDD, `specs/ui/spec.md`)

- **Los controles viven en el pie de la barra lateral:** con el layout desplegado, el botón de plegar y el conmutador de tema están dentro de la `Sider`, a continuación del menú, y son alcanzables con el teclado.
- **La marca se oculta al plegar:** con la barra plegada, «Kesto» no se renderiza y el logo sigue visible.
- **La marca sigue visible desplegada:** con la barra desplegada, «Kesto» se renderiza.
- Se conservan los escenarios ya existentes del layout, la marca y el tema.

## 6. Tareas

Ver `openspec/changes/ajustes-de-sidebar/tasks.md`. Resumen del ciclo:

1. **RED:** tests que fallen — marca oculta al plegar; controles en el pie de la barra lateral y no en una cabecera; marca visible desplegada.
2. **GREEN:** renderizado condicional de la marca; pie con botón y conmutador; retirada de la cabecera y ajustes de CSS.
3. **REFACTOR:** `just ui-check` en verde (typecheck app + tests, lint, test, build); revisar 80 px plegado y ventana estrecha.
4. **Cierre:** archivar el change, documentar la feature y abrir PR.

## 7. Criterios de aceptación

- [x] Con la barra lateral colapsada no se ve (ni se renderiza) el texto «Kesto».
- [x] Plegar y tema están al pie de la barra lateral y son accesibles por teclado.
- [x] `just ui-check` en verde.
- [x] El contrato se actualiza en `openspec/specs/ui/spec.md` y la feature queda documentada en `docs/FEATURE-019-ajustes-de-sidebar.md`.

## 8. Referencias

- Issue #66
- `ui/src/components/Layout/Layout.tsx`, `ui/src/estilos.css`, `ui/src/components/ConmutadorDeTema/ConmutadorDeTema.tsx`
- `openspec/specs/ui/spec.md` (requisitos de layout, marca y tema)