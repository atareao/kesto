# Proposal

## Why

Al cambiar de página, los tres listados paginados del panel —Histórico (`useEjecuciones`), Snapshots (`useSnapshots`) y Restauraciones (`useRestauraciones`)— **parpadean**: la tabla desaparece y vuelve a aparecer. React Query trata cada página como una consulta distinta (el `offset` forma parte de la clave), así que al cambiar de página la nueva consulta nace en `pending`, la pantalla pinta su estado «cargando» y la tabla se desmonta hasta que llega la respuesta (issue #146). Quedó anotado como follow-up en `docs/FEATURE-033-paginacion-de-listados.md`.

## What Changes

- Los tres hooks ganan `placeholderData: keepPreviousData` de React Query (`@tanstack/react-query`). Con él, mientras llega la página siguiente, `data` conserva la página anterior y `isPlaceholderData` es `true`; `isPending` se queda en `false`, así que las pantallas **no** entran en su rama de «cargando» y la tabla no se desmonta. La transición muestra la página anterior hasta que la nueva está lista, sin hueco ni parpadeo.
- El import de `keepPreviousData` se añade a los tres hooks.
- Pruebas: en `ui/src/hooks/useEjecuciones.test.tsx`, `useSnapshots.test.tsx` y (nuevo) `useRestauraciones.test.tsx`, un caso por hook que cambia de página con la segunda petición **pendiente** y afirma que `data` conserva la página anterior, `isPlaceholderData` es `true` e `isPending` es `false`.

## Non-goals

- NO se toca la API, las claves de caché (`claves.*`) ni el contrato de `limit`/`offset`.
- NO se añade paginación a listados que no la tienen (targets, repositorios, tareas): solo los tres paginados.
- NO se rediseña la vista ni se añade un indicador de carga nuevo: basta con no desmontar la tabla. El `showTotal` del paginador sigue leyendo el `total` de la respuesta, que durante la transición es el de la página anterior (coherente con las filas que se ven).
- NO se cambia la política de reintento ni el refresco de restauraciones.

## Capabilities

### New Capabilities

_(ninguna)_

### Modified Capabilities

- `ui`: los tres listados paginados conservan la página anterior mientras llega la siguiente, para no parpadear al paginar.
