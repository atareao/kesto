# Handoff — kesto — 2026-11-15 (v0.9.0 publicada; release-prepare desbloqueado)

## Objetivo
Sin tarea abierta. El trabajo de esta sesión —issue **#79** (filtro por target en Restauraciones) y el **desbloqueo del pipeline de release** (#83)— está entregado y publicado en **v0.9.0**.

## Estado
- **v0.9.0 publicada**: tag `v0.9.0` sobre `e3af578 chore: release v0.9.0`; `.vampus.yml` y `Cargo.toml` en `0.9.0`; GitHub Release `kesto v0.9.0`; `release.yml` completo en verde (crate, imágenes amd64+arm64 con smoke test y manifiesto multi-arquitectura). `main` y `development` **sincronizadas** (0/0).
- **#79 cerrado** (PR #80, `cb9d882`): selector de **target** en la pantalla de Restauraciones (opciones del catálogo `GET /api/v1/targets`, `target_id` en la consulta, reinicio de paginación). Sin cambios de backend, esquema ni contrato de API. SDD archivado (`openspec/changes/archive/2026-11-15-filtro-por-target-en-restauraciones/`; requisito fusionado en `openspec/specs/ui/spec.md:545`); plan `plans/PLAN-022-COMPLETED.md`; doc `docs/FEATURE-022-filtro-por-target-en-restauraciones.md`.
- **#83 cerrado** (PR #84, `45903a5`): `release-prepare` estaba **roto de forma permanente** desde v0.8.0. La release v0.8.0 había **versionado `vampus.tgz`** (2 MB) porque la descarga escribía dentro del árbol y el paso de commit usaba `git add -A`; el checkout traía el fichero y la descarga abortaba con «already exists». Arreglado: descarga a `$RUNNER_TEMP` con `--clobber`, `git add` explícito de `CHANGELOG.md .vampus.yml Cargo.toml Cargo.lock`, `vampus.tgz` fuera del índice y en `.gitignore`. Detalle en el issue #83 y en los comentarios del workflow.
- PRs de relevo: #80 (feat, a `development`), #81 (relevo, a `development`), #82 (release, a `main`), #84 (hotfix, a `main`).

## Pendiente
1. **Ninguna tarea abierta.** No hay issues abiertos ni changes SDD activos (`openspec/changes/` solo tiene `archive/`).
2. Follow-ups de #67 (listados en `docs/FEATURE-022-…`): restauración parcial por rutas, copia previa automática, progreso incremental real (bloqueado por `rustic_core`) y e2e de los `#[ignore]` en CI.
3. Auditoría de la UI sugiere **ticket aparte**: extraer pie de paginación, `TAMANO_DE_PAGINA` y `ESTADOS` compartidos entre `historico.tsx` y `restauraciones.tsx`; y `allowClear` de Antd es solo-ratón (un botón «Limpiar filtros» lo resolvería).
4. Decidir si el arreglo de CI de #83 merece documento propio: **no se creó** `plans/PLAN-023.md` ni `docs/FEATURE-023-…` por proporcionalidad (es un hotfix de 3 líneas de workflow), pero la convención del repo documenta los arreglos de infraestructura como features (`FEATURE-010` a `FEATURE-012`). Si se quiere, es un `📝 docs:` trivial.

## Punto de partida
```
git checkout development && git pull origin development
# backend: just check   ·   ui/: pnpm run lint && npx tsc --noEmit && pnpm test
# cierre: just check-all
```

## Metodología / reglas
SDD (change en `openspec/changes/`) + TDD (RED por aserción → GREEN → REFACTOR). Rama `feature/*` desde `development`, PR a `development`; **hotfix desde `main`** con PR a `main` y sincronización posterior de `development`. Nada directo a `main`/`development`. Gitmoji + conventional commits. Cierre: `docs/FEATURE-0NN-<slug>.md` y plan renombrado a `-COMPLETED`.

## Riesgos y notas
- **Un merge a `main` ES una release.** Y ahora, tras el arreglo de #83, el commit de release solo añade cuatro ficheros: si algún día hace falta versionar otra cosa desde `release-prepare`, hay que ampliar esa lista a propósito.
- El binario `vampus.tgz` **sigue en el historial de git** (pesa 2 MB). Se retiró del árbol, no del pasado; reescribir el historial no se consideró.
- Restaurar una base **pisa datos vivos**: la única red es `sobrescribir: true` + teclear el nombre del target; no hay copia previa automática (decisión D de #67).
