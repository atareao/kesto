# Handoff — kesto — 2026-11-16 18:40

## Objetivo
Cerrar los defectos de contrato y render del panel (**#87**, **#89**, **#93**) y **publicar la release** resultante en `main`.

## Estado
- Hecho:
  - **PR #96** (`feature/panel-contratos-de-tareas` → `development`) fusionado (`a2eb28d`): nombre de tarea obligatorio y único global (migración `0007`, API `400`/`409`, formulario) + columna «Tarea» de Snapshots + confirmación de poda para targets de directorio. Change archivado en `openspec/changes/archive/2026-11-16-contratos-y-renders-del-panel/`; deltas fusionados en `openspec/specs/{db,api-resources,ui,restauracion}`; `docs/FEATURE-024-nombre-de-tarea-obligatorio-y-renders-del-panel.md`; `plans/PLAN-024-COMPLETED.md`.
  - `just check-all` en verde sobre `development` tras la fusión (backend `fmt`/`clippy`/`test`, guard de tags, UI `typecheck`+`eslint`+`vitest` 228/228 + `build`).
  - **PR #97** (`🔖 release: v0.10.0`, `development` → `main`) fusionado (`88d7fd5`). `release-prepare` hizo `action=bump` → **minor** (había `✨ feat` desde `v0.9.0`) → `v0.10.0`: tag, `CHANGELOG.md`, `Cargo.lock` y sync de `development` con `main` (`46745d3`). `release.yml` publicó imagen `ghcr.io` multi-arquitectura + crate; GH release **v0.10.0 = Latest**.
  - Issues **#86, #87, #89, #93** autocerrados al fusionar en `main`.
- En curso: nada de código.
- Rama actual: `development` (al día con `origin`, `46745d3`).
- Working tree: `HANDOFF.md` modificado sin commitear → viaja en esta rama `chore/handoff-tras-v0.10.0`.

## Pendiente
- **Decidir el cierre de #88 y #90** (no se reproducen en HEAD; re-verificar contra 0.10.0 en el navegador) y de **#91/#92** (curados solo con el backend 0.9.0 desplegado).
- **#94** (agotamiento de inodos en `/tmp` por los tests) sigue abierto y sin arreglar.
- `openspec/specs/verificacion-e2e/spec.md` tiene el `## Purpose` en `TBD`: `openspec validate --all --strict` sale rojo (el CI no valida openspec). Deuda real.
- **Desplegar 0.10.0** en el contenedor `kesto` (podman) y actualizar el panel desplegado.

## Punto de partida
```
just check-all
gh issue view 88 && gh issue view 90
# desplegar 0.10.0 en podman (recordar --userns=keep-id:uid=10001,gid=10001 y
# respaldo previo de la base) y re-verificar #88/#90 en el navegador (127.0.0.1:8080)
```

## Metodología / reglas
SDD (change en `openspec/changes/`, aprobación del usuario **antes** de escribir código) + TDD (RED → GREEN → REFACTOR). Rama `feature/*` desde `development`; PR a `development`; nada directo a `development` ni `main`. Gitmoji + conventional commits. Antes de commitear: `just check`; antes del PR: `just check-all`. **Un merge a `main` ES una release**: `release-prepare` deduce el bump; el PR de release se llama `🔖 release: vX.Y.Z` y se fusiona **sin** `--delete-branch` (borraría `development`). Cierre de feature: deltas en `openspec/specs/`, change archivado, `docs/FEATURE-0NN-<slug>.md` y plan renombrado a `-COMPLETED`.

## Riesgos y notas
- **`Closes #NN` no cierra desde un PR a `development`**: GitHub solo autocierra al fusionar en la rama por defecto (`main`). Los issues de feature se cierran al pasar por el PR de release.
- **La migración `0007` es la parte delicada** (reconstruye `backup_jobs`; SQLite no añade `NOT NULL` con `ALTER`): trata las **claves foráneas** y los **homónimos preexistentes** (desambiguación por `rowid` con sufijo `(2)`, `(3)`…). Copia de seguridad recomendada antes de actualizar una instalación real.
- En este entorno **`/snapshots` no se ha recapturado en vivo** (el repositorio S3 `rustfs-lab:9000` no resuelve por DNS); la pantalla está verificada por tests.
- La app solo escucha en **IPv4**: `localhost:8080` falla, `127.0.0.1:8080` va bien.
- **Higiene de subagentes**: trocear los encargos en unidades pequeñas y verificables (sesiones previas agotaron presupuesto de pasos).
