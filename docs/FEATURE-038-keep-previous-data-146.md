# FEATURE-038 — Conservar la página anterior al paginar (`keepPreviousData` en el panel) (#146)

- **Issue:** #146 · **Change OpenSpec:** `keep-previous-data-146` · **Plan:** `plans/PLAN-038.md`.
- **Tipo:** panel (React + TypeScript). No toca la API, las claves de caché ni la política de reintento.

## Qué es

Al cambiar de página en las tres pantallas paginadas del panel —**Histórico** (`useEjecuciones`), **Snapshots** (`useSnapshots`) y **Restauraciones** (`useRestauraciones`)— la tabla ya no parpadea: conserva la página anterior mientras llega la nueva.

## Causa / motivo

React Query trata cada página como una **consulta distinta** (el `offset` forma parte de la clave). Al cambiar de página, la consulta nueva nace en `pending`, la pantalla pinta su rama de «cargando» y la tabla se desmonta hasta que llega la respuesta: el parpadeo. Quedó anotado como follow-up en `docs/FEATURE-033-paginacion-de-listados.md`.

## Ficheros implicados

- `ui/src/hooks/useEjecuciones.ts`, `ui/src/hooks/useSnapshots.ts`, `ui/src/hooks/useRestauraciones.ts`: `placeholderData: keepPreviousData` + import.
- `ui/src/hooks/useRestauraciones.test.tsx` (**nuevo**), `useEjecuciones.test.tsx`, `useSnapshots.test.tsx`: un caso por hook.
- `ui/src/pages/restauraciones.test.tsx`: matcher del botón «Actualizar» tolerante al icono de carga (ver Decisiones).
- `openspec/changes/keep-previous-data-146/` · `docs/FEATURE-038-keep-previous-data-146.md` · `plans/PLAN-038.md`.

## Cómo funciona

- Los tres `useQuery` ganan `placeholderData: keepPreviousData` de `@tanstack/react-query` (^5.107.1, ya instalado).
- Mientras llega la página nueva: `data` **conserva** la página anterior, `isPlaceholderData` es `true` e `isPending` se queda en `false`. Como las pantallas gatean el «cargando» con `isPending`, no entran en esa rama y la tabla **no se desmonta**: la transición muestra la página anterior hasta que la nueva está lista.
- La clave de la consulta (con su `offset`) **no cambia**: cada página sigue siendo su propia entrada de caché y se pide al servidor.
- El `total` que alimenta el paginador sigue leyéndose de la respuesta; durante la transición es el de la página anterior, coherente con las filas que todavía se ven.

## Cómo verificar

```sh
cd ui && pnpm test -- --run          # 325 tests, incluidos los tres nuevos
just ui-check                        # typecheck app + typecheck tests + lint + test + build
```

## Decisiones

- **`placeholderData` y no caché**: no se precargan páginas ni se cambia el `offset` de la clave; solo se evita el desmontaje durante la transición.
- **Tipo explícito en `useSnapshots`** (`useQuery<ListadoDeSnapshots>`): el `queryFn` de ese hook tiene una rama que rechaza (`Promise<never>`) y, con `placeholderData` en juego, la inferencia del genérico de datos se pierde (`data` quedaría tipado como la propia función del placeholder). El genérico explícito lo fija. Es obligatorio para que `tsc` pase; no cambia el comportamiento.
- **Matcher del botón «Actualizar»** (`/Actualizar/` en `restauraciones.test.tsx`): al conservar la página anterior, un cambio de filtro ya no pasa por `isPending` y el refresco deja `isRefetching: true`; Antd añade entonces el icono con nombre accesible «loading» y el nombre accesible del botón pasa a «loading Actualizar». Es un artefacto de jsdom/Antd (el nodo del icono sobrevive al volver a `loading: false`). Se prefirió tolerar el prefijo en el matcher antes que poner `aria-label="Actualizar"`, que eliminaría la única señal accesible de «cargando» del botón (Antd no emite `aria-busy`). El test sigue afirmando lo mismo.

## Follow-ups

- Ninguno. Los tres listados paginados quedan alineados.
