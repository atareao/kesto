# FEATURE-021 — Restauración de bases de datos (fase 2) + persistencia

- **Issue:** #67 (fase 2 de la restauración; la fase 1 fue el #64 / FEATURE-018).
- **Plan:** `plans/PLAN-021.md` (→ `PLAN-021-COMPLETED.md`).
- **Change SDD:** `openspec/changes/restauracion-de-bases-de-datos/`.

## Qué es

Cierra el ciclo de la copia para los targets de base de datos: restaurar **dentro de su contenedor** el volcado que la copia guardó (el reverso exacto del `exec` del §4 del SRS), con confirmación explícita de sobrescritura y **modo seguro por defecto**, y **persistir** la ejecución de restauración (hasta ahora efímera) con su histórico en el panel.

## Ficheros implicados

| Capa | Fichero | Papel |
|---|---|---|
| Esquema | `migrations/0006_restauraciones.sql` | tabla `restore_executions`, `CHECK` de `status`, FK a `backup_targets(id)` `ON DELETE CASCADE`, índices `idx_restore_executions_status_started_at` e `idx_restore_executions_target_id` |
| Datos | `src/db/entities/restauracion.rs` | `Restauracion`, `RestauracionListada` y la capa `Restauraciones` (`alta`, `leer`, `actualizar`, `listar_filtrado`) |
| Datos | `src/db/entities/execution.rs` | `ObjetivoDeRestauracion` (añade los campos del target: contenedor, usuario, contraseña descifrada, base, ruta) |
| Rustic | `src/rustic/repositorio.rs` | `Repositorio::volcar_a` (vuelca el único nodo de fichero de la instantánea a un `Write`) |
| Docker | `src/docker/cliente.rs` | `Cliente::ejecutar_con_entrada` (`attach_stdin`, escribe el volcado por `stdin`, drena `stderr`, plazo de inactividad) |
| Restauración | `src/engine/restauracion.rs` | comando inverso por tipo (`comando`), puente repo→stdin (`tunel_de_restauracion`) y conducción de órdenes (`ejecutar_las_ordenes`) |
| Motor | `src/engine/mod.rs` | `Motor::restaurar`: rama por tipo, reserva por destino, ciclo de vida persistido y fases SSE |
| API | `src/api/snapshots.rs` | `POST …/restore` con `sobrescribir`, `428`, fila `pending` antes del `200` |
| API | `src/api/restorations.rs` | `GET /api/v1/restorations` (filtros + paginación) |
| Error | `src/error.rs` | `AppError::PreconditionRequired` → `428` |
| UI | `ui/src/pages/snapshots.tsx` · `ui/src/pages/restauraciones.tsx` | confirmación tipada + pantalla de histórico |

## Cómo funciona

**Restauración de un target de base.** El motor decide la rama con `restauracion::comando(&objetivo)`:
- `mariadb`: `mariadb -u<user> [<db>]`; contraseña en `MYSQL_PWD`.
- `postgresql`: `psql -U <user> -d <db|postgres>`; contraseña en `PGPASSWORD`.
- `sqlite`: **dos** órdenes — `rm -f <db_path> <db_path>-wal <db_path>-shm` y luego `sqlite3 <db_path>` reimportando el `.dump`.
La contraseña **nunca** aparece en `cmd` (solo en el entorno), y un valor que parezca bandera (`ValorQuePareceBandera`) se rechaza.

**Streaming sin fichero temporal (§4).** El volcado sale del repositorio con `Repository::dump(node, &mut impl Write)` hacia un **puente** (`tunel_de_restauracion`): un `Write` bloqueante con `blocking_send` a un canal acotado y un lector `AsyncRead`. El final del volcado **no** viaja como mensaje: se declara con una señal (`AtomicBool`), de modo que el canal cerrado sin señal es `UnexpectedEof` (el defecto falla **cerrado**). El `docker` escribe ese lector en el `stdin` del `exec` y lo cierra **solo tras el último byte**. Nada toca el disco de Kesto.

