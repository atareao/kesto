# FEATURE-011 — Imagen de release rápida: construcción nativa por arquitectura

**Issue:** [#38](https://github.com/atareao/kesto/issues/38) · **Plan:** `plans/PLAN-011-COMPLETED.md` · **Change OpenSpec:** `openspec/changes/archive/2026-11-13-release-image-rapida/`

## Qué es

El job `image` del release publicaba la imagen de contenedor en **un único job multi-arquitectura** (`platforms: linux/amd64,linux/arm64`): la capa arm64 se compilaba **emulada** sobre el runner x64 y el `Dockerfile` recompilaba la UI y todo el Rust **desde cero por arquitectura**, sin caché. Medido en las releases previas: **~1 h 44 min a 2 h 25 min** por release (v0.3.1–v0.4.0).

Esta feature desdobla ese job en una **construcción nativa por arquitectura** más un ensamblado de manifest list, y añade **caché de compilación** en dos niveles. Objetivo: bajar el job por debajo de 45 min, sin cambiar el contrato externo del registro.

## Ficheros implicados

- `.github/workflows/release.yml` — jobs `image` (matriz), `smoke` y `manifest`.
- `Dockerfile` — etapa `rust-builder`: `sccache` + capa de dependencias.
- `scripts/release-image-workflow.test.sh` — prueba estructural del pipeline y del Dockerfile.
- `.justfile` — receta `scripts-test`.
- `.github/workflows/ci.yml` — paso `Scripts tests` en el job `backend`.

## Cómo funciona

1. Un tag `vX.Y.Z` entra en `preflight`, que detecta que hay imagen; el job `image` arranca en **matriz**:
   - `amd64` → `ubuntu-26.04`, `platform: linux/amd64`
   - `arm64` → `ubuntu-26.04-arm` (**nativo**, sin QEMU), `platform: linux/arm64`
2. Cada job construye **solo su plataforma** con `docker/build-push-action` y `outputs: type=image,name=ghcr.io/<repo>,push-by-digest=true,push=true`: sube la imagen **por digest**, sin etiquetas. La caché `type=gha` usa un `scope` por arquitectura.
3. Cada job exporta su digest a un artefacto `digests-<arch>` (`actions/upload-artifact`).
4. El job `smoke` (amd64) descarga `digests-amd64`, hace `docker run` de `ghcr.io/<repo>@sha256:<digest>` y espera respuesta HTTP en `:8080`.
5. El job `manifest` descarga ambos digests, calcula el guard de `latest` (`scripts/latest-tag-guard.sh`), deja que `docker/metadata-action` compute los tags (`X.Y.Z`, `X.Y`, `latest`) y ensambla el manifest list con `docker buildx imagetools create`.
6. En el `Dockerfile`, la etapa `rust-builder` instala `sccache`, compila las dependencias contra un `main.rs` vacío (capa cacheable) y luego el crate real; `sccache` cachea la compilación en la caché de GitHub Actions.

## Variables y configuración

- `SCCACHE_VERSION` (build-arg del `Dockerfile`, defecto `0.19.0`).
- `ACTIONS_CACHE_URL` y `ACTIONS_RUNTIME_TOKEN`: los aporta el runner; `release.yml` los copia a `$GITHUB_ENV` y los pasa al build como **secretos de BuildKit** (`actions_cache_url`, `actions_runtime_token`). Sin ellos, `SCCACHE_GHA_ENABLED` queda apagado y el build funciona igual, pero sin caché.
- Nada nuevo para el operador: el `compose.yaml` del §8 sigue igual.

## Cómo habilitar / desplegar

No requiere configuración. Un merge a `main` dispara `release-prepare` (bump + tag) y, con el tag, el `release.yml` nuevo entra solo. En local, `podman build` sigue funcionando: sin secretos, `sccache` queda desactivado.

## Decisiones

- **Runners ARM nativos** en lugar de emulación QEMU: `ubuntu-26.04-arm` es gratis en repositorios públicos y evita la penalización de emulación.
- **`sccache` con backend de la caché de Actions** además de la capa de dependencias del `Dockerfile`: la capa cubre el caso «dependencias sin cambios», `sccache` cubre el resto entre runs.
- **Smoke test contra la imagen publicada** (por digest) en vez de contra una build local amd64: se prueba exactamente el artefacto que se publica.
- **`scope` de caché por arquitectura** en `type=gha` para no mezclar capas de arch distintas.

## Follow-ups

- **Verificación end-to-end (pendiente):** en la primera release con este pipeline, comprobar con `podman manifest inspect ghcr.io/atareao/kesto:latest` que `latest` == `X.Y.Z` y que el manifest contiene `amd64` + `arm64`, y medir el tiempo del job (objetivo <45 min).
- `actionlint` marca `ubuntu-26.04` como etiqueta desconocida porque su lista aún no la incluye; no es un fallo (ya ocurría antes de esta feature).
