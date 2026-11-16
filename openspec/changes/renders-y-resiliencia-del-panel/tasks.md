# Tasks — renders-y-resiliencia-del-panel

> Tres partes independientes: A y C son de render (frontend puro), B toca las dos orillas —el
> transporte del backend y la política de espera del panel—. Ninguna cambia el contrato HTTP, así que
> el orden entre partes no importa y cada una puede entrar y cerrarse por su cuenta.
>
> La decisión D2 (B1) —el tope de tiempo del cliente S3 y su valor— está **aprobada** por el
> operador: el tope es de **10 s**. La decisión D12 (B6) —visibilidad de salud por repositorio—
> está **decidida**: este change implementa la forma mínima (atribuir el fallo por nombre desde el
> listado) y la forma completa (campo/estado de salud + endpoint + Ajustes) se abre como change
> aparte. Las partes A y C no dependen de ninguna de las dos.

## 0. Preparación

- [ ] 0.1 Punto de partida en verde: `just check` y `just ui-check` (verificación: los dos terminan sin error antes de tocar nada).
- [ ] 0.2 D2 (B1) está aprobada por el operador con valor concreto: el tope de tiempo del cliente S3 es de **10 s** (verificación: el proposal fija 10 s en D2 y la parte B queda desbloqueada).
- [ ] 0.3 Crear/actualizar `plans/PLAN-0NN.md` para las tres partes y referenciar los issues #99, #100 y #101 (verificación: el plan cita los tres números).

## 1. Parte A — la columna «Tarea» del Dashboard (#99)

### RED

- [ ] 1.1 UI: un test de `TablaDeTareasEnCurso` con una tarea de nombre `copia nocturna` sobre un target de nombre distinto exige que la celda «Tarea» muestre `copia nocturna`, **no** el nombre del target (verificación: `npx vitest run src/components/TablaDeTareasEnCurso` en rojo hoy —la celda pinta el target—).
- [ ] 1.2 UI: el test del borde en que `tarea.name` llega `null` o en blanco exige un marcador explícito y atenuado («sin nombre») y **nunca** el nombre del target (verificación: en rojo hoy).

### GREEN

- [ ] 1.3 `ui/src/components/TablaDeTareasEnCurso/TablaDeTareasEnCurso.tsx`: la fila deja de resolver `nombre` contra `nombresDeTarget` y la columna «Tarea» pinta `tarea.name` con la red de seguridad atenuada, como `ui/src/pages/tareas.tsx` (verificación: tests 1.1 y 1.2 en verde).
- [ ] 1.4 Retirar el uso del mapa `nombresDeTarget` para esa celda y el `formatearUuid(tarea.target_id)` que era su sustituto (verificación: `rg -n "nombresDeTarget|formatearUuid\(tarea.target_id\)" ui/src/components/TablaDeTareasEnCurso` sin resultados en la ruta de la celda).

### REFACTOR

- [ ] 1.5 Extraer el marcador atenuado a una constante o pequeño componente compartido con `tareas.tsx` si el código queda duplicado, sin cambiar comportamiento (verificación: `pnpm test` sigue en verde).

## 2. Parte B — resiliencia ante un S3 que no responde (#100)

### RED

- [ ] 2.1 Rust: con el tope impuesto por Kesto, una operación contra un destino que no responde devuelve `Error::DestinoNoAlcanzable` en un tiempo **acotado** —muy por debajo de los 36,9 s medidos— (verificación: `cargo test --locked` en rojo hoy; reutiliza el patrón de `src/rustic/repositorio.rs:2981` pero **con** el tope impuesto, no con `retry = 0`).
- [ ] 2.2 UI: con la consulta del espacio en S3 fallando, `TarjetasDeResumen` muestra un estado de error con «Reintentar» y **no** un `Skeleton` perpetuo (verificación: `npx vitest run src/components/TarjetasDeResumen` en rojo hoy).
- [ ] 2.3 UI: con el listado de snapshots fallando, `snapshots.tsx` muestra el `EstadoDeError` y **no** `<Cargando />` indefinido (verificación: en rojo hoy si el caso no está cubierto).
- [ ] 2.4 UI: `useEspacioEnS3` y `useSnapshots` declaran un `retry` acotado, y el `QueryClient` global **no** cambia su política de reintento (verificación: test que distingue la política de la consulta del S3 de la global; en rojo hoy).

### GREEN

- [ ] 2.5 `src/rustic/repositorio.rs` (`opciones_de_s3`) **e** la capa de Kesto que ejecuta la operación S3: informar un `retry` acotado en `opciones_de_s3` **e** imponer el tope de **10 s** (D2) alrededor de las operaciones que abren o listan el repositorio (verificación: test 2.1 en verde; el error sigue siendo `DestinoNoAlcanzable` —D7—).
- [ ] 2.6 `ui/src/hooks/useEspacioEnS3.ts` y el hook de snapshots: `retry` acotado (`false` o `1`) por consulta, sin tocar `ui/src/services/clienteDeConsultas.ts` (verificación: test 2.4 en verde).
- [ ] 2.7 `ui/src/components/TarjetasDeResumen/TarjetasDeResumen.tsx`: el error de `useEspacioEnS3` deja de quedar tapado por el `Skeleton`; con error, se pinta el `Alert` con «Reintentar» (verificación: test 2.2 en verde).
- [ ] 2.8 `ui/src/pages/snapshots.tsx`: el fallo del listado lleva al `EstadoDeError` con «Reintentar»; `<Cargando />` deja de ser indefinido (verificación: test 2.3 en verde).

