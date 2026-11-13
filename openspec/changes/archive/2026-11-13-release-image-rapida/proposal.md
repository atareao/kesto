# Proposal

## Why

El job `image` de `.github/workflows/release.yml` publica `ghcr.io/atareao/kesto` en **un solo job multi-arquitectura** (`platforms: linux/amd64,linux/arm64`). La capa `arm64` se compila **emulada** sobre el runner x64 y el `Dockerfile` recompila la UI y todo el Rust **desde cero por arquitectura**, sin caché de compilación. Resultado medido: builds de **~1 h 44 min a 2 h 25 min** por release (v0.3.1–v0.4.0). Ese tiempo es lo que hace que las releases se solapen y compliquen el guard de `latest`.

Issue de entrada: [#38](https://github.com/atareao/kesto/issues/38).
Plan: `plans/PLAN-011.md`.

## What Changes

- El job de imagen pasa a una **matriz por arquitectura**: amd64 corre en `ubuntu-26.04` y arm64 en **`ubuntu-26.04-arm`** (runner ARM nativo). Cada job construye **solo su plataforma**, publicando **por digest** (`push-by-digest=true`, `name-canonical=true`). Desaparece la emulación por completo.
- Un job final **ensambla el manifest list** con `docker buildx imagetools create`, aplicando sobre los dos digests los tags que calcula `docker/metadata-action` (`X.Y.Z`, `X.Y`, `latest`).
- El **guard de `latest`** (`scripts/latest-tag-guard.sh`) y el **smoke test** (arranque del contenedor) se mantienen; el smoke test corre en el job amd64.
- **Caché de compilación Rust en dos niveles:** (1) capa de dependencias del `Dockerfile` (patrón dummy `src/main.rs` + `cargo build --release` de las dependencias antes de copiar el código real) y (2) **`sccache`** con backend de caché de GitHub Actions (`SCCACHE_GHA_ENABLED=true`), inyectando `ACTIONS_CACHE_URL` y `ACTIONS_RUNTIME_TOKEN` al build mediante `secrets` de BuildKit.
- No se toca código Rust ni el panel: es el pipeline de publicación.

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `deploy`: se añade el requisito de que la imagen multi-arquitectura se construya de forma **nativa por arquitectura**, de que el manifest publicado contenga amd64 y arm64 bajo los mismos tags, y de que la compilación Rust se **cachee entre ejecuciones**, sin alterar el contrato externo ni el guard de `latest`.

## Impact

- **Ficheros:** `.github/workflows/release.yml`, `Dockerfile`, `docs/FEATURE-011-<slug>.md`.
- **Registro:** GHCR — mismos tags (`X.Y.Z`, `X.Y`, `latest`), mismo manifest list.
- **Sin cambios** en la imagen funcional, el binario, la API, el panel, el `compose.yaml` ni el guard.
