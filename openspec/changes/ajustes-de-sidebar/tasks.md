# Tasks — ajustes-de-sidebar

## 0. Preparación
- [ ] 0.1 `just ui-check` en verde antes de tocar código.
- [ ] 0.2 Fijar con tests que fallen los tres comportamientos: marca oculta al plegar, controles en el pie de la barra lateral.

## 1. RED
- [ ] 1.1 Test de UI: con la barra lateral plegada, «Kesto» no está en el documento.
- [ ] 1.2 Test de UI: con la barra lateral desplegada, «Kesto» sí se ve junto al logo.
- [ ] 1.3 Test de UI: el control de plegado está dentro de la barra lateral, a continuación del menú, con su nombre accesible y `aria-expanded`.
- [ ] 1.4 Test de UI: el conmutador de tema está dentro del pie de la barra lateral y es alcanzable por teclado.
- [ ] 1.5 Test de UI: no queda ninguna cabecera con los controles (la cabecera se retira).

## 2. GREEN
- [ ] 2.1 Renderizar el nombre «Kesto» de forma condicional al plegado (el logo permanece siempre).
- [ ] 2.2 Mover el botón de plegar al pie de la `Sider`, conservando `aria-label`/`aria-expanded`/`aria-controls`.
- [ ] 2.3 Mover el conmutador de tema al mismo pie.
- [ ] 2.4 Retirar la cabecera vacía (`Header`).
- [ ] 2.5 Ajustar `estilos.css`: regla del pie, retirada de `.cabecera-del-panel` y del `margin-inline-start: auto` del conmutador; pie en columna cuando la barra está plegada.

## 3. REFACTOR
- [ ] 3.1 `just ui-check` en verde (typecheck aplicación y tests, lint, test, build).
- [ ] 3.2 Revisar el comportamiento con la barra plegada (80 px) y en ventana estrecha (`breakpoint="lg"`).

## 4. Cierre
- [ ] 4.1 Archivar el change y fundir la delta en `openspec/specs/ui/spec.md` (`just spec-archive ajustes-de-sidebar`).
- [ ] 4.2 Documentar la feature en `docs/FEATURE-019-ajustes-de-sidebar.md`.
- [ ] 4.3 Commit y PR a `development`.
