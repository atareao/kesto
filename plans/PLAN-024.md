# PLAN-024 — Nombre de tarea obligatorio y único, y renders del panel

- **Issues:** #87, #89, #93
- **Rama:** `feature/panel-contratos-de-tareas`
- **Change SDD:** `openspec/changes/contratos-y-renders-del-panel/`
- **Estado:** en curso

## Objetivo

Tres frentes en un solo change:

1. **A) `backup_jobs.name` obligatorio y único (global).** Deja de ser opcional: contrato de esquema y de API.
2. **B) Renders del panel (#87, #89).** Las columnas «Nombre» (Tareas) y «Tarea» (Snapshots) muestran el nombre; la primera no queda vacía y la segunda no duplica la «Etiqueta».
3. **C) Poda de directorio (#93).** La confirmación de sobrescritura es alcanzable también para targets de directorio.

## Por qué

- Hoy `name` es `NULL`-able **a propósito** (migración `0005`): «una tarea sin nombre es un estado válido». En uso, eso deja la columna «Nombre» vacía y obliga a los renders a rellenar con `''` o con `label`. El usuario ha decidido cerrar la raíz: **el nombre pasa a ser obligatorio y único**.
- **#93**: el backend ya autoriza la poda con `sobrescribir`, pero la UI solo abre la confirmación para bases; un directorio solo recibe.

## Decisiones aprobadas por el usuario

- Unicidad **global**: `UNIQUE(name)` en toda la tabla.
- Las tareas existentes sin nombre se rellenan con un **nombre derivado automático y único** en la propia migración.
- El marcador «sin nombre» atenuado queda solo como **red de seguridad** en los renders (si algún día llegara un `null`), no como solución.

## Alcance

### A — Contrato del nombre
- Migración nueva `migrations/0007_<slug>.sql`: **reconstrucción** de `backup_jobs` para `name TEXT NOT NULL` con `UNIQUE`; antes, rellenar los `NULL` con un valor derivado único. Reconstruir la tabla exige cuidar las claves foráneas (ver `AGENTS.md`: «el `DROP TABLE` de una reconstrucción se lleva por delante las filas que cuelgan»).
- `src/api/jobs.rs`: en crear y actualizar, `name` deja de ser `Option<String>`; se exige no vacío (tras `trim`); el duplicado responde conflicto (`409`) con la envoltura del §5.
- UI: el campo «Nombre» del modal de tarea pasa a obligatorio y el `4.09` se muestra en el formulario.

### B — Renders
- `ui/src/pages/tareas.tsx`: la celda «Nombre» muestra `tarea.name` (red de seguridad «sin nombre» si llegara `null`).
- `ui/src/pages/snapshots.tsx`: la columna «Tarea» muestra `job_name` y **deja de caer a `label`**; la «Etiqueta» no se toca.
- Corregir los comentarios que hoy justifican el hueco y la duplicación.
- Actualizar `ui/src/pages/tareas.test.tsx:185` y `ui/src/pages/snapshots.test.tsx:447`.

### C — Poda de directorio
- `ui/src/pages/snapshots.tsx`: abrir la confirmación para directorios y enviar `sobrescribir: true` al confirmar.
- `ui/src/api/snapshots.ts`: corregir los comentarios engañosos.

### Specs
- Deltas en `openspec/specs/` para: el nombre de tarea (contrato), la UI y la restauración.

## Tareas (TDD)

1. **RED** — #93: directorio que confirma → `sobrescribir: true`; sin confirmar → `false`.
2. **RED** — #87: tarea sin nombre muestra «sin nombre» (red de seguridad).
3. **RED** — #89: snapshot de tarea sin nombre no duplica «Etiqueta».
4. **RED** — A: crear/actualizar sin nombre → `400`; nombre repetido → `409`; la migración rellena los `NULL` y deja la columna `NOT NULL` + `UNIQUE`.
5. **GREEN** — migración, API, modal y renders.
6. **REFACTOR** — deduplicar la rama de base y la de directorio; limpiar comentarios.
7. **Spec** — fusionar los deltas.
8. **Cierre** — `just check` (backend) y `pnpm run lint && npx tsc --noEmit && pnpm test` (UI); luego `just check-all`.

## Criterios de aceptación

- No se puede crear ni editar una tarea sin nombre ni con un nombre ya usado (global).
- La migración deja las 5 tareas existentes con nombre único y la columna `NOT NULL` + `UNIQUE`.
- «Nombre» (Tareas) nunca queda vacía; «Tarea» (Snapshots) nunca coincide con «Etiqueta» por falta de nombre.
- Un target de directorio ofrece la confirmación de sobrescritura igual que uno de base.
- `just check-all` en verde.

## Riesgos

- Reconstruir `backup_jobs` con claves foráneas encendidas puede llevarse filas que cuelgan: la migración debe tratarlas.
- La unicidad global puede chocar con tareas homónimas ya existentes; el relleno derivado debe garantizarlo.
- La poda es **irreversible y previa a la escritura**: la confirmación debe advertirlo.
- Numeración: se usa `PLAN-024` porque el `PLAN-023` viaja en el PR #95, aún sin mergear.