# PLAN-021 — Restauración de bases de datos (fase 2) + persistencia

- **Issue:** #67 (fase 2 de la restauración; fase 1 = #64 / FEATURE-018).
- **Change SDD:** `openspec/changes/restauracion-de-bases-de-datos/`.
- **Fecha:** 2026-11-15.
- **Estado:** completado (verificado con `just check-all`, auditorías sin bloqueantes y change archivado).

## Objetivo

Cerrar el ciclo de la copia para los targets de base de datos: restaurar dentro de su contenedor el volcado que la copia guardó (reverso del `exec` del §4), con confirmación explícita de sobrescritura y modo seguro por defecto, y **persistir** la ejecución de restauración (hoy efímera) con su histórico en el panel.

## Alcance

- Restaurar `mariadb`, `postgresql` y `sqlite` en su contenedor, por `stdin`, sin fichero temporal.
- Confirmación explícita: `sobrescribir: true` en la API + teclear el nombre del target en el panel.
- Tabla `restore_executions` (migración `0006`) + `GET /api/v1/restorations` + pantalla «Restauraciones».
- Levantar la limitación por tipo en el panel; siguen prohibidos los snapshots sin target (`target_type` null).

## Fuera de alcance

- Restauración parcial por rutas y purga de snapshots.
- Destino alternativo o copia de seguridad previa automática.
- Progreso incremental (`rustic_core` 0.13 sin callback).

## Decisiones

