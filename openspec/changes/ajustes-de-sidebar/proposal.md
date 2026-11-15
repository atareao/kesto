# Proposal — ajustes-de-sidebar

## Why

Tres detalles de presentación del layout estorban al operador: al plegar la barra lateral el texto «Kesto» sigue renderizándose (solo lo recorta el CSS), y los dos controles que gobiernan el panel —plegar el menú y alternar el tema— viven en la cabecera, lejos del menú que controlan. Los controles de la barra lateral deben estar en la barra lateral.

## What Changes

- Ocultar el texto «Kesto» cuando la barra lateral está plegada: el nombre deja de renderizarse; el logo permanece.
- Mover el botón de plegar de la cabecera al pie de la barra lateral, conservando `aria-label`, `aria-expanded` y `aria-controls` y la accesibilidad por teclado.
- Mover el conmutador de tema de la cabecera al pie de la barra lateral, conservando su nombre accesible y su comportamiento.
- Retirar la cabecera (`Header`) que queda vacía y ajustar `estilos.css`: nueva regla del pie, retirada de `.cabecera-del-panel` y del `margin-inline-start: auto` del conmutador.
- Actualizar los tests de `Layout.test.tsx` que asumían que el conmutador estaba en la cabecera, y añadir el caso de la marca oculta al plegar.

## Capabilities

**Modified Capabilities**: `ui` — cambian a nivel de requisito el layout (dónde vive el control de plegado), la identidad de marca (cuándo se ve el nombre) y la ubicación del control de tema.

## Fuera de alcance

- Rediseño del menú de navegación o de las pantallas.
- Persistir la preferencia de plegado: la barra lateral sigue empezando desplegada.
- Cambios de backend, esquema o API.

## Impacto

- Frontend: `ui/src/components/Layout/Layout.tsx`, `ui/src/estilos.css`, `ui/src/components/Layout/Layout.test.tsx`; `ui/src/components/ConmutadorDeTema/ConmutadorDeTema.tsx` (se retira la prop `className`, ya sin consumidores).
- Tests: el barrido con Tab de `Layout.test.tsx` se acota con el ciclo real de tabulación en vez de un límite de pasos a ojo, y el ancho de ventana se guarda y se restaura por prueba.
- Sin cambios de esquema, API ni backend.

## Referencias

- Issue #66
- `plans/PLAN-019.md`
- `ui/src/components/Layout/Layout.tsx`, `ui/src/estilos.css`
