# Git Flow — Kesto

Este proyecto sigue **Git Flow** con versionado semántico. Igual que el resto de proyectos del entorno, se usa **siempre**: ninguna feature se commitea directamente en `main` ni en `development`.

**Estado actual:** remoto en `github.com/atareao/kesto` (público), ramas `main` y `development`, y pipeline de GitHub Actions en verde. Pendiente: crear los secretos `GH_PAT` y `CARGO_REGISTRY_TOKEN`.

## Ramas

| Rama | Propósito | Base |
|---|---|---|
| `main` | Producción. Cada merge aquí dispara una release. | — |
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

Están automatizadas: **un merge a `main` es una release**. No se etiqueta a mano.

1. PR `development` → `main`.
2. Al mergear, `release-prepare.yml` (trigger: push a `main`):
   - compara `.vampus.yml:current_version` con el tag `vX.Y.Z` y decide entre `skip` (ya está publicada y HEAD es ese tag), `bump` (hay commits nuevos) o `tag` (la versión ya se subió a mano en el PR, solo falta etiquetar);
   - deduce el bump de los conventional commits: `✨ feat` → minor, `!:` o `BREAKING CHANGE` → major, el resto → patch;
   - verifica que vampus realmente subió la versión, resincroniza `Cargo.lock`, genera `CHANGELOG.md` con git-cliff y commitea `chore: release vX.Y.Z`;
   - empuja el tag `vX.Y.Z` y sincroniza `development` con `main`.
3. El tag dispara `release.yml`:
   - **imagen** multi-arquitectura (amd64 + arm64) a `ghcr.io/atareao/kesto`, con un smoke test *antes* de publicar;
   - **crate** a crates.io;
   - **GitHub Release** con las notas del changelog.

### Requisitos antes de la primera release

| Secreto | Para qué |
|---|---|
| `GH_PAT` | PAT con `contents: write`. Lo necesita `release-prepare` en el checkout: un tag empujado con el `GITHUB_TOKEN` por defecto **no** dispara `release.yml`. |
| `CARGO_REGISTRY_TOKEN` | Token de API de crates.io para el job `publish` de `release.yml`. |

### Por qué el crate no se publica todavía

`Cargo.toml` lleva `publish = false` a propósito. `cargo publish` se niega a publicar mientras esté ahí y el job `publish` lee esa línea para saltarse solo. La razón: crates.io **no permite borrar** una versión publicada, solo yank, así que sin la guarda la primera release dejaría un `0.1.0` de `hello world` ahí para siempre. **Borra esa línea cuando el crate merezca publicarse.**

Y la imagen: el job `image` se salta mientras no existan `Dockerfile` y `ui/package.json`. Los dos se encienden solos, sin editar los workflows.

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

## CI

| Workflow | Cuándo | Qué |
|---|---|---|
| `ci.yml` | PR y push a `development`/`main` | `preflight` → `backend` (fmt, clippy, test) → `frontend` (se salta mientras no exista `ui/`) |
| `release-prepare.yml` | push a `main` | versión, changelog y tag |
| `release.yml` | tag `v*` | imagen a GHCR, crate a crates.io, GitHub Release |

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
