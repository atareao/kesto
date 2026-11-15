# FEATURE-019 — Ajustes de sidebar (marca, plegado y tema)

- **Issue:** [#66](https://github.com/atareao/kesto/issues/66).
- **Plan:** `plans/PLAN-019.md` (renombrado a `plans/PLAN-019-COMPLETED.md` al cerrar).
- **Change SDD:** `openspec/changes/archive/2026-11-15-ajustes-de-sidebar/` (archivado).

## Qué es

Tres ajustes de presentación del armazón del panel: el nombre «Kesto» deja de renderizarse cuando la barra lateral está plegada (antes seguía en el DOM y solo lo recortaba el CSS); y el botón de plegar y el conmutador de tema dejan la cabecera y pasan al pie de la propia barra lateral, junto al menú que gobiernan. La cabecera, al quedarse sin contenido, se retira.

## Ficheros implicados

- `ui/src/components/Layout/Layout.tsx` — marca condicional (`!plegado ? … : null`); nuevo `div.pie-del-panel` dentro de la `Sider`, tras `nav#menu-principal`, con `<ConmutadorDeTema />` y el botón de plegar; retirada del `LayoutDeAntd.Header`.
- `ui/src/components/Layout/Layout.test.tsx` — aserciones reescritas (el conmutador ya no está en `.cabecera-del-panel`) y nuevos casos: pie de la barra lateral, marca oculta al plegar, marca visible desplegada y alcance por teclado.
- `ui/src/estilos.css` — retiradas `.cabecera-del-panel` y `.conmutador-de-tema`; añadidas `.ant-layout-sider-children` (columna flex), `.pie-del-panel` (fila, pegado al fondo con `margin-top: auto`) y `.ant-layout-sider-collapsed .pie-del-panel` (columna centrada).
- `ui/src/components/ConmutadorDeTema/ConmutadorDeTema.tsx` — se retira la prop `className`, que quedó sin uso.

## Cómo funciona

- **Marca oculta al plegar.** `plegado` es el estado del `Sider`. El nombre se monta con `{!plegado ? (<Typography.Text strong>Kesto</Typography.Text>) : null}`; el `img.logo-del-panel` permanece siempre. Ocultar con CSS no bastaba: `overflow: hidden` recortaba el texto pero lo dejaba en el DOM y en el árbol accesible.
- **Pie de la barra lateral.** Un contenedor `.pie-del-panel` cierra la `Sider`, después del `nav`. Antd mide la `Sider` a `100vh`, así que sus hijos van en columna (`display: flex; flex-direction: column`) y el pie reclama el hueco que sobra (`margin-top: auto`), quedando pegado al fondo. El `Switch` va a la izquierda y el botón de plegar a la derecha (`justify-content: space-between`).
- **Plegada.** La `Sider` mide 80 px: `.ant-layout-sider-collapsed .pie-del-panel` pasa a columna centrada, de modo que el `Switch` y el botón no desbordan.
- **Accesibilidad.** El botón conserva `aria-label` («Plegar el menú» / «Desplegar el menú»), `aria-expanded` y `aria-controls="menu-principal"`, y el `Switch` su nombre accesible dinámico. Ambos son alcanzables y activables con el teclado; el orden de tabulación del pie es conmutador → botón.

## Variables / configuración

Ninguna. No hay flags ni variables de entorno; es presentación del panel.

## Cómo habilitar / desplegar

Sin pasos: va en el bundle del panel al desplegar. `just ui-check` (typecheck de aplicación y tests, lint, test, build) debe estar en verde.

## Decisiones

- **Ocultar por renderizado condicional, no por CSS** (D1 del plan): `overflow: hidden` no retira el texto del DOM. Asumido: con la barra plegada el nombre desaparece del árbol accesible; el logo es decorativo (`alt=""`) y el menú lleva su propio `aria-label`, así que no se pierde información esencial.
- **Retirar la cabecera** (D3): al mover los dos controles, el `Header` quedaba vacío; se elimina y el `Content` ocupa todo el alto. Se pierde el landmark `banner`, que no aportaba contenido de cabecera real; la navegación (`nav[name=Principal]`) y el `main` siguen.

## Follow-ups

- **Acoplamiento a Antd.** La regla `.ant-layout-sider-children` depende del DOM interno de Ant Design; un *major* podría cambiarlo. Queda documentado en el comentario del CSS.
- **Plegado en ventana estrecha.** `onBreakpoint={setPlegado}` sobrescribe la elección manual al redimensionar (comportamiento preexistente, no introducido por esta feature).
- **Sin persistencia de plegado.** La barra sigue empezando desplegada; no se guarda la preferencia (fuera de alcance).
- **Landmark del pie.** Si se quisiera rigor semántico, el pie podría envolverse en un `<footer>`/`role="contentinfo"` dentro del `aside`; hoy no es exigible.
