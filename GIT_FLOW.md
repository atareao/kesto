# Git Flow — Kesto

Este proyecto sigue **Git Flow** con versionado semántico. Igual que el resto de proyectos del entorno, se usa **siempre**: ninguna feature se commitea directamente en `main` ni en `development`.

**Estado actual:** ramas `main` y `development` creadas, sin remote y sin CI. Pendiente: repo en GitHub, `.github/workflows/`, `.vampus.yml` y `cliff.toml` (la skill `gitflow` los genera en un paso).

## Ramas

| Rama | Propósito | Base |
|---|---|---|
| `main` | Producción. Cada merge aquí disparará una release. | — |
| `development` | Integración de features en curso. Rama activa de trabajo. | `main` |
| `feature/<kebab-case>` | Nuevas funcionalidades. | `development` |
| `hotfix/<kebab-case>` | Correcciones urgentes a producción. | `main` |

## Flujo diario

### Features

```bash
git checkout development && git pull --ff-only
git checkout -b feature/mi-feature
# ... trabajo ...
git commit -m "✨ feat: add dark mode toggle"
git push -u origin feature/mi-feature
gh pr create --base development --head feature/mi-feature
```

### Hotfixes

```bash
git checkout main && git pull --ff-only
git checkout -b hotfix/arreglo-critico
git commit -m "🚑️ hotfix: crash on empty input"
git push -u origin hotfix/arreglo-critico
gh pr create --base main --head hotfix/arreglo-critico
# tras el merge, sincronizar la integración:
git checkout development && git merge main && git push origin development
```

### Releases

Todavía no hay release automatizada (sin `.vampus.yml` ni workflows). Procedimiento manual mientras tanto:

1. PR `development` → `main`.
2. Tag anotado `vX.Y.Z` en `main`.
3. Sincronizar `development` con `main`.

Cuando se configure el CI con la skill `gitflow`, `release-prepare.yml` hará el bump de versión, el changelog con git-cliff y el tag automáticamente.

## Conventional Commits con gitmoji

El formato del mensaje determina el bump automático:

| Mensaje | Bump |
|---|---|
| `✨ feat: ...` | minor (0.Y.0) |
| `🐛 fix: ...` | patch (0.0.Z) |
| `💥 feat!: ...` o `BREAKING CHANGE` | major (X.0.0) |
| cualquier otro tipo (`refactor`, `docs`, `chore`, `test`…) | patch (0.0.Z) |

| Tipo | Emoji | Tipo | Emoji |
|---|---|---|---|
| `feat` | ✨ | `style` | 💄 |
| `fix` | 🐛 | `test` | ✅ |
| `hotfix` | 🚑️ | `chore` | 🔧 |
| `docs` | 📝 | `ci` | 👷 |
| `refactor` | ♻️ | `revert` | ⏪️ |
| `perf` | ⚡ | `deps` | ⬆️ |
| `breaking` | 💥 | `release` | 🔖 |

## Formateo previo obligatorio

Antes de crear ramas, commitear o abrir PRs, formatea para que el CI no falle. **El layout de Kesto es `src/` (Rust) + `ui/` (React), no `backend/` + `frontend/`** — adapta los comandos de la skill `gitflow` a estos directorios:

```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test
cd ui && pnpm run lint && npx tsc --noEmit && pnpm test
```

Si el formateo produce cambios, commitéalos aparte antes de cualquier otro cambio:

```bash
git commit -m "🎨 style: apply formatting"
```

## Planes y features

- Un plan por trabajo en `plans/PLAN-NNN.md`; al terminar `plans/PLAN-NNN-COMPLETED.md`, si se aparca `plans/PLAN-NNN-PENDING.md`.
- Un documento por feature entregada en `docs/FEATURE-NNN-<slug>.md`.
- Convención completa: [AGENTS.md](./AGENTS.md).

## Resumen visual

```text
main        ──●───────────────────────●────────────
               \                      /
development ────●──●──●──●──●──●──────●
                 \    /   \  /
feature           ●──●     ●─●
```
