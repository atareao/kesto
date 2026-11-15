# PLAN-018 — Restauración de snapshots sobre el target de origen

**Issue:** #64 — ✨ feat: restaurar un snapshot desde el panel
**Base:** `development` tras el merge de #63
**Rama:** `feature/restauracion-de-snapshots` (desde `development`)
**Change SDD:** `openspec/changes/restauracion-de-snapshots/`
**Estado:** Completado — PR #68 a `development` abierto, pendiente de merge

## 1. Objetivo

Permitir restaurar un snapshot completo desde el panel sobre el **target del que procede**. Hoy el sistema solo lista snapshots (`GET /api/v1/snapshots`); no existe ninguna operación de restauración (`src/api/snapshots.rs:169-197`, `ui/src/pages/snapshots.tsx`).

## 2. Contexto y alcance

- `rustic_core` 0.13.0 expone `Repository::prepare_restore(snapshot, dest) -> RestorePlan` y `Repository::restore(plan, &RestoreOptions, node_streamer, &LocalDestination)`; el destino es un **sistema de ficheros local** (`LocalDestination`).
- No hay tabla `snapshots`: la única relación es `job_executions.snapshot_id` (`docs/DEFINITION.md:112-123`) y `job_executions.job_id -> backup_jobs.target_id -> backup_targets` (`:97-109`).
- El `label` del snapshot es el `job_id` en forma con guiones (`src/engine/ejecucion.rs:431`), así que el mapeo `snapshot -> job` puede resolverse por `job_executions.snapshot_id` o por la etiqueta.

### Alcance v1

- Restaurar **snapshot completo** (no parcial) sobre el target de origen.
- Tipos soportados en v1: `directory` → restaurar el árbol del snapshot en `backup_targets.host_path` con `LocalDestination` y `RestoreOptions` conservadores (`delete: false`, `verify_existing: true`).
- Endpoint de restauración, evento SSE de progreso y acción en la UI con confirmación.

### Fuera de alcance v1 (fase 2)

- Restaurar snapshots de targets de BD (`mariadb`, `postgresql`, `sqlite`) dentro de su contenedor (reverso del `exec` de volcado, o escritura del `.db`). Se documenta el enfoque, pero no se implementa en v1 por el riesgo de sobrescribir datos vivos.
- Restauración parcial por rutas.
- Borrado/purga de snapshots.

### Decisión pendiente de aprobación

v1 solo cubre `directory`. Para BD, la restauración real exige inyectar el volcado en el contenedor; se difiere a fase 2. Si el usuario quiere BD en v1, ampliar este plan antes de aprobar el change SDD.

## 2-bis. Enmiendas tras auditoría (D1–D3)

Una auditoría posterior al borrador del change encontró tres puntos en los que el contrato no decía la verdad. Los tres se incorporan a la delta y a este plan; **el contrato cambia**, no es un ajuste de redacción.

- **D1 — El listado amplía su contrato.** `GET /api/v1/snapshots?repository_id=<id>` añade a **cada** elemento `target_id`, `target_name` y `target_type`, anulables y a `null` cuando la instantánea no casa con ninguna ejecución/tarea/target (copia huérfana, hecha por otra herramienta o con la fila borrada por `ON DELETE CASCADE`). **Por qué:** la confirmación del panel no puede nombrar el target y la acción no puede saber si es restaurable si el listado no lo dice. **Implica:** ampliar el DTO del listado y su consulta, tolerar el vínculo ausente sin romper el listado, y añadir escenarios en `specs/api-resources/spec.md` y en `specs/ui/spec.md` (target desconocido y tipo no restaurable → acción deshabilitada y limitación explicada).
- **D2 — `404` de verdad si la instantánea no está en el repositorio.** El handler comprueba **antes de responder** que la instantánea existe en el almacenamiento; si no está, `404`. **Por qué:** el `200` optimista daba por lanzada una restauración contra una instantánea inexistente; el §4 de este plan ya prometía el `404` y no se cumplía. **Implica:** comprobar contra el repositorio antes de acuñar la ejecución, y no publicar ni arrancar nada en ese caso.
- **D3 — El progreso se declara limitación.** El evento `restore_progress` publica un «empezó/terminó» con `phase`, y los cuatro contadores (`files_done`, `files_total`, `bytes_done`, `bytes_total`) van **siempre a `0`**, porque `Repository::restore` de `rustic_core` 0.13 no ofrece callback de progreso por su API pública. **Por qué:** el escenario 4 prometía «avance» y no lo hay; prometer lo que sí ocurre —la fase— es lo honesto. **Implica:** reescribir el escenario 4 (§5) y el de `specs/ui/spec.md` para hablar de fase (en curso / terminada / fallida) sin avance incremental, y dejar la limitación en el plan, en la spec y en `docs/FEATURE-018`.

