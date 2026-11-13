# Tasks

## SDD

- [x] Aprobar este change proposal (`release-image-rapida`) — aprobado con sccache y runner `ubuntu-26.04-arm`.

## Preparación

- [x] Crear rama `feature/release-image-rapida` desde `development`.

## Implementación

- [x] Refactorizar `.github/workflows/release.yml`:
  - [x] matriz por arquitectura (`amd64` → `ubuntu-26.04`, `arm64` → `ubuntu-26.04-arm`),
  - [x] `push-by-digest=true` + `push=true`, scope de caché `type=gha` por arquitectura,
  - [x] smoke test contra la imagen publicada por digest (amd64),
  - [x] job final `manifest` con `docker buildx imagetools create` aplicando los tags de `docker/metadata-action`,
  - [x] pasar `ACTIONS_CACHE_URL` y `ACTIONS_RUNTIME_TOKEN` como `secrets` de BuildKit para `sccache`.
- [x] `Dockerfile`: instalar `sccache` en `rust-builder`, `RUSTC_WRAPPER=sccache`, `SCCACHE_GHA_ENABLED` y cachear la capa de dependencias Rust.

## Verificación

- [x] `actionlint .github/workflows/release.yml` (solo el aviso preexistente `runner-label` de `ubuntu-26.04`).
- [x] Tests del guard: `scripts/latest-tag-guard.sh` siguen verdes.
- [x] `openspec validate release-image-rapida --strict`.
- [x] `just check` verde (425 + 5) y `just scripts-test` verde.
- [ ] Release de prueba: verificar con `podman manifest inspect ghcr.io/atareao/kesto:latest` que `latest` == `X.Y.Z` y contiene `amd64` + `arm64`. *(diferido a la primera release)*
- [ ] Medir el tiempo del job de imagen (objetivo: <45 min). *(diferido a la primera release)*

## Cierre

- [x] `docs/FEATURE-011-release-image-rapida.md`.
- [x] Renombrar `plans/PLAN-011.md` → `PLAN-011-COMPLETED.md`.
- [x] Archivar el change (`openspec archive release-image-rapida`).
