# Relevo — FEATURE-018: restauración de snapshots (PLAN-018 · issue #64)

Rama `chore/handoff-tras-68` (solo la actualización de este fichero, pendiente de PR). El **PR #68** (https://github.com/atareao/kesto/pull/68) ya está **fusionado en `development`** (merge `fa33aa6`, 2026-11-15), y el feature branch `feature/restauracion-de-snapshots` ya no está pendiente. El issue **#64** hubo que cerrarlo a mano: el `Closes #64` no lo cerró al fusionarse contra `development`, porque GitHub solo lo aplica contra la rama por defecto, `main`. También se cerró a mano el issue **#62**, cuyo PR #63 se había fusionado el 2026-11-14 sin referencia de cierre. `main` sigue en **v0.6.1** (tag `v0.6.1`) y lo que hay en `development` es una feature `feat` todavía sin publicar. El cambio está archivado en `openspec/changes/archive/2026-11-15-restauracion-de-snapshots/` y el plan ya está renombrado a `plans/PLAN-018-COMPLETED.md`.

## Hecho en esta sesión — `feature/restauracion-de-snapshots` (issue #64)

Feature nueva y completa, con el ciclo SDD+TDD cerrado de punta a punta. El endpoint `POST /api/v1/snapshots/{id}/restore` con cuerpo `{repository_id}` resuelve el vínculo `snapshot → job_executions → backup_jobs → backup_targets` (`Ejecuciones::objetivo_de_la_instantanea` en `src/db/entities/execution.rs`) y responde `200` con `execution_id` (efímero, no persistido), `snapshot_id`, `target_id` y `target_name`; los errores son `400` UUID mal formado, `404` repositorio ausente **o instantánea que no está en el repositorio** (comprobado contra el almacenamiento antes de responder, no optimista), `409` sin vínculo o destino ya ocupado, y `422` para targets de BD (v1 solo restaura `directory`). La restauración real vive en `Repositorio::restaurar_snapshot` (`src/rustic/repositorio.rs`: `prepare_restore` + `restore`, `delete: false`, `verify_existing: true`), corre en `spawn_blocking` (`fuera_del_runtime`) y se lanza en segundo plano para que el `200` vuelva antes que el trabajo.

El progreso viaja por el stream SSE global con `event: restore_progress` (`NOMBRE_DEL_EVENTO_DE_RESTAURACION` en `src/api/events.rs`) y las fases `restaurando`/`terminada`/`fallida`: el primer aviso se publica **antes** de restaurar, y el cierre se publica en el cuerpo asíncrono para cubrir también el pánico del hilo de bloqueo. Los cuatro contadores (`files_done`, `files_total`, `bytes_done`, `bytes_total`) van **siempre a 0** porque `rustic_core` 0.13 no ofrece progreso incremental por su API pública: queda declarado como limitación en el plan, la spec y el doc de la feature. La guarda de exclusión por destino es `Motor::reservar_el_destino` + `ReservaDeRestauracion` (con `Drop`, clave normalizada con `canonicalize` cuando existe), en `src/engine/mod.rs`.

El listado `GET /api/v1/snapshots?repository_id=` se amplió con `target_id`/`target_name`/`target_type` nullable por fila, con consulta por lotes vía `json_each` y el vínculo fusionado por el manejador (el motor no lee la base). El panel (`ui/src/pages/snapshots.tsx`, `useSnapshots.ts`, `ui/src/api/snapshots.ts`, `ui/src/services/useEventStream.ts`, `events.tsx`) trae la acción «Restaurar», una confirmación que **nombra el target**, acción deshabilitada y explicada si el target es desconocido o no es `directory`, resultado **al cerrar fase** (no al `202`), progreso por **fase** (sin barra), aviso cerrable con plazo de confirmación de 30 min, suscripción al SSE mientras la operación está en vuelo, `loading`/`disabled`, a11y por fila y **una sola restauración a la vez** en el panel. La migración `migrations/0004_snapshot_id.sql` añade el índice sobre `job_executions.snapshot_id` (medido con `EXPLAIN QUERY PLAN`: lo usa la consulta de una instantánea; la del lote `json_each` no lo usa).

Proceso: dos auditorías y dos re-auditorías sin bloqueantes; los cuatro bloqueantes originales quedaron corregidos. **Verificación:** `just check-all` verde — backend `fmt`, `clippy -D warnings` y `cargo test` con **464 passed, 0 failed, 22 ignored**; UI `pnpm run lint` limpio, `tsc` app y tests OK, `pnpm test` con **188 passed** y build OK (con el aviso preexistente del chunk de ~1,3 MB por Antd). `openspec validate --all --strict` → **12/12**.

