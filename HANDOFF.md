# Handoff — kesto — 2026-11-17

## Objetivo
Sesión cerrada. Se corrigieron los tres defectos del panel aparecidos tras desplegar 0.10.0 (**#99/#100/#101**) y se arregló el agotamiento de inodos de las pruebas (**#94**). **No hay tarea en curso**; lo siguiente es, si se quiere, publicar release.

## Estado
- Hecho esta sesión (todo fusionado en `development`, todo con CI verde):
  - **PR #102** ✅ — `✨ feat(panel): renders correctos y resiliencia ante un S3 caído` (#99/#100/#101): columna «Tarea» del Dashboard = `tarea.name` (nunca el target); tope de **10 s** en la capa de Kesto (`con_tope`) + `retry=1` para que un S3 que no responde falle en ~10 s (era 36,9–38,4 s); resumen del Dashboard no todo-o-nada (nombra el repo caído, espacio como «mínimo», `retry` acotado por consulta); consola de una ejecución terminada muestra resumen/`error_message`. Change archivado `2026-11-16-renders-y-resiliencia-del-panel`; `docs/FEATURE-025-…`; `plans/PLAN-025-COMPLETED.md`.
  - **PR #103** ✅ — cierre documental del anterior.
  - **PR #104** ✅ — `✅ test(pruebas): el arnés recoge sus temporales y no agota los inodos` (**#94**): raíz única por ejecución `/tmp/kesto-prueba-<pid>/` + barrido al arrancar por pid vivo (`/proc`) + `remove_dir_all`/`remove_file` + aviso de inodos no bloqueante + receta **`just clean-tmp`** (`scripts/clean-tmp.sh`). Capacidad nueva **`arnes-de-pruebas`**; `docs/FEATURE-026-…`; `plans/PLAN-026-COMPLETED.md`. Sin dependencias nuevas (solo `std`).
  - **PR #105** ✅ — cierre documental del anterior.
  - **Issues verificadas y cerradas**: #88, #90, #91, #92 (no reproducen en HEAD; #92 cerrada con nota de seguimiento) y #94. **0 issues abiertas · 0 PRs abiertas.**
  - `just check-all` verde: Rust **562** tests, panel **247**, scripts (incl. `clean-tmp`), fmt/clippy limpios.
- En curso: nada.
- Rama actual: `development` (`30961ec`), al día con `origin`.
- Working tree: limpio salvo `cache/`, `downloading/`, `models/` (de **LocalAI**, ajenos: **no** versionar).

## Pendiente
- **(Opcional) Release**: `development` → `main` cuando se quiera publicar. Un merge a `main` **es** una release (`release-prepare` deduce el bump; el PR se llama `🔖 release: vX.Y.Z` y se fusiona **sin** `--delete-branch`).
- **Deuda ajena**: `openspec/specs/verificacion-e2e/spec.md` sigue con el `## Purpose` en `TBD` → `openspec validate --all --strict` sale rojo (el CI no valida openspec).
- **Change aparte (D12/B6)**: la visibilidad de salud por repositorio **completa** (campo de salud + endpoint de comprobación + indicador en Ajustes) quedó **decidida y diferida**; este panel solo tiene la forma mínima (nombrar el repo que falla desde el listado).
- **Follow-ups menores**: unificar el marcador «sin nombre»/«sin tarea» en un solo componente; test cruzado Rust↔UI con el cuerpo real de tarea (nota de cierre de #92); deadline de **cliente** en el panel (hoy no existe: la vista no se cuelga porque el backend acota a 10 s); el aviso de inodos depende de `df -P -i`.

## Punto de partida
```
just check-all
gh issue list --state open
```

## Metodología / reglas
SDD (change en `openspec/changes/`, aprobación del usuario **antes** de escribir código) + TDD (RED → GREEN → REFACTOR). Rama `feature/*` desde `development`; PR a `development`; nada directo a `development` ni `main`. Gitmoji + conventional commits. Antes de commitear: `just check`; antes del PR: `just check-all`. **Un merge a `main` ES una release**: `release-prepare` deduce el bump; el PR de release se llama `🔖 release: vX.Y.Z` y se fusiona **sin** `--delete-branch` (borraría `development`). Cierre de feature: deltas en `openspec/specs/`, change archivado, `docs/FEATURE-0NN-<slug>.md` y plan renombrado a `-COMPLETED`.

## Riesgos y notas
- **`Closes #NN` no cierra desde un PR a `development`**: GitHub solo autocierra al fusionar en la rama por defecto (`main`). Los issues de este bloque se cerraron **a mano** con la evidencia.
- **Los tests ya no acumulan `/tmp/kesto-prueba-*`** (raíz por pid + barrido). Si `/tmp` se llena, **`just clean-tmp`**.
- **LocalAI** deja `cache/`, `downloading/`, `models/` en la raíz del repo: excluirlos de cualquier `git add` (no están en `.gitignore`).
- La app solo escucha en **IPv4**: `localhost:8080` falla, `127.0.0.1:8080` va bien.
- **Higiene de subagentes**: trocear los encargos en unidades pequeñas y verificables.
