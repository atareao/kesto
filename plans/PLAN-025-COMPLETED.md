---
# PLAN-025 — Renders y resiliencia del panel

**Estado:** en curso
**Change OpenSpec:** `renders-y-resiliencia-del-panel`
**Issues:** #99, #100, #101
**Feature doc de cierre:** `docs/FEATURE-025-renders-y-resiliencia-del-panel.md`

## Contexto

Tras desplegar la versión 0.10.0 y observar el panel con el repositorio S3 no disponible, se reportaron varios síntomas. El triaje separó lo que era entorno de laboratorio (S3 `rustfs-lab` sin DNS, contenedores fuente apagados) de tres defectos reales de producto, que son los que este plan aborda:

- **A (#99)** — La columna «Tarea» del Dashboard pinta el nombre del *target*, no el de la tarea.
- **B (#100)** — Si el S3 no responde, el panel se queda «cargando» minutos (backend ~37 s + `retry: 3` por defecto) y, con varios repositorios, el fallo de uno borra la información de los sanos.
- **C (#101)** — «Ver consola» de una ejecución terminada no muestra nada (solo pinta SSE en vivo).

## Objetivos

1. La columna «Tarea» del Dashboard muestra `tarea.name` (nunca el nombre del target).
2. Un listado contra un S3 que no responde falla **rápido** (tope de 10 s) con error tipado y legible.
3. El panel acota su espera y su reintento, y ofrece estado de **error con «Reintentar»** en vez de un «cargando» sin fin.
4. El resumen del Dashboard **no es todo-o-nada**: con un repositorio caído conserva la información de los sanos y avisa **nombrando** el repositorio que falla.
5. La cifra de espacio se declara como **mínimo** («al menos») cuando algún repositorio no se pudo leer.
6. La consola de una ejecución **terminada** muestra un resumen útil (`error_message` cuando `failed`) en vez de un hueco.

## Decisiones aprobadas

- **D2/B1** — Tope de tiempo del cliente S3: **10 segundos**.
- **D12/B6** — La visibilidad de salud por repositorio, en su forma **completa** (campo/estado + endpoint + indicador en Ajustes, tocando `repository`/`api-resources`), se difiere a un **change aparte**. Este plan implementa solo la forma **mínima**: atribuir el fallo por nombre desde el intento de listado, sin añadir campos a `Repository` ni rutas nuevas.
- **Parte C** — Alcance: mostrar el resumen (`error_message`/texto claro). **Persistir y servir el log completo queda fuera de alcance.**

## Tareas

### Preparación
- [ ] 0.1 Punto de partida en verde: `just check` y `just ui-check`.
- [ ] 0.2 D2 aprobada con valor concreto (10 s) — hecho por el operador.

### Parte A — columna «Tarea» del Dashboard (#99)
- [ ] A RED: test de `TablaDeTareasEnCurso` que exige el nombre de la tarea y no el del target; y el borde `null`/blanco → «sin nombre».
- [ ] A GREEN: `TablaDeTareasEnCurso.tsx` pinta `tarea.name` con red de seguridad atenuada; retirar `nombresDeTarget` y `formatearUuid(tarea.target_id)` de esa celda.
- [ ] A REFACTOR: extraer el marcador atenuado si queda duplicado con `tareas.tsx`.

### Parte B — resiliencia ante un S3 que no responde (#100)
- [ ] B RED: test Rust de operación contra destino que no responde → `Error::DestinoNoAlcanzable` en tiempo acotado (muy por debajo de 36,9 s).
- [ ] B RED: `TarjetasDeResumen` con fallo → estado de error con «Reintentar», no `Skeleton` perpetuo.
- [ ] B RED: `snapshots.tsx` con fallo → `EstadoDeError` con «Reintentar», no `<Cargando/>` indefinido.
- [ ] B RED: `retry` acotado por consulta en `useEspacioEnS3`/`useSnapshots`; la política global del `QueryClient` no cambia.
- [ ] B GREEN: `src/rustic/repositorio.rs` (`opciones_de_s3`) informa el tope de 10 s en las opciones de `opendal`.
- [ ] B GREEN: `retry` acotado en los hooks que dependen del S3, sin tocar `clienteDeConsultas.ts`.
- [ ] B GREEN: `TarjetasDeResumen` y `snapshots.tsx` muestran error con «Reintentar».
- [ ] B (B4) RED/GREEN: con dos repositorios y uno caído, el Dashboard nombra el caído y conserva la info del sano; con todos sanos, sin aviso.
- [ ] B (B5) GREEN: con varios repos y uno ilegible, la tarjeta de espacio se declara «al menos» (mínimo), reutilizando `truncated`.
- [ ] B (B6) Decidido: mínima en este change; la completa se abre como change aparte (no añadir campos de salud aquí).
- [ ] B REFACTOR: sin `unwrap`/`expect` nuevos en el camino S3; una sola constante para el `retry`.

### Parte C — consola de una ejecución terminada (#101)
- [ ] C RED: ejecución `failed` con `error_message` → la consola lo muestra; ejecución `success` sin líneas → texto claro.
- [ ] C GREEN: `ConsolaDeEjecucion.tsx` pinta el resumen si no hay líneas y la ejecución está terminada; `PropsDeConsolaDeEjecucion` recibe `status` y `error_message`.
- [ ] C: no tocar el historial en vivo de `useEventStream.ts` (sin replay ni persistencia).
- [ ] C REFACTOR: reutilizar `Empty`/`Typography.Text` existentes.

### Verificación y cierre
- [ ] V1 `just check-all` en verde (fmt, clippy, test, typecheck, tests UI, lint, build).
- [ ] V2 Fusionar deltas con `just spec-archive renders-y-resiliencia-del-panel`; `openspec validate --all --strict` en verde.
- [ ] V3 `docs/FEATURE-025-renders-y-resiliencia-del-panel.md`.
- [ ] V4 Renombrar a `plans/PLAN-025-COMPLETED.md` con `git mv` y abrir PR a `development` con `Closes #99, #100, #101`.

## Verificación

Cada parte se cierra con su test en verde y con `just check-all` antes del commit. La parte B, además, con una prueba manual: con el S3 caído, `/api/v1/snapshots` debe fallar en ~10 s (no 36,9 s) y la vista Snapshots mostrar el error con «Reintentar».

## Fuera de alcance

- #88 (columna «Target» del Histórico), #90, #91, #92, #94.
- Persistir y servir el log completo de una ejecución.
- Visibilidad de salud por repositorio en su forma completa (change aparte).
---