# Tasks

## SDD

- [x] Aprobar este change proposal (`release-image-rapida`) — aprobado con sccache y runner `ubuntu-26.04-arm`.

## Preparación

- [x] Crear rama `feature/release-image-rapida` desde `development`.

## Implementación

- [ ] Refactorizar `.github/workflows/release.yml`:
  - [ ] matriz por arquitectura (`amd64` → `ubuntu-26.04`, `arm64` → `ubuntu-26.04-arm`),
  - [ ] `push-by-digest=true`, `name-canonical=true`, scope de caché `type=gha` por arquitectura,
  - [ ] smoke test en el job amd64,
  - [ ] job final `manifest` con `docker buildx imagetools create` aplicando los tags de `docker/metadata-action`,
  - [ ] pasar `ACTIONS_CACHE_URL` y `ACTIONS_RUNTIME_TOKEN` como `secrets` de BuildKit para `sccache`.
- [ ] `Dockerfile`: instalar `sccache` en `rust-builder`, `RUSTC_WRAPPER=sccache`, `SCCACHE_GHA_ENABLED=true`, y cachear la capa de dependencias Rust (dummy `src/main.rs` + `cargo build --release` antes del `COPY` real).

## Verificación

- [ ] `actionlint .github/workflows/release.yml` (si está disponible).
- [ ] Tests del guard: `scripts/latest-tag-guard.sh` siguen verdes.
- [ ] `openspec validate release-image-rapida --strict`.
- [ ] `just check` y `just check-all` (sin regresiones).
- [ ] Release de prueba: verificar con `podman manifest inspect ghcr.io/atareao/kesto:latest` que `latest` == `X.Y.Z` y contiene `amd64` + `arm64`.
- [ ] Medir el tiempo del job de imagen (objetivo: <45 min).

## Cierre

- [ ] `docs/FEATURE-011-<slug>.md`.
- [ ] Renombrar `plans/PLAN-011.md` → `PLAN-011-COMPLETED.md`.
- [ ] Archivar el change (`openspec archive release-image-rapida`).
