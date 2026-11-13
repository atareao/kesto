# PLAN-011 — Acelerar el build de la imagen de release

- **Issue:** [#38](https://github.com/atareao/kesto/issues/38)
- **Change OpenSpec:** `openspec/changes/release-image-rapida/`
- **Estado:** en curso

## Objetivo

Reducir de ~2 h a un objetivo de <45 min el job `image` de `.github/workflows/release.yml`, sin cambiar el contrato externo del registro (mismos tags, mismo manifest list amd64+arm64, mismo smoke test, mismo guard de `latest`).

## Diagnóstico

- Job único multi-arquitectura (`platforms: linux/amd64,linux/arm64`): la capa arm64 se compila **emulada** sobre runner x64.
- `Dockerfile` recompila UI y Rust **desde cero** por arquitectura; sin caché de dependencias Rust.
- `cache-to: type=gha,mode=max` exporta toda la caché cada run.
- Medido: v0.3.1 ≈ 2 h 24 min; v0.3.2 ≈ 2 h 20 min; v0.3.3 ≈ 2 h 25 min; v0.4.0 ≈ 1 h 44 min.

## Alcance

1. **Runners ARM nativos** (`ubuntu-26.04-arm`, gratis en repos públicos) en vez de emulación QEMU.
2. **Matriz amd64/arm64**: cada job construye solo su plataforma y publica `push-by-digest`; job final ensambla el manifest list con `docker buildx imagetools create` usando los tags de `docker/metadata-action`. Smoke test se mantiene en amd64.
3. **Caché de compilación Rust**: capa de dependencias cacheada (dummy `src/main.rs` + `cargo build --release` de deps antes del `COPY` del código real) y evaluación de `sccache`.

## Fuera de alcance

- Firmas/imágenes cosign, SBOM, provenance ampliado.
- Cambios en el binario, la API, el panel o el `compose.yaml`.
- Cambiar el contrato de tags o el guard `scripts/latest-tag-guard.sh`.

## Tareas

- [ ] Aprobar el change proposal `release-image-rapida`.
- [ ] Rama `feature/release-image-rapida` desde `development`.
- [ ] Refactorizar `.github/workflows/release.yml`: matriz por arquitectura + job de ensamblado de manifest.
- [ ] Mejorar caché en `Dockerfile` (capa de dependencias Rust).
- [ ] Validar YAML y tests del guard.
- [ ] Release de prueba y verificar con `podman manifest inspect` que `latest` == `X.Y.Z` y contiene amd64+arm64.
- [ ] `docs/FEATURE-011-<slug>.md` y cierre del plan.

## Criterios de aceptación

- El manifest publicado contiene amd64 **y** arm64 nativos; `X.Y.Z`, `X.Y` y `latest` resuelven al mismo manifest list.
- El guard de `latest` sigue vigente y verde.
- El smoke test pasa en la imagen publicada.
- Tiempo del job de imagen reducido significativamente (objetivo <45 min).

## Riesgos

- Reutilización de la caché `type=gha` entre jobs de arquitecturas distintas (usar scope por arquitectura).
- `docker buildx imagetools create` debe ejecutarse tras AMBOS digests; cuidado con el `latest` guard y con la concurrencia.
- Disponibilidad/cola de los runners `ubuntu-26.04-arm` (preview). Fallback: `ubuntu-24.04-arm`.