Puntos que la auditoría dejó como tareas, sin cambio de contrato salvo donde se dice:

- **Guardar contra restauraciones concurrentes sobre el mismo destino:** dos restauraciones del mismo target a la vez escriben el mismo árbol. Hay que impedir la segunda.
- **Índice `snapshot_id`:** el mapeo del listado (una resolución por fila) y el de la restauración filtran por `job_executions.snapshot_id`, que hoy **no** tiene índice (`0002_indexes.sql` no lo crea). Falta una migración que lo añada —solo el índice, sin tocar columnas—.
- **Publicar la fase terminal fuera del cierre bloqueante**, para que un fallo del trabajo o un cierre tardío no dejen al panel sin desenlace.

## 3. Mapa técnico

| Fichero | Cambio previsto |
|---|---|
| `src/db/…` o el módulo de consultas | Consulta que mapea `snapshot_id -> (job_id, target)` uniendo `job_executions`, `backup_jobs` y `backup_targets` (una resolución por instantánea, tolerante a la ausencia de vínculo). |
| `src/rustic/repositorio.rs` | Método bloqueante `restaurar_snapshot(snapshot_id, destino)` usando `prepare_restore` + `restore`, con `fuera_del_runtime`; y comprobación de que la instantánea existe en el índice antes de restaurarla (D2). |
| `src/rustic/origen.rs` | Reutilizar el acceso/credenciales del repositorio. |
| `src/engine/mod.rs` | `Motor::restaurar(repository_id, snapshot_id)` async: abre repo, resuelve el target, valida el tipo, ejecuta y emite progreso; guarda contra restauraciones concurrentes sobre el mismo destino. |
| `src/api/snapshots.rs` | Ruta `POST /api/v1/snapshots/{id}/restore` con `{ repository_id }`; el `404` real antes de responder (D2); y los tres campos nuevos del listado (D1). |
| `migrations/0004_….sql` | Índice sobre `job_executions.snapshot_id` (solo índice; sin cambios de columnas). |
| `src/api/events.rs` | Evento SSE `restore_progress` (payload acordado en la spec). |
| `src/api/mod.rs` | Montar la ruta en el router de negocio (protegida por OID, abierta en modo `Disabled`). |
| `ui/src/pages/snapshots.tsx` | Acción «Restaurar» por fila, modal de confirmación (muestra el target del listado) y reflejo de la fase. |
| `ui/src/services/api/*` | Cliente del endpoint, tipos del listado ampliados y consumo del evento SSE. |
| `docs/FEATURE-018-restauracion-de-snapshots.md` | Documentación de la feature. |

## 4. Contrato de API (a fijar en la delta)

### 4.1 `GET /api/v1/snapshots?repository_id=<id>` (ampliado, D1)

Cada elemento de `data` añade a los seis campos actuales (`id`, `time`, `paths`, `label`, `files`, `bytes`):