**Liveness del transporte.** La entrega y el drenaje corren con `select` y **el drenaje manda**: si el contenedor deja de drenar, el plazo de inactividad cierra la operación (`RestauracionEstancada`) y la entrega se suelta; se preserva la precedencia «error de entrada primero».

**Persistencia.** El manejador crea la fila en `restore_executions` en `pending` **antes** del `200`, y ese `id` es el `execution_id` que devuelve la API. El motor la transiciona `pending→running→success|failed` (con `finished_at` y `error_message`), **también al fallar**: ninguna restauración queda mintiendo en `running`.

**Modo seguro.** Sin `sobrescribir: true` en el cuerpo, un target de base se rechaza con `428 Precondition Required` sin crear fila, sin reservar destino y sin publicar ningún aviso. En el panel, además, hay que **teclear el nombre del target** antes de que el botón de confirmar se habilite.

## Contrato de API

- `POST /api/v1/snapshots/{id}/restore` — cuerpo `{ repository_id, sobrescribir?: bool }` (`deny_unknown_fields`). Códigos, en orden: `400` id mal formado → `404` repositorio → `409` sin vínculo → `404` instantánea ausente en el almacenamiento → `422` `directory` sin `host_path` → `428` base sin `sobrescribir` → `409` destino ocupado → `200` con `{ execution_id, snapshot_id, target_id, target_name }`.
- `GET /api/v1/restorations?status=&target_id=&limit=&offset=` — `200` con `data: { restorations, total }`; cada fila: `id`, `snapshot_id`, `target_id`, `target_name` (nullable), `target_type` (nullable), `status`, `error_message`, `started_at`, `finished_at`. Orden `started_at DESC, id`; en la mitad protegida.
- Progreso por SSE global `restore_progress` con fases `restaurando` / `terminada` / `fallida` (contadores a `0` en esta versión: `rustic_core` 0.13 no ofrece callback de progreso).

## Variables y configuración

No añade variables de entorno nuevas. Reutiliza `KESTO_MASTER_KEY` (descifrado del secreto del target) y `DOCKER_HOST` (socket de podman en dev).

## Cómo habilitar/desplegar

No hay interruptor: es el comportamiento del endpoint existente. Desplegar es aplicar la migración `0006` (la hace `sqlx::migrate!()` al arrancar). Los targets `mariadb`/`postgresql`/`sqlite` son restaurables; un `directory` sin `host_path` sigue dando `422`.

## Decisiones

- **D1 Streaming obligatorio** — `Repository::dump` + bollard `attach_stdin`, sin fichero temporal.
- **D2 Cierre del `stdin`** tras el último byte; `LogOutput::StdIn` ignorado.
- **D3 Comandos inversos** exactos por tipo (arriba).
- **D4 Modo seguro** con `428 Precondition Required` y confirmación tipada en el panel.
- **D5 Persistencia** en `restore_executions`; el `execution_id` del `200` es la fila.
- **D6 Cascada**: borrar un target borra su histórico (`ON DELETE CASCADE`).

## Verificación

- `just check-all` en verde (backend `fmt`/`clippy -D warnings`/`test`; UI `lint`/`typecheck`/`typecheck:test`/`test`/`build`).
- Backend: **532 tests pasan** (527 unidad + 5 integración), 27 `#[ignore]` (e2e con podman).
- Panel: **217 tests pasan** (30 ficheros).
- `openspec validate restauracion-de-bases-de-datos --strict` → válido.
- Auditorías `@rust-reviewer` y `@react-reviewer`: sin bloqueantes tras corregir el de liveness del `join!`.

## Follow-ups

- Progreso incremental real (bytes/ficheros) cuando `rustic_core` exponga callback.
- Restauración parcial por rutas y purga de snapshots.
- Copia de seguridad previa automática antes de sobrescribir (hoy la red es la bandera + confirmación tipada).
- Filtro por `target_id` expuesto en la UI de Restauraciones (la API ya lo soporta).
- e2e de los escenarios `#[ignore]` en CI con un contenedor de podman.