### REFACTOR

- [ ] 2.9 Sin `unwrap`/`expect` nuevos fuera de tests en el camino de S3, y el motivo del error sigue sin ser el texto crudo de la dependencia (verificación: `cargo clippy --all-targets -- -D warnings` limpio y la aserción de no-filtración sigue en verde).
- [ ] 2.10 UI: confirmar que el `retry` por consulta se lee de una sola constante, no repetido literal en cada hook (verificación: `rg -n "retry" ui/src/hooks ui/src/services` acotado).

### Parte B (D10/B4) — el resumen del Dashboard no es todo-o-nada

- [ ] 2.11 RED: con dos repositorios, uno de ellos caído, el Dashboard SHALL mostrar el aviso **nombrando el repositorio caído** y SHALL conservar la información del sano —nº de repositorios y tareas activas—; NO SHALL sustituir las tres tarjetas por un `Alert` global (verificación: `npx vitest run src/components/TarjetasDeResumen` en rojo hoy —el error colapsa las tres—).
- [ ] 2.12 RED: con todos los repositorios sanos, NO SHALL aparecer ningún aviso de fallo (verificación: el test distingue «sin aviso» de «aviso vacío»; en rojo hoy si no está cubierto).
- [ ] 2.13 GREEN: `useEspacioEnS3` y `TarjetasDeResumen` dejan de colapsar el error; el fallo se modela **por repositorio** y las tarjetas independientes del S3 siguen pintándose (verificación: tests 2.11 y 2.12 en verde).
- [ ] 2.14 GREEN (D11/B5): con varios repositorios y uno ilegible, la tarjeta de espacio SHALL declarar la cifra como **mínimo** («al menos»), reutilizando el patrón `truncated`; con uno solo ilegible NO SHALL presentar la suma parcial como total (verificación: test nuevo en verde; el rótulo `Espacio en S3 (al menos)` aparece).
- [ ] 2.15 REFACTOR: unificar la condición de «la cifra es un mínimo» —`truncated` o repositorio ilegible— en una sola expresión, sin duplicar el rótulo (verificación: `pnpm test` en verde).

### Parte B (D12/B6) — visibilidad de salud por repositorio

- [ ] 2.16 **DECIDIDA**: este change implementa la forma **mínima** de B6 (atribuir el fallo por nombre desde el listado, sin contrato nuevo); la forma **completa** (campo/estado de salud por repositorio + endpoint de comprobación + indicador en Ajustes, que arrastra `repository`/`api-resources`) se abre como **change aparte** y NO entra aquí (verificación: D12 del proposal ya no está «pendiente» y no se abre ningún contrato nuevo en este change).
- [ ] 2.17 Con la forma mínima, NO SHALL añadirse ningún campo a `Repository` ni ninguna ruta a `/api/v1/repositories` (verificación: `rg -n "salud|health" ui/src/services/api/types.ts src/api/repositories.rs` sin apariciones nuevas).


## 3. Parte C — la consola de una ejecución terminada (#101)

### RED

- [ ] 3.1 UI: una ejecución con `status = failed` y `error_message` no nulo, seleccionada en el Histórico, hace que la consola muestre ese `error_message` (verificación: `npx vitest run src/components/ConsolaDeEjecucion` en rojo hoy —la consola sale vacía—).
- [ ] 3.2 UI: una ejecución con `status = success` y sin líneas en el historial hace que la consola muestre un texto claro, no un hueco (verificación: en rojo hoy).

### GREEN

- [ ] 3.3 `ui/src/components/ConsolaDeEjecucion/ConsolaDeEjecucion.tsx`: cuando no hay líneas y la ejecución está terminada, pinta el resumen —`error_message` si `failed`, texto claro si no— en vez del vacío (verificación: tests 3.1 y 3.2 en verde).
- [ ] 3.4 El tipo `PropsDeConsolaDeEjecucion` recoge lo que la consola necesita de la fila (al menos `status` y `error_message`), y la pantalla del Histórico se lo pasa (verificación: `npx tsc --noEmit` limpio).
- [ ] 3.5 El historial en vivo (`ui/src/services/useEventStream.ts`) NO se toca: sigue siendo en memoria y sin replay; solo cambia lo que la consola hace cuando no hay líneas (verificación: `rg -n "replay|persist" ui/src/services/useEventStream.ts` sin apariciones nuevas).

### REFACTOR

- [ ] 3.6 Reutilizar el `Empty`/`Typography.Text` que la consola ya usa para el estado sin selección, sin duplicar estilos (verificación: `pnpm test` en verde).

## 4. Verificación y cierre

- [ ] 4.1 `just check-all` (verificación: fmt, clippy, test, typecheck, tests de UI, lint y build en verde).
- [ ] 4.2 Fusionar los deltas en las specs con `just spec-archive renders-y-resiliencia-del-panel` (verificación: `openspec validate --all --strict` en verde y `openspec show ui --type spec` mostrando la columna «Tarea» por nombre y el resumen de consola).
- [ ] 4.3 Documentar la feature en `docs/FEATURE-0NN-<slug>.md`, emparejada con su plan (verificación: el documento describe las tres partes, los ficheros tocados y el alcance decidido de la parte C —resumen, no log completo—).
- [ ] 4.4 Renombrar el plan a `plans/PLAN-0NN-COMPLETED.md` con `git mv` y abrir el PR a `development` con gitmoji y `Closes #99, #100, #101` (verificación: plan renombrado y PR abierto).
