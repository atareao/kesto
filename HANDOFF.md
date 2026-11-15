# Handoff — kesto — 2026-11-15 (issue #79 entregado y fusionado; release v0.9.0 pendiente)

## Objetivo
Issue **#79**: selector de **target** en la pantalla de Restauraciones para acotar el histórico por base de datos. **Entregado y fusionado** en `development` (PR #80, `cb9d882`), CI 3/3 verde. Pendiente solo la **release**.

## Estado
- Relevo anterior cerrado: PR #77 (#67 fase 2) **MERGED** y release **v0.8.0**; `main` quedó sincronizada.
- **#79 cerrado**. PR **#80 MERGED** en `development` (`cb9d882`) con la rama `feature/filtro-target-en-restauraciones`. Commits: `📝 docs(plan)`, `📝 docs` (relevo), `✅ test` (RED), `✨ feat` (GREEN), `✅ test` (reforzado), `📝 docs(restauracion)` (cierre), `📝 docs` (relevo).
- SDD: change `filtro-por-target-en-restauraciones` **archivado** en `openspec/changes/archive/2026-11-15-filtro-por-target-en-restauraciones/`; requisito fusionado en `openspec/specs/ui/spec.md:545`; sin changes activos; `openspec validate --specs --strict` → 13 pasan, 0 fallan.
- Plan: `plans/PLAN-022-COMPLETED.md`. Doc: `docs/FEATURE-022-filtro-por-target-en-restauraciones.md`.
- Entregado: `Select` de target con `allowClear` en `ui/src/pages/restauraciones.tsx` (opciones del catálogo `GET /api/v1/targets`, etiqueta = nombre, valor = `id`), `target_id` en los filtros de `useRestauraciones`, reinicio de `offset` al cambiar filtro y estado propio del target para sobrevivir a un target borrado del catálogo. **Sin cambios de backend, esquema ni contrato de API.**
- Verificado `just check-all` **EXIT=0**: backend `fmt`+`clippy -D warnings`+`test` (**527+5 pasan**, 27 `#[ignore]` e2e con podman); panel `lint`+`typecheck`+`typecheck:test`+`test` (**223 pasan**)+`build`.
- Auditoría `@react-reviewer`: **sin bloqueantes**; el *recomendado* (el caso de reinicio de paginación por estado no salía de la página 1) quedó cerrado en `1fbb6a5` y verificado por mutación.

## Pendiente
1. **Release v0.9.0**: `development` está por delante de `main`. Abrir PR `development` → `main` y mergear; `release-prepare.yml` deduce **minor** (`✨ feat`), sube la versión con vampus, genera `CHANGELOG.md`, empuja el tag `v0.9.0` (que dispara `release.yml`: imagen a `ghcr.io/atareao/kesto` + GitHub Release) y sincroniza `development`. **No se etiqueta a mano.**
2. Tras la release, otro relevo (`chore/handoff-tras-v0.9.0`), como manda la convención.
3. Follow-ups abiertos de #67 (en `docs/FEATURE-022-…`): restauración parcial por rutas, copia previa automática, progreso incremental real (bloqueado por `rustic_core`) y e2e de los `#[ignore]` en CI.
4. Auditoría sugiere **ticket aparte**: extraer pie de paginación, `TAMANO_DE_PAGINA` y `ESTADOS` compartidos entre `historico.tsx` y `restauraciones.tsx`; y `allowClear` de Antd es solo-ratón (un botón «Limpiar filtros» lo resolvería).

## Punto de partida
```
git checkout development && git pull origin development
# release: PR development -> main  (el tag y el changelog los hace release-prepare)
# backend: just check   ·   ui/: pnpm run lint && npx tsc --noEmit && pnpm test
# cierre: just check-all
```

## Metodología / reglas
SDD (change en `openspec/changes/`) + TDD (RED por aserción → GREEN → REFACTOR). Rama `feature/*` desde `development`, PR a `development`; nada directo a `main`/`development`. Gitmoji + conventional commits. Cierre: `docs/FEATURE-0NN-<slug>.md` y plan renombrado a `-COMPLETED`.

## Riesgos y notas
- Restaurar una base **pisa datos vivos**: la única red es `sobrescribir: true` + teclear el nombre del target; no hay copia previa automática (decisión D de #67). Este issue **no** lo mitiga.
- Decisión D4/D5: si un `target_id` desaparece del catálogo, no se reconcilian las opciones (se pinta el vacío); y no se extrajo el patrón de filtro a un componente compartido, por consistencia con Histórico mientras solo haya dos pantallas.
- Un merge a `main` **es** una release: el PR `development` → `main` no debe llevar nada que no deba publicarse.