- **D1 — Streaming obligatorio.** El volcado sale del repo con `Repository::dump(node, &mut impl Write)` y viaja al `stdin` del `exec` por un puente bloqueante acotado (espejo del `ProductorDelVolcado`). Sin fichero temporal (§4).
- **D2 — `attach_stdin` en bollard.** El transporte pide `attach_stdin` y escribe el canal en `StartExecResults::Attached{input}`; cierra el `stdin` solo tras el último byte.
- **D3 — Comandos inversos.** `mariadb -u<user> [<db>]`, `psql -U <user> -d <db|postgres>`, y `sqlite`: retirar `<db_path>`/`-wal`/`-shm` y `sqlite3 <db_path>` reimportando el `.dump`.
- **D4 — Modo seguro.** Sin `sobrescribir: true`, un target de BD se rechaza con `428 Precondition Required` sin lanzar trabajo. `AppError::PreconditionRequired`.
- **D5 — Persistencia.** `restore_executions` con `status` CHECK (`pending|running|success|failed`), `error_message`, `started_at`, `finished_at`, `target_id` FK `ON DELETE CASCADE`; el `execution_id` del `200` es la fila.
- **D6 — Un change para todo el issue.`

## Mapa arquitectónico

| Capa | Fichero(s) | Cambio |
|---|---|---|
| Esquema | `migrations/0006_restauraciones.sql` | Tabla `restore_executions` + índice |
| Datos | `src/db/entities/restauracion.rs` (nuevo), `mod.rs` | Entidad + acceso: alta, actualizar, listar filtrado |
| Rustic | `src/rustic/repositorio.rs` | `dump` de un nodo a un `Write` puente |
| Docker | `src/docker/cliente.rs` | `attach_stdin` + inyección de canal + drenaje de stderr |
| Restauración | `src/engine/restauracion.rs` (nuevo) | Comando inverso + flujo (2 execs sqlite) |
| Motor | `src/engine/mod.rs` | Rama por tipo, reserva por destino de BD, persistencia del ciclo de vida |
| API | `src/api/snapshots.rs`, `src/api/restorations.rs` (nuevo) | `sobrescribir`, `42,8`, `execution_id` persistido, `GET /restorations` |
| Error | `src/error.rs` | `PreconditionRequired` → `428` |
| UI | `ui/src/pages/snapshots.tsx`, `ui/src/pages/restorations.tsx` (nuevo), `ui/src/hooks/*`, `ui/src/api/*`, `ui/src/services/*` | Confirmación tipada, `sobrescribir`, pantalla histórico |

## Tareas (TDD)

### 0. Preparación
- [x] 0.1 `just check-all` verde antes de tocar código.
- [x] 0.2 Migración `0006` + entidad/acceso de `restore_executions`.
- [x] 0.3 Tests que fallen para cada contrato (ver `tasks.md` del change).

### 1. RED
- [x] 1.1 db: tabla/CHECK/FK/índice; estado inválido rechazado; cascada borra histórico.
- [x] 1.2 Comando inverso mariadb/postgresql; credencial en env, no en cmd.
- [x] 1.3 sqlite retira `<db_path>`/`-wal`/`-shm` antes de reimportar.
- [x] 1.4 Streaming repo→stdin sin fichero temporal; cierre tras último byte. **Hecho**: puente acotado + `volcar_a` + `ejecutar_con_entrada` (los tests de motor van `#[ignore]`).
- [x] 1.5 API `428` sin `sobrescribir`; `200` con él; fila `pending` antes del `200`.
- [x] 1.6 `GET /restorations` con filtro, orden y paginación; ruta protegida.
- [x] 1.7 Motor: dos restauraciones sobre el mismo destino no caben. **Hecho**: `la_reserva_de_un_destino_de_base_excluye_la_segunda_y_admite_otro_destino`.
- [x] 1.8 UI: confirmación tipada, `sobrescribir` condicional, pantalla histórico.

### 2. GREEN
- [x] 2.1 Transporte stdin + `rustic::dump` a puente. **Hecho**: `ejecutar_con_entrada` y `volcar_a`, con el puente `tunel_de_restauracion`. La unión de la entrega y el drenaje vive en `conducir_la_orden`: **manda el drenaje**, así que un cliente que deja de leer su `stdin` acaba en `RestauracionEstancada` en vez de colgar la orden.
- [x] 2.2 `restauracion` (comandos + flujo) y rama por tipo en el motor. **Hecho**: `ejecutar_las_ordenes`/`ejecutar_una` conducen el flujo (volcado solo a la última orden, corte en la primera que falla) y `Motor::restaurar` decide la rama por `CaminoDeRestauracion`.
- [x] 2.3 Persistencia `pending→running→success|failed` con `error_message`. **Hecho**: `marcar_en_curso`/`cerrar_la_restauracion`, con las dos pruebas de desenlace en `engine::tests`.
- [x] 2.4 API `sobrescribir`/`428`/`execution_id` y listado.
- [x] 2.5 UI: levantar limitación, confirmación tipada, pantalla Restauraciones.

### 3. Refactor y verificación
- [x] 3.1 `just check-all` verde (backend fmt/clippy/tests; UI lint/typecheck/test/build).
- [x] 3.2 Auditoría `@rust-reviewer` + `@react-reviewer`; corregir bloqueantes.
- [x] 3.3 `docs/FEATURE-021-restauracion-de-bases-de-datos.md`.
- [x] 3.4 Coherencia delta/tasks/plan/doc.

### 4. Cierre
- [x] 4.1 `openspec archive restauracion-de-bases-de-datos`.
- [x] 4.2 Renombrar este plan a `plans/PLAN-021-COMPLETED.md`.
- [x] 4.3 PR a `development` con gitmoji y `Closes #67`.

## Verificación

- `just check-all` (backend: `cargo fmt --check` + `cargo clippy -D warnings` + `cargo test`; UI: lint + typecheck + test + build).
- `openspec validate restauracion-de-bases-de-datos --strict`.
- E2E manual contra el lab (rustfs + mariadb-src/postgres-src/sqlite-src): copiar y restaurar cada tipo; verificar datos.

## Riesgos

- **Destructivo por diseño.** Restaurar una base pisa datos vivos; la única red es la bandera explícita y la confirmación tipada. Sin copia previa automática (decisión).
- **sqlite:** se retira el fichero vivo antes de reimportar; ventana sin base. Requiere `sqlite3` en el contenedor.
- **Contenedor parado o sin cliente** → `failed` con motivo del contenedor.
- **Compatibilidad de la migración** con el esquema existente; sin cambios de columnas en tablas previas.