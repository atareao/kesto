# Handoff — kesto — 2026-11-15 (#67 fase 2 completada, PR #77 abierto)

## Objetivo
Issue **#67**: restaurar snapshots de bases de datos dentro de su contenedor, con modo seguro y confirmación explícita, y persistir la ejecución de restauración. **Completado**; pendiente solo el merge del PR.

## Estado
- Hecho: #66 (PR #72), chore de vampus (PR #73), #65 (PR #74/#75) y el relevo anterior (PR #76), todo en `development`; última release **v0.7.0**.
- **#67 fase 2 entregada** en la rama `feature/restauracion-de-bases-de-datos` → **PR #77** a `development` (`Closes #67`). Commits: `✨ feat(restauracion): …`, `📝 docs(restauracion): …`, `📝 docs(plan): …`.
- SDD: change `restauracion-de-bases-de-datos` **archivado** en `openspec/changes/archive/2026-11-15-restauracion-de-bases-de-datos/`; specs fusionados en `openspec/specs/` (`openspec validate --specs --strict` → 13 pasan, 0 fallan).
- Plan: `plans/PLAN-021-COMPLETED.md` (todas las casillas cerradas). Doc: `docs/FEATURE-021-restauracion-de-bases-de-datos.md`.
- Entregado: migración `0006_restauraciones.sql` + entidad/acceso `Restauraciones`; comando inverso por tipo; streaming repo→`stdin` (`Repository::dump` + puente acotado + `attach_stdin`, sin fichero temporal); liveness (`conducir_la_orden`, manda el drenaje); rama por tipo y persistencia `pending→running→success|failed` en `Motor::restaurar`; API (`sobrescribir`/`428`/`execution_id` persistido + `GET /api/v1/restorations`); UI (confirmación tipada, `sobrescribir` condicional y pantalla **Restauraciones**).
- Verificado `just check-all` **EXIT=0**: backend `fmt`+`clippy -D warnings`+`test` (**532 pasan**, 27 `#[ignore]` e2e con podman); panel `lint`+`typecheck`+`typecheck:test`+`test` (**217 pasan**)+`build`.
- Auditorías `@rust-reviewer` y `@react-reviewer`: **sin bloqueantes** (se corrigió el de liveness del `join!` en `src/docker/cliente.rs`).

## Pendiente
1. **Mergear el PR #77** a `development` (si CI verde).
2. Release **v0.8.0** (PR `development` → `main`): `release-prepare` calculará **minor** por los `feat` acumulados; no se etiqueta a mano.
3. Follow-ups del issue (en `docs/FEATURE-021-…`): progreso incremental real cuando `rustic_core` lo exponga, restauración parcial por rutas, copia previa automática, filtro `target_id` en la UI y e2e de los `#[ignore]` en CI.

## Punto de partida
```
git checkout development && git pull origin development
# (si el PR #77 siguiera abierto) git fetch && git checkout feature/restauracion-de-bases-de-datos
# backend: just check   ·   ui/: pnpm run lint && pnpm run typecheck && pnpm test
# cierre: just check-all
```

## Metodología / reglas
SDD (change en `openspec/changes/`) + TDD (RED por aserción → GREEN → REFACTOR). Rama `feature/*` desde `development`, PR a `development`; nada directo a `main`/`development`. Gitmoji + conventional commits. Cierre: `docs/FEATURE-0NN-<slug>.md` y plan renombrado a `-COMPLETED`.

## Riesgos y notas
- Restaurar una base **pisa datos vivos**: la única red es `sobrescribir: true` + teclear el nombre del target; no hay copia previa automática (decisión D).
- `sqlite` retira `<db_path>`/`-wal`/`-shm` antes de reimportar (ventana sin base) y necesita `sqlite3` en el contenedor.