## Pendiente / siguiente

- **(a) Publicar la release: abrir el PR `development` → `main` — lo único que falta.** El PR #68 ya está fusionado en `development`, así que la feature está integrada; lo que queda es la release. Un merge a `main` **es** la release: `release-prepare` decide el bump desde los conventional commits (aquí hay varios `feat`, así que **minor**, o sea **v0.7.0**) y empuja él mismo el tag, que es lo que dispara `release.yml` — el tag **no** se pone a mano. Los issues #64 y #62 quedaron cerrados a mano (ver arriba). Nada más de esta feature está pendiente: el plan está marcado y renombrado, el change archivado y las deltas fusionadas en `openspec/specs/api-resources/spec.md` y `openspec/specs/ui/spec.md`.
- **(b) Fase 2 de la restauración (issue #67).** Persistir la ejecución de restauración —hoy es efímera— y restaurar targets de BD dentro de su contenedor; más adelante, restauración parcial por rutas y purga de snapshots.
- **(c) Heredados vivos.** Copia binaria `.backup` de SQLite (API `sqlite3_backup`) y Litestream; y el reintento/purga del «olvido fallido» de FEATURE-015.
- **(d) Follow-up de FEATURE-016.** El bundle del panel sigue en un único chunk (~1,3 MB / ~405 kB gzip) por Antd; `manualChunks`/code-splitting futuro.
- **(e) Nota de CI.** El test `dos_restauraciones_simultaneas_sobre_el_mismo_destino_no_caben` (`src/api/snapshots.rs`) tarda ~25 s en CI: cada restauración abre el repositorio dos veces y cada apertura cuesta varios segundos en la compilación de pruebas.
- Al abrir feature nueva: empezar por su **GitHub Issue**, luego `plans/PLAN-NNN.md` y el change SDD.

## Lab (levantado; heredado de la sesión anterior)

Red podman `kesto-lab`, todo alcanzable por nombre. Comprobado: `rustfs-lab`, `mariadb-src`, `postgres-src`, `sqlite-src` y `kesto` siguen arriba.

- `rustfs-lab` — RustFS (S3), puertos 9000/9001, creds `rustfsadmin`/`rustfsadmin`, bucket **`kesto`**. Datos en `lab/rustfs/`.
- `mariadb-src` (`pruebas`), `postgres-src` (`pruebas`), `sqlite-src` (`/datos/app.db`, WAL).
- `kesto` — imagen **`ghcr.io/atareao/kesto:0.6.1`** con el **panel nuevo montado**: `-v /data/rust/kesto/ui/dist:/app/ui/dist:ro` (la imagen publicada no recompila el binario por un cambio de panel). Red `kesto-lab`, `-p 8080:8080`, `--userns=keep-id:uid=10001,gid=10001`, volumen `kesto_data:/app/data`, socket podman en `/var/run/docker.sock`, `-v /data/rust/kesto/lab/directorio:/datos:ro`. Clave maestra en `/tmp/opencode/kesto-master-key`. Panel: `http://127.0.0.1:8080`.
  - **Cuidado:** si recreas `kesto` sin el montaje de `ui/dist`, vuelve al panel **viejo** de la imagen. Para el panel nuevo, reconstruye `ui/dist` (`pnpm run build`) y añade el montaje; o construye una imagen completa con `just build`.
- Parar el lab: `podman rm -f kesto rustfs-lab mariadb-src postgres-src sqlite-src`.
- **Recordatorio:** credenciales de Docker Hub del host caducadas → usar `podman pull --authfile /tmp/opencode/anon.json <img>`.

## Recordatorios heredados

- El CI **no** levanta daemon de contenedores: los e2e van `#[ignore]` (`cargo test -- --ignored`). `DOCKER_HOST=unix:///run/user/1000/podman/podman.sock` en esta máquina.
- El binario del host (glibc nueva) **no** corre en la imagen `bookworm`: para probar contra RustFS, usar la imagen publicada del tag.
- Antes de cada commit: `just check` (backend) y, en `ui/`, `pnpm run lint` → `pnpm run typecheck` → `pnpm run typecheck:test` → `pnpm test`. Git flow con gitmoji; nada directo a `main`/`development`.
