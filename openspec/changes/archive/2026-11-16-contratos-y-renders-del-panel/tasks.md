# Tasks — contratos-y-renders-del-panel

> Tres partes en un solo change, y el orden importa: la parte A cambia el **contrato** (columna, API y
> formulario), así que los renders de la parte B se escriben contra el contrato nuevo, no contra el
> viejo. La parte C es independiente de las otras dos y puede ir en cualquier momento.
>
> Sin cambio de backend en la parte C: la bandera `sobrescribir` ya viaja en
> `CuerpoDeLaRestauracion` (`#[serde(default)]`, `src/api/snapshots.rs:169`) y el motor ya la aplica
> como `RestoreOptions::delete(podar)`; la puerta del `428` es sólo para las bases
> (`src/api/snapshots.rs:453-462`), así que un directorio sin bandera sigue restaurándose.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` y `just ui-check` (verificación: los dos terminan sin error antes de tocar nada).
- [x] 0.2 Confirmar el siguiente número de migración libre (`0007`, tras `0006_restauraciones.sql`) (verificación: `ls migrations/` no tiene ningún `0007*`).
- [x] 0.3 Cerrar las dos decisiones propuestas del proposal antes de escribir la migración: el `CHECK` de no-vacío (D7) y la sensibilidad a mayúsculas de la unicidad (D8) (verificación: las dos están resueltas por escrito en el proposal, sin «pendiente»).
- [x] 0.4 Abrir el issue de la parte A con `gh issue create --assignee atareao` o anexarla explícitamente a #87, y reescribir `plans/PLAN-024.md` para las tres partes (verificación: el proposal cita el número de issue y el plan deja de cubrir sólo la parte C).

## 1. Parte A — RED

- [x] 1.1 Rust: un alta **sin** `name` contesta `400` con la envoltura y el mensaje **nombra `name`** (verificación: `cargo test --locked` falla hoy; con la `0007` y el borde nuevo pasa).
- [x] 1.2 Rust: un alta con `name` en blanco —`"   "`— contesta `400` nombrando `name`; el escenario viejo «un nombre en blanco se guarda como `null`» se retira, porque codifica el contrato que este change deroga (verificación: el test del contrato derogado ya no existe y el nuevo falla hoy).
- [x] 1.3 Rust: un alta con un `name` que ya tiene **otra** tarea contesta **`409`** con la envoltura, y el mensaje **no repite el valor** (verificación: falla hoy —hoy entra sin más—).
- [x] 1.4 Rust: una edición que **conserva** el nombre propio de la tarea NO choca: contesta `200` (verificación: falla hoy por ausencia; es el test que impide que el `409` convierta el formulario en intocable).
- [x] 1.5 Rust: los mensajes del `400` por falta y del `409` por duplicado son **distintos** entre sí (verificación: la aserción compara los dos textos).
- [x] 1.6 Rust: `GET /api/v1/jobs` publica `name` como cadena no vacía en **todas** las filas, y el escenario «el listado publica el nombre anulable» se retira (verificación: falla hoy).
- [x] 1.7 Migración: con una base llevada a mano hasta la `0006` y poblada con dos tareas sin nombre y una con nombre, más sus filas en `job_executions`, aplicar la `0007` deja `name` `NOT NULL` y `UNIQUE`, las tres filas con nombre **distinto**, y **todas** las ejecuciones intactas (verificación: falla hoy —no existe la `0007`—; es el test que cubre el `DROP TABLE` de una tabla padre).
- [x] 1.8 Migración: ningún nombre derivado es el identificador de la tarea ni una copia de él, y ninguno mide más de 120 caracteres (verificación: falla hoy).

## 2. Parte A — GREEN

- [x] 2.1 `migrations/0007_nombre_de_tarea_obligatorio.sql`: reconstruir `backup_jobs` con `name TEXT NOT NULL UNIQUE`, copiando **columna a columna, nombradas**, recreando `idx_backup_jobs_target_id` e `idx_backup_jobs_repository_id`, y dejando `name` en la última posición declarada, como la dejó la `0005` (verificación: tests 1.7 y 1.8 en verde; `PRAGMA table_info(backup_jobs)` sin cambio de orden).
- [x] 2.2 La migración rellena las filas con `name` a `NULL` con un nombre derivado y único —función de la propia fila, sin chocar con ningún nombre ya presente— y no toca las que ya tenían (verificación: test 1.7 en verde).
- [x] 2.3 El manejo de claves foráneas de la reconstrucción: la migración corre con `foreign_keys` apagado (`db::migrate::aplicar`) y la verificación posterior confirma que no quedaron filas huérfanas (verificación: test 1.7 en verde con las ejecuciones presentes; ningún `job_executions` perdido).
- [x] 2.4 `docs/DEFINITION.md` §3: la columna `name` de `backup_jobs` pasa a `TEXT NOT NULL` con `UNIQUE(name)` (verificación: `rg -n "name" docs/DEFINITION.md` muestra la forma nueva en la tabla del §3).
- [x] 2.5 `src/db/entities/job.rs` y su capa de acceso: `name` deja de ser `Option<String>` y la violación de la unicidad se traduce a un error tipado que el borde HTTP pueda convertir en `409` (verificación: `cargo clippy --all-targets -- -D warnings` limpio y test 1.3 en verde por el camino de la base, no por un pre-chequeo).
- [x] 2.6 `src/api/jobs.rs`: `AltaDeTareaDelCable` acepta el nombre y el borde lo **exige** —falta o en blanco tras recortar es `400` nombrando `name`—, el duplicado sale `409` y `TareaDelCable` publica `name` no anulable (verificación: tests 1.1 a 1.6 en verde).
- [x] 2.7 `ui/src/components/ModalDeTarea/ModalDeTarea.tsx`: «Nombre» es obligatorio (regla `required` con `whitespace: true` —un `required` a secas deja pasar una cadena de espacios—), se deja de omitir la clave cuando queda vacío, y los rechazos del guardado se reparten por campo —`400` que nombra `name` y `409` a «Nombre», `400` que nombra `cron_expression` al cron— sin cerrar el formulario (verificación: tests de `ModalDeTarea.test.tsx` actualizados y en verde; el «Nombre» deja de tener placeholder «Sin nombre»).
- [x] 2.8 `ui/src/services/api/types.ts`: `Job.name` deja de ser `string | null` y `AltaJob.name` pasa a obligatorio, con los comentarios del contrato viejo retirados (verificación: `npx tsc --noEmit` limpio).

## 3. Parte B — RED

- [x] 3.1 UI: en `ui/src/pages/tareas.test.tsx:185`, la aserción que hoy exige la celda **vacía** se sustituye por el contrato nuevo: la celda muestra el nombre y, cuando el valor llega `null` o en blanco, un marcador explícito y atenuado —nunca un hueco y nunca el nombre del target— (verificación: `npx vitest run src/pages/tareas.test.tsx` en rojo hoy).
- [x] 3.2 UI: en `ui/src/pages/snapshots.test.tsx:236-238`, la aserción que hoy espera el identificador de la tarea en la columna «Tarea» se sustituye por el marcador atenuado, y se añade la prueba de que «Tarea» y «Etiqueta» **nunca** muestran el mismo valor (verificación: `npx vitest run src/pages/snapshots.test.tsx` en rojo hoy).
- [x] 3.3 UI: el `describe` de «Tarea» y «Etiqueta» (`snapshots.test.tsx:210-240`) deja de presentar el identificador de la tarea como el sustituto válido de «Tarea» (verificación: el texto del bloque describe la causa del `null` que queda —la instantánea huérfana— y ningún test del bloque queda en rojo por el comentario).

## 4. Parte B — GREEN

- [x] 4.1 `ui/src/pages/tareas.tsx:91-97`: la celda «Nombre» pinta `tarea.name` y su red de seguridad atenuada, sin caer al nombre del target (verificación: test 3.1 en verde).
- [x] 4.2 `ui/src/pages/snapshots.tsx:440-445`: la columna «Tarea» pinta `job_name` y **deja de caer a `label`**; el `null` se pinta con el marcador atenuado; «Etiqueta» queda tal cual (verificación: tests 3.2 y 3.3 en verde).

## 5. Parte C — RED

- [x] 5.1 UI: restaurar un snapshot de un target `directory` **confirmando** la sobrescritura envía el `POST` con `sobrescribir: true` (verificación: `npx vitest run src/pages/snapshots.test.tsx` en rojo —hoy la rama de directorio no ofrece la confirmación y nunca manda la bandera—).
- [x] 5.2 UI: restaurar un directorio **sin** confirmar envía el `POST` con `sobrescribir: false`, y el test que hoy afirma `toMatchObject({ repository_id: 'r1' })` (`snapshots.test.tsx:447`) se ajusta a ese contrato (verificación: falla en el árbol actual y pasa tras 6.1/6.2).
- [x] 5.3 UI: el comentario de contrato del `describe` de restauración (`snapshots.test.tsx:244-260`) deja de presentar la sobrescritura como exclusiva de las bases y nombra la poda del directorio (verificación: el texto lo dice y ningún test del bloque queda en rojo).

## 6. Parte C — GREEN

- [x] 6.1 `ui/src/api/snapshots.ts`: el cuerpo del `POST` lleva `sobrescribir` como booleano explícito (`true`/`false`) y el comentario de `PeticionDeRestauracion` deja de afirmar que en un directorio no hay nada que sobrescribir (verificación: `npx vitest run src/api` + test 5.2 en verde, `npx tsc --noEmit` limpio).
- [x] 6.2 `ui/src/pages/snapshots.tsx`: la rama por tipo deja de reservar la confirmación a `esTargetDeBase`; un `directory` la ofrece y, al aceptarla, llama a `alRestaurar(snapshot, true)` —la firma lleva la bandera explícita para todo tipo— (verificación: test 5.1 en verde).
- [x] 6.3 `ui/src/pages/snapshots.tsx`: el aviso de la confirmación de un directorio explica que se **retira del destino** lo que no está en la instantánea, y la ruta sin confirmar sigue restaurando con `sobrescribir: false` (verificación: aserción de texto del test 5.1 y test 5.2 en verde).

## 7. REFACTOR y verificación

- [x] 7.1 Backend: sin préstamos que sobrevivan a la escritura ni `unwrap`/`expect` nuevos; el error de unicidad viaja tipado desde la capa de acceso hasta el `409` (verificación: `cargo clippy --all-targets -- -D warnings` limpio y `rg -n "unwrap\(\)|expect\(" src/api/jobs.rs src/db/` sin apariciones nuevas fuera de tests).
- [x] 7.2 Frontend: deduplicar la rama de confirmación de directorio con la de base —un solo camino y una sola llamada a `alRestaurar(snapshot, sobrescribir)`—, sin cambiar comportamiento (verificación: `pnpm test` sigue en verde).
- [x] 7.3 Corregir los comentarios que alimentan los defectos: el «no hay nada que sobrescribir» del `alRestaurar` y de `ui/src/api/snapshots.ts`, y el «sin nombre la celda queda vacía» de `tareas.tsx` (verificación: `rg -n "nada que sobrescribir" ui/src` y `rg -n "Sin nombre" ui/src` sin resultados en los sitios corregidos).
- [x] 7.4 Cierre de calidad: `just check-all` (verificación: fmt, clippy, test, typecheck, tests de UI, lint y build en verde).

## 8. Spec, documentación y cierre

- [x] 8.1 Fusionar los cuatro deltas en las specs con `just spec-archive contratos-y-renders-del-panel` (verificación: `openspec validate --all --strict` en verde y `openspec show ui --type spec` mostrando la confirmación de directorio).
- [x] 8.2 Documentar la feature en `docs/FEATURE-024-nombre-de-tarea-obligatorio-y-renders-del-panel.md`, emparejada con `plans/PLAN-024.md` (verificación: el documento describe las tres partes, los ficheros tocados, la forma del nombre derivado y la decisión de no tocar el backend en la parte C).
- [ ] 8.3 Renombrar `plans/PLAN-024.md` a `plans/PLAN-024-COMPLETED.md` con `git mv` y abrir el PR a `development` con gitmoji y `Closes #87, #89, #93` (verificación: plan renombrado y PR abierto).
