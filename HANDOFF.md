# Handoff — kesto — 2026-11-15 (fase 2 de #67, bloqueante de `@rust-reviewer` corregido)

## Objetivo
Continuar la fase 2 de restauración de Kesto (issue **#67**): restaurar snapshots de bases de datos dentro de su contenedor y persistir esa ejecución (hoy efímera).

## Estado
- Hecho: #66 sidebar (PR #72), chore de vampus (PR #73), #65 (PR #74 + #75) y el relevo anterior (PR #76). Todo en `development`; última release **v0.7.0** (`main` en `f4f8a8f`).
- En curso: #67 restauración de bases de datos. Change `openspec/changes/restauracion-de-bases-de-datos/`, plan `plans/PLAN-021.md`. Fase 1 (migración `0006_restauraciones.sql` + entidad/acceso `src/db/entities/restauracion.rs`), el **comando inverso por tipo** (`src/engine/restauracion.rs`), el **camino streaming repo→`stdin`** (tareas 1.4, 2.1 y 2.2) —puente acotado `tunel_de_restauracion`, `Repositorio::volcar_a` y `Cliente::ejecutar_con_entrada`—, el **flujo de órdenes** (`ejecutar_las_ordenes`/`ejecutar_una`, tarea 2.3), la **rama por tipo y la persistencia** en `Motor::restaurar` (tarea 2.4) y la **API** (2.5). Sin fichero temporal en ningún punto.
- Bloqueante de `@rust-reviewer` corregido (ALTA): la unión de la entrega del volcado y el drenaje de la salida vive ahora en `conducir_la_orden` (`src/docker/cliente.rs`), con **el drenaje mandando** —`pin!` + `select`—, así que un contenedor que deja de leer su `stdin` acaba en `RestauracionEstancada` en vez de dejar la entrega bloqueada para siempre (fila en `running` y reserva sin soltar). Precedencia conservada: si las dos mitades terminan, manda el error de la entrada; un drenaje bueno con la entrega pendiente **no** es un bien (es un volcado truncado). Dos nits de doc arreglados en `src/engine/mod.rs` (el «un solo camino» del aviso de cierre no cubre un pánico del propio cuerpo asíncrono; la garantía del ciclo de vida cubre «la fila no está» pero no «la escritura falló»).
- Rama: `development`, con los cambios **sin commitear** (el usuario pidió expresamente no commitear). `ui/` intacto.
- Verificado: `just check` en verde (`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` y `cargo test --locked`: 532 pasan —527 de la unidad + 5 de `tests/arranque.rs`—, 27 `#[ignore]`). Los 5 e2e nuevos/tocados también pasan a mano con Podman (`cargo test -- --ignored --exact <nombre>`).

## Pendiente
Seguir con #67: UI (tareas 2.7 y 2.8 del change: levantar la limitación por tipo, confirmación tipada y pantalla de Restauraciones) y, con las dos auditorías (3.2), el cierre (3.1 `just check-all`, 3.3 doc de la feature, 3.4 coherencia delta/tasks/plan y 4.1–4.3). Ojo con la sincronización de `tasks.md`/`plans/PLAN-021.md` que aún tiene pendientes ya implementados (p. ej. `0.4`, `1.1`–`1.3`, `1.5`, `1.6`). Antes de commitear: `just check`.

## Punto de partida
```
git checkout development && git pull origin development
git checkout -b feature/restauracion-comando-inverso  # o reutiliza la rama de #67 si ya existe
# SDD: openspec new change <nombre>  → plan plans/PLAN-021.md (ruta explícita a @plan-writer)
# backend: just check   ·   ui/: pnpm run lint && pnpm run typecheck && pnpm test
# cierre: just check-all
```

## Metodología / reglas
SDD (change en `openspec/changes/`) + TDD (RED con fallos por aserción → GREEN → REFACTOR). Rama `feature/*` desde `development`, PR a `development`; nada directo a `main`/`development`. Gitmoji + conventional commits. Cierre: `docs/FEATURE-0NN-<slug>.md`, plan renombrado a `-COMPLETED`.

## Riesgos y notas
- Diseño de #67: hoy el panel limita a **una restauración a la vez** y los targets de BD (`mariadb`/`postgresql`/`sqlite`) devuelven `422` en v1. Decidir alcance antes de tocar motor (backend + `src/engine` + panel).
- Release v0.8.0 (PR `development` → `main`): `release-prepare` calculará **minor** por los `feat` acumulados; no se etiqueta a mano.
- Follow-ups FEATURE-020: (1) `maxLength={120}` del panel sin test que lo ate al tope del backend; (2) en Snapshots, `job_name` null cae a la `label`.
- Heredados vivos: copia binaria `.backup` de SQLite y Litestream; reintento/purga del «olvido fallido» (FEATURE-015); chunk único ~1,3 MB del panel por Antd (FEATURE-016).
- CI: el test `dos_restauraciones_simultaneas_sobre_el_mismo_destino_no_caben` (`src/api/snapshots.rs`) tarda ~25 s.
- Lab levantado con imagen `ghcr.io/atareao/kesto:0.6.1` (panel nuevo montado desde `ui/dist`); conviene actualizarlo. Creds Docker Hub caducadas → `podman pull --authfile /tmp/opencode/anon.json`.
- CI no levanta daemon: e2e `#[ignore]` (`cargo test -- --ignored`); `DOCKER_HOST=unix:///run/user/1000/podman/podman.sock`.