- `target_id` (`<uuid>` o `null`), `target_name` (cadena o `null`), `target_type` (cadena o `null`).
- Se resuelven por el mismo vínculo que la restauración: `snapshot → job_executions → backup_jobs → backup_targets`, acotado al `repository_id` que se pide.
- Los tres van a `null` cuando la instantánea no casa con ninguna ejecución/tarea/target del repositorio. `null` es un valor del contrato: la fila sigue apareciendo y las demás no se ven afectadas.

### 4.2 `POST /api/v1/snapshots/{id}/restore`

- Cuerpo: `{ "repository_id": "<uuid>" }`.
- Respuesta OK (`200`): `{ "status": 200, "message": "...", "data": { "execution_id": "<uuid>", "snapshot_id": "<id>", "target_id": "<uuid>", "target_name": "..." } }`.
- Errores: `404` si el repositorio no existe **y** `404` si la instantánea no existe en el almacenamiento de ese repositorio —comprobado **antes** de responder, no optimista (D2)—; `409` si la instantánea existe pero no está asociada a ninguna ejecución/target; `422` si el target no es restaurable en v1.
- Progreso por SSE: `event: restore_progress` con `{ execution_id, snapshot_id, phase, files_done, files_total, bytes_done, bytes_total }`. En v1 la fase es `restaurando` → `terminada` | `fallida`, y los cuatro contadores van a `0` (limitación de `rustic_core` 0.13; D3).

## 5. Escenarios BDD mínimos

1. **Restaurar un snapshot de un target de directorio:** dado un snapshot de un target `directory`, cuando se lanza la restauración, entonces los ficheros quedan en `host_path` y la operación termina en `success`.
2. **Snapshot sin target asociado:** dado un snapshot que existe en el repositorio pero sin vínculo, cuando se pide restaurar, entonces la API responde `409` con un mensaje claro.
3. **Target no restaurable en v1:** dado un snapshot de un target `mariadb`, cuando se pide restaurar, entonces la API responde `422` explicando que no está soportado.
4. **Progreso (fase, no avance — D3):** durante una restauración, el panel recibe `restore_progress` y refleja la **fase** —en curso, terminada o fallida— sin recargar; los cuatro contadores van a `0`, así que no hay porcentaje ni barra determinada.
5. **Sin borrado destructivo:** con `delete: false`, los ficheros presentes que no están en el snapshot no se eliminan.
6. **Listado con target (D1):** una instantánea con ejecución trae `target_id`/`target_name`/`target_type`; una huérfana los trae a `null` y sigue en el listado.
7. **Instantánea ausente (D2):** dado un `id` que no está en el almacenamiento, cuando se pide restaurar, entonces la API responde `404` y no se publica ni se lanza nada.

## 6. Tareas TDD

### Fase 0 — Preparación
- [x] 0.1 Verificar `just check` y `just ui-check` en verde.
- [x] 0.2 Fijar el contrato del evento SSE y la forma del mapeo con un test de integración que falle.
- [x] 0.3 Fijar con tests que fallen el contrato del listado ampliado (tres campos nullable; huérfana → `null`) y el `404` real antes de responder (D1/D2).

### Fase 1 — RED
- [x] 1.1 Test de API: `POST .../restore` con snapshot de directorio devuelve `data` con `execution_id` y `target_id` (fallará).
- [x] 1.2 Test de API: snapshot sin vínculo devuelve `409`; target de BD devuelve `422`.
- [x] 1.3 Test de UI (vitest): el botón «Restaurar» abre confirmación y llama al cliente.
- [x] 1.4 Test de API (D1): el listado trae `target_id`/`target_name`/`target_type` de una instantánea con ejecución, y los tres `null` para una sin vínculo, sin perder la fila.
- [x] 1.5 Test de API (D2): `POST .../restore` con un `id` que no está en el almacenamiento devuelve `404` y no publica `restore_progress` ni lanza ejecución.
- [x] 1.6 Test de UI (D3): el evento `restore_progress` refleja la fase (en curso / terminada / fallida) sin recargar y sin porcentaje ni barra determinada.
- [x] 1.7 Test de UI (D1): con `target_type` a `null` o distinto de `directory`, la acción queda deshabilitada y se explica la limitación.
- [x] 1.8 Test: dos restauraciones concurrentes sobre el mismo destino — la segunda se rechaza.

### Fase 2 — GREEN
- [x] 2.1 Implementar el mapeo `snapshot_id -> target` en la capa de datos, tolerante a la ausencia de vínculo.
- [x] 2.2 Implementar `restaurar_snapshot` en `rustic` con `prepare_restore` + `restore`.
- [x] 2.3 Implementar `Motor::restaurar` con emisión de `restore_progress` al hub.
- [x] 2.4 Añadir la ruta y el handler con la envoltura.
- [x] 2.5 Añadir la acción y el modal en `snapshots.tsx`.
- [x] 2.6 Extender el listado (`SnapshotDelCable` y su consulta) con los tres campos nullable (D1).
- [x] 2.7 Comprobar la instantánea contra el almacenamiento antes de responder y contestar `404` si no está (D2).
- [x] 2.8 Publicar la fase terminal (`terminada`/`fallida`) fuera del cierre bloqueante, para que el desenlace llegue aunque el trabajo falle o el cliente se suscriba tarde (D3).
- [x] 2.9 Guarda contra restauraciones concurrentes sobre el mismo destino.
- [x] 2.10 Migración con índice `snapshot_id` sobre `job_executions` (solo índice).

### Fase 3 — REFACTOR y verificación
- [x] 3.1 `just check` y `just ui-check` en verde.
- [x] 3.2 Auditar con `@rust-reviewer` (backend) y `@react-reviewer` (UI).
- [x] 3.3 Escribir `docs/FEATURE-018-restauracion-de-snapshots.md`.
- [x] 3.4 Repasar que la delta, el plan, las tasks y el doc de la feature siguen diciendo lo mismo tras las enmiendas D1–D3.

## 7. Criterios de aceptación

- [x] Restaurar un snapshot de un target `directory` deja los datos en `host_path`.
- [x] Snapshot sin vínculo y target no soportado devuelven error claro.
- [x] Una instantánea que no está en el repositorio devuelve `404` real (D2).
- [x] El listado publica `target_id`/`target_name`/`target_type` por fila, con `null` para las huérfanas (D1).
- [x] El panel muestra confirmación con el target, deshabilita las filas no restaurables y refleja la **fase** (D1/D3).
- [x] No se admiten dos restauraciones concurrentes sobre el mismo destino.
- [x] `just check` y `just ui-check` verdes.
- [x] Change SDD archivado, plan renombrado y PR abierto.

## 8. Riesgos

- **Alto (mitigado):** restaurar sobre datos vivos. Se restringe a `directory` con `delete: false`, y BD queda fuera de v1.
- **Medio:** mapeo imposible si la ejecución/target se borraron (`ON DELETE CASCADE`). Se responde `409`; en el listado esos campos van a `null`.
- **Medio:** snapshots creados antes de la feature sin fila en `job_executions`. Mismo `409` / `null` en el listado.
- **Medio (D3):** el aviso de progreso no da avance incremental; el panel debe pintar solo la fase para no parecer colgado ni mentir con una barra.
- **Medio:** dos restauraciones del mismo target a la vez escriben el mismo árbol; la guarda de concurrencia es lo que lo evita.
- **Bajo:** resolver el target por cada fila del listado añade consultas; el índice `snapshot_id` acota el coste.

## 9. Flujo git

1. `git checkout development && git pull origin development`
2. `git checkout -b feature/restauracion-de-snapshots`
3. Commits con gitmoji (`✨ feat`, `✅ test`, `📝 docs`…). PR a `development`.