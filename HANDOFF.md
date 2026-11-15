# Handoff — kesto — 2026-11-15 (issue #79 en curso: filtro por target en Restauraciones)

## Objetivo
Issue **#79**: añadir un selector de **target** a la pantalla de Restauraciones para acotar el histórico por base de datos. Follow-up de #67. Ciclo **SDD (change aprobado) + TDD**: RED en curso.

## Estado
- Relevo anterior **ya cerrado**: PR #77 (`#67` fase 2) **MERGED** y release **v0.8.0** publicada; `main` y `development` idénticas, sin PRs ni issues abiertos.
- Rama de trabajo: **`feature/filtro-target-en-restauraciones`** desde `development`.
- Issue **#79** abierto y asignado a `atareao` (punto de entrada).
- SDD: change `filtro-por-target-en-restauraciones` con sus 4 artefactos; `openspec validate --strict` → **válido**. Delta **solo** sobre la capacidad `ui` (requisito «El panel muestra el histórico de restauraciones» pasa a filtro por estado **y** por target); sin capacidades nuevas y **sin cambios de backend/esquema/API**.
- Plan: `plans/PLAN-022.md`. Commit de planificación: `30af250` (`📝 docs(plan): spec SDD y PLAN-022 del filtro por target en Restauraciones`).
- Reconocimiento: `GET /api/v1/restorations` ya acepta y prueba `target_id` (`src/api/restorations.rs`, test `el_filtro_por_target_acota_el_listado`), el índice `idx_restore_executions_target_id` ya existe, y `ui/src/api/restauraciones.ts` + `ui/src/hooks/useRestauraciones.ts` ya transportan el parámetro. **Falta solo el selector en `ui/src/pages/restauraciones.tsx`.**

## Pendiente
1. **RED**: `@frontend-dev` escribiendo los casos nuevos en `ui/src/pages/restauraciones.test.tsx` (acotar por target con total, reinicio de paginación, limpiar/combinar filtros, target ausente del catálogo). Confirmar que fallan y que el resto sigue verde + `npx tsc --noEmit`.
2. **GREEN**: selector de target con `allowClear` en `ui/src/pages/restauraciones.tsx`, opciones de `useTargets()`, patrón del Histórico (`ui/src/pages/historico.tsx`), reinicio de `offset` al cambiar filtro.
3. **REFACTOR + auditoría**: `pnpm run lint` → `npx tsc --noEmit` → `pnpm test` → `pnpm run build`; `@react-reviewer` sobre el diff sin bloqueantes.
4. **Cierre**: `just check-all` EXIT=0; `docs/FEATURE-022-filtro-por-target-en-restauraciones.md`; `openspec archive filtro-por-target-en-restauraciones`; `plans/PLAN-022.md` → `-COMPLETED`; PR a `development` con `Closes #79`.

## Punto de partida
```
git checkout feature/filtro-target-en-restauraciones
just check-all
# ui/: pnpm test -- restauraciones   ·   npx tsc --noEmit
```

## Metodología / reglas
SDD (change en `openspec/changes/`) + TDD (RED por aserción → GREEN → REFACTOR). Rama `feature/*` desde `development`, PR a `development`; nada directo a `main`/`development`. Gitmoji + conventional commits. Cierre: `docs/FEATURE-0NN-<slug>.md` y plan renombrado a `-COMPLETED`.

## Riesgos y notas
- Restaurar una base **pisa datos vivos**: la única red es `sobrescribir: true` + teclear el nombre del target; no hay copia previa automática (decisión D de #67). Este issue **no** lo mitiga.
- Decisión D4: si un `target_id` desaparece del catálogo, no se reconcilian las opciones del selector; se pinta el vacío.
- Follow-ups de #67 que siguen abiertos: restauración parcial por rutas, copia previa automática, progreso incremental real (bloqueado por `rustic_core`) y e2e de los `#[ignore]` en CI.
