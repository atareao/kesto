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

En corto: `just check` encadena los tres del backend en el orden exacto del CI, y `just check-all` añade los cuatro del panel. Para aplicar formato en vez de comprobarlo: `just fmt-fix`, `just justfile-fmt` y `pnpm run lint` dentro de `ui/`.

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
- Automatizado en el `.justfile`: `just plans`, `just plan-new`, `just plan-done N`, `just plan-pending N` y `just feature-new <slug>`.
- Convención completa: [AGENTS.md](./AGENTS.md).

## Resumen visual

```text
main        ──●───────────────────────●────────────
               \                      /
development ────●──●──●──●──●──●──────●
                 \    /   \  /
feature           ●──●     ●─●
```

## Detalle íntegro procedente de `AGENTS.md`

> Bloques **literales** que estaban en el `AGENTS.md` de Kesto y se movieron aquí en la poda v2 (2026-10-31). El `AGENTS.md` conserva las reglas imperativas y el puntero a este documento. **No se crea `docs/PIPELINE.md`**: todo el detalle de git/release vive aquí.

## Git Flow — obligatorio

**Este proyecto usa gitflow siempre: nada se commitea directamente en `main` ni en `development`.**

| Rama | Base | Uso |
|---|---|---|
| `main` | — | Producción. Un merge aquí **dispara una release**. |
| `development` | `main` | Integración (rama activa de trabajo) |
| `feature/<kebab-case>` | `development` | Nuevas funcionalidades |
| `hotfix/<kebab-case>` | `main` | Correcciones urgentes sobre producción |

```bash
git checkout development && git pull --ff-only
git checkout -b feature/<nombre>
# ... trabajo ...
git push -u origin feature/<nombre>
gh pr create --base development --head feature/<nombre>
```

- Mensajes: conventional commits con gitmoji (`✨ feat:`, `🐛 fix:`, `🚑️ hotfix:`, `📝 docs:`, `✅ test:`, `🔧 chore:`…). Flujo completo y tabla de gitmoji en `GIT_FLOW.md`; automatización en la skill `gitflow`.
- **Ojo con la skill `gitflow`**: sus pasos de formateo asumen `backend/` y `frontend/`. Aquí el layout es `src/` + `ui/`: adapta los comandos, no los directorios.
- Formateo **antes** de cada commit: `cargo fmt --all` → `cargo clippy -- -D warnings` → `cargo test`; en `ui/`: `pnpm run lint` → `npx tsc --noEmit` → `pnpm test`. Si el formateo toca ficheros, commit aparte (`🎨 style: apply formatting`).
- Nunca hagas `git commit` directo en `main`: ahí solo entran PRs de release desde `development`.

## Pipeline (GitHub Actions)

| Workflow | Se dispara con | Hace |
|---|---|---|
| `ci.yml` | PR y push a `development`/`main` | `preflight` detecta el árbol → `backend` (`fmt --check`, `clippy -D warnings`, `test --locked`) → `frontend` (npm o pnpm según el lockfile) |
| `release-prepare.yml` | push a `main` (solo eso) | bump con vampus, CHANGELOG con git-cliff, tag `vX.Y.Z`, sincroniza `development` |
| `release.yml` | tag `v*` | imagen multi-arquitectura a `ghcr.io/atareao/kesto` + crate a crates.io + GitHub Release |

Lo que hay que saber antes de tocarlo:

- **Un merge a `main` ES una release.** No se etiqueta a mano: `release-prepare` decide el bump desde los conventional commits (`feat` → minor, `!:` / `BREAKING CHANGE` → major, resto → patch) y empuja el tag, que es lo que dispara `release.yml`.
- **Dos guardas deliberadas** para no publicar basura, ambas auto-activables: la imagen se salta mientras no existan `Dockerfile` y `ui/package.json`; el crate se salta mientras `Cargo.toml` diga `publish = false`. **Quita `publish = false` cuando el crate merezca publicarse**: crates.io no permite borrar versiones, solo yank.
- **El job `frontend` del CI se salta solo** mientras no exista `ui/package.json`, y elige npm o pnpm por el lockfile presente. No hace falta tocarlo cuando llegue la UI.
- **Secretos necesarios antes de la primera release**: `GH_PAT` (PAT con `contents: write`) y `CARGO_REGISTRY_TOKEN`. Sin `GH_PAT` el tag no dispara `release.yml`, porque un tag empujado con el `GITHUB_TOKEN` por defecto no dispara workflows.
- La versión vive en **tres sitios** que deben ir sincronizados: `Cargo.toml`, la entrada `[[package]]` de `Cargo.lock` y `.vampus.yml:current_version`. `release-prepare` los resincroniza y falla en alto si vampus no sube nada; si lo tocas a mano, toca los tres.
- El CI corre exactamente `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` y `cargo test --locked`: replica esos comandos antes de cada push para no descubrir el fallo en el PR.

## Planes y documentación — regla fija

Dos directorios en la raíz, siempre:

**1) `plans/`** — un fichero por plan, correlativo de tres dígitos y sin huecos (el siguiente número = mayor existente + 1; `plans/` es la única fuente del contador):

| Estado | Nombre |
|---|---|
| En curso | `plans/PLAN-001.md` |
| Terminado | `plans/PLAN-001-COMPLETED.md` |
| Aparcado | `plans/PLAN-001-PENDING.md` |

Los planes **se renombran, nunca se borran** (`git mv`). El renombrado al cerrar o aparcar lo haces tú al terminar, no el subagente que escribe el plan.

**2) `docs/`** — un documento por feature entregada, correlativo y emparejado con su plan: `docs/FEATURE-001-<slug>.md` (slug en kebab-case). `docs/DEFINITION.md` es el SRS de referencia y no se mezcla con las features.

Ciclo de vida de una feature: change proposal de OpenSpec aprobado → `plans/PLAN-NNN.md` → implementación TDD → al cerrar, `docs/FEATURE-NNN-<slug>.md` con lo que realmente quedó construido (contratos, endpoints, esquema, decisiones y deuda pendiente) → `plans/PLAN-NNN-COMPLETED.md`.

⚠️ **`@plan-writer` escribe `PLAN.md` en la raíz por defecto.** En este repo los planes van a `plans/PLAN-NNN.md`: pásale la ruta de destino explícita y el contenido completo del plan en el prompt.

