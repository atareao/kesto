# Design

## Context

Ver `proposal.md` — Why. Lo que condiciona el enfoque:

- `ui/src/pages/restauraciones.tsx` ya tiene un `Select` de estado con `allowClear` y una paginación por `limit`/`offset` que reinicia `offset` al cambiar el filtro de estado.
- `ui/src/api/restauraciones.ts` ya declara `FiltrosDeRestauraciones { status?, target_id?, limit?, offset? }` y ya los envía en la consulta; `ui/src/hooks/useRestauraciones.ts` ya incluye los filtros en la `queryKey` y ya refresca mientras haya filas abiertas.
- El catálogo de targets ya se consume en el panel: `ui/src/hooks/useTargets.ts` sobre `GET /api/v1/targets`.
- El patrón de referencia es el **Histórico** (`ui/src/pages/historico.tsx`): un `Select` de filtro cuyas opciones salen de un catálogo (`useTareas`) y un `Select` de estado, ambos reiniciando `offset`.

## Goals / Non-Goals

**Goals**

- Añadir el selector de target a la pantalla de Restauraciones sin tocar el contrato de la API.

**Non-Goals**

- No se toca el backend, ni el esquema, ni el transporte del parámetro: ya cumplen.
- No se añade filtro por rango de fechas, ni por snapshot.
- No se cambia la columna Target ni el formato del vacío.

## Decisions

**D1 — Reutilizar `useTargets()` en vez de derivar las opciones de las filas ya listadas.**
Las opciones del selector salen del catálogo completo (`GET /api/v1/targets`), no de los targets presentes en la página actual. Derivarlas de la página haría que un target sin restauraciones en esa página desapareciera del selector, y que el propio selector cambiara de opciones al paginar. El coste es una petición extra de catálogo, ya cacheada por la librería de consultas, y ya pagada por la pantalla de Ajustes y por el Histórico. Alternativa descartada: `Select` de texto libre con el identificador; obliga al operador a conocer un UUID.

**D2 — `target_id` es un valor del selector, no el nombre del target.**
El selector muestra el nombre del target —lo que el operador reconoce— y envía su identificador, que es lo que la API valida como UUID. Se mantiene el criterio ya usado por el filtro de tarea del Histórico: la etiqueta es legible, el valor es el identificador.

**D3 — Un `offset` heredado se reinicia al cambiar el filtro.**
Es la misma guarda que ya aplica el filtro de estado y el de tarea en el Histórico: sin reiniciar, elegir un target con menos restauraciones que la página en curso devolvería una página vacía sobre un listado que sí tiene filas, y el operador leería «no hay nada» donde sí lo hay.

**D4 — Un target ausente del catálogo no rompe la pantalla.**
Si el `target_id` pedido ya no existe, el selector no tendrá su opción; la pantalla se limita a pintar lo que la API devuelva (lista vacía o filas con el `target_name` ya resuelto en el histórico) y su estado vacío. No se añade lógica de reconciliación de opciones.

## Risks / Trade-offs

- [El selector puede quedar «sin valor visible» si la opción elegida desaparece del catálogo] → la consulta se sigue enviando con su `target_id` y la pantalla cae a su estado vacío; el operador puede limpiar el filtro. No se implementa reconciliación (D4).
- [Una petición extra de catálogo al abrir la pantalla] → ya está cacheada por la librería de consultas y la comparten Ajustes e Histórico; el coste no es nuevo.
- [El selector y el filtro de estado podrían pisarse al reiniciar la paginación] → ambos reinician `offset` por el mismo camino, y hay un escenario que fija la combinación de los dos.

## Migration Plan

No hay migración: cambio aditivo de interfaz sobre una ruta ya existente. La vuelta atrás es retirar el selector; el backend no se ve afectado.

## Open Questions

Ninguna.
