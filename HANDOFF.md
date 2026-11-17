# Handoff — kesto — 2026-11-17

## Objetivo
Sesión cerrada. El bloque de defectos del panel (**#99/#100/#101**) y el arreglo del arnés de pruebas (**#94**) se fusionaron, se **publicó la release v0.11.0** y se **desplegó en el lab**. **No hay tarea en curso**.

## Estado
- **Release v0.11.0 publicada** ✅
  - PR de release **#107** (`development → main`, merge `ca2539f`) fusionado **sin** `--delete-branch` (habría borrado `development`).
  - `release-prepare` dedujo bump **minor** (por el `✨ feat(panel)` de #102) → tag **v0.11.0**, que disparó `release.yml`.
  - GitHub Release *kesto v0.11.0* (**Latest** · https://github.com/atareao/kesto/releases/tag/v0.11.0); imagen **`ghcr.io/atareao/kesto:0.11.0`** multi-arch (**amd64 + arm64**); crate en crates.io; `CHANGELOG.md` generado.
  - `Cargo.toml` y `.vampus.yml` = **0.11.0**; `development` **sincronizado** con `main` (`df273fa`).
- **Lab actualizado a 0.11.0** ✅: contenedor `kesto` recreado con los mismos parámetros (`--network kesto-lab`, `--userns=keep-id:uid=10001,gid=10001`, volumen `kesto_data`, binds `ui/dist` y `lab/directorio`, `--restart no`). `GET /api/v1/health` → `auth_mode=disabled, version=0.11.0`; DB migrada y datos intactos (repos/targets/ejecuciones listados). Respaldo previo de la base: `/tmp/opencode/kesto_data-0.10.0-20261117-064156.tar`.
- Trabajo de la sesión (ya en `main`): **#102** renders y resiliencia del panel, **#103** cierre doc, **#104** recogida de temporales de prueba, **#105** cierre doc, **#106** handoff.
- **0 issues abiertas · 0 PRs abiertas.**
- `just check-all` verde: Rust **562** tests, panel **247**, scripts (incl. `clean-tmp`), fmt/clippy limpios.
- Rama local: `development` (`df273fa`), al día con `origin`.

## Pendiente
- Nada en curso. Deudas diferidas (a criterio del usuario, en changes aparte):
  - **D12/B6**: visibilidad de salud por repositorio **completa** (campo de salud + endpoint de comprobación + indicador en Ajustes); hoy solo está la forma mínima (nombrar el repo que falla en el listado).
  - **Deuda ajena**: `openspec/specs/verificacion-e2e/spec.md` con el `## Purpose` en `TBD` → `openspec validate --all --strict` sale rojo (el CI no valida openspec).
  - **Follow-ups menores**: unificar el marcador «sin nombre»/«sin tarea»; test cruzado Rust↔UI con el cuerpo real de tarea (nota de cierre de #92); deadline de **cliente** en el panel (hoy no existe: el backend acota a 10 s); el aviso de inodos depende de `df -P -i`.

## Punto de partida
```
git checkout development && git pull --ff-only
just check-all
gh issue list --state open
```

## Metodología / reglas
SDD (change en `openspec/changes/`, aprobación del usuario **antes** de escribir código) + TDD (RED → GREEN → REFACTOR). Rama `feature/*` desde `development`; PR a `development`; nada directo a `development` ni `main`. Gitmoji + conventional commits. Antes de commitear: `just check`; antes del PR: `just check-all`. **Un merge a `main` ES una release**: `release-prepare` deduce el bump; el PR de release se llama `🔖 release: vX.Y.Z` y se fusiona **sin** `--delete-branch` (borraría `development`). Cierre de feature: deltas en `openspec/specs/`, change archivado, `docs/FEATURE-0NN-<slug>.md` y plan renombrado a `-COMPLETED`.

## Riesgos y notas
- **`Closes #NN` no cierra desde un PR a `development`**: GitHub solo autocierra al fusionar en la rama por defecto (`main`). Los issues de este bloque se cerraron **a mano** con evidencia.
- **Los tests ya no acumulan `/tmp/kesto-prueba-*`** (raíz por pid + barrido). Si `/tmp` se llena: **`just clean-tmp`**.
- **LocalAI** deja `cache/`, `downloading/`, `models/` en la raíz del repo: excluirlos de cualquier `git add` (no están en `.gitignore`).
- La app solo escucha en **IPv4**: `localhost:8080` falla, `127.0.0.1:8080` va bien.
- **Higiene de subagentes**: trocear los encargos en unidades pequeñas y verificables.
