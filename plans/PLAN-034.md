# PLAN-034 — Reactivar el build arm64 de la imagen

- **Issue**: #129 — 🔧 Reactivar el build arm64 de la imagen (pausado temporalmente)
- **Estado**: en curso
- **Rama**: `feature/reactivar-build-arm64` (desde `development`)
- **Fecha**: 2026-11-19

## Objetivo

Volver a publicar la imagen de Kesto en `linux/amd64` **y** `linux/arm64`, de modo que un `pull` en ARM deje de fallar. Hoy la matriz del job `image` de `.github/workflows/release.yml` tiene el arco arm64 comentado (issue #130), y los tags `X.Y.Z`, `X.Y` y `latest` son manifests **solo amd64**.

## Contexto

- El arco arm64 se pausó en el #130 por inestabilidad en producción. El #129 es el issue de reactivación.
- Cada arquitectura se construye en un runner **nativo** de su misma arquitectura (nada de emulación, issue #38):
  - `amd64` → `ubuntu-26.04`
  - `arm64` → `ubuntu-26.04-arm`
- El job `manifest` ya descarga `digests-*` con `merge-multiple: true`, así que **no necesita cambios**: recogerá automáticamente ambos digests cuando arm64 vuelva.
- El job `smoke` es intencionadamente **solo amd64** (`digests-amd64`); se queda como está.
- `scripts/release-image-workflow.test.sh` admite los dos estados legítimos (ACTIVO o PAUSADO) y **no requiere cambios**; sirve como verificación.

## Alcance

- `.github/workflows/release.yml` — único fichero que cambia.
- Verificación: `scripts/release-image-workflow.test.sh` + `just check-all`.

## Tareas

- [ ] 1. Descomentar las tres líneas del arco arm64 en el `include:` de la matriz del job `image` (`- arch: arm64`, `platform: linux/arm64`, `runner: ubuntu-26.04-arm`).
- [ ] 2. Sustituir el bloque de comentario `# TODO(#129): ...` por un comentario breve que documente que arm64 está activo (runner nativo, sin emulación).
- [ ] 3. Ejecutar `bash scripts/release-image-workflow.test.sh` y confirmar el mensaje «matriz con runners nativos amd64 (ubuntu-26.04) y arm64 (ubuntu-26.04-arm)».
- [ ] 4. Ejecutar `just check-all` (incluye `scripts-test`) en verde.
- [ ] 5. Commit con gitmoji: `🔧 ci(release): reactiva el build arm64 de la imagen`.
- [ ] 6. Abrir PR a `development` referenciando #129.

## Tras el merge

- [ ] 7. Lanzar una **release nueva** (merge a `main` → `release-prepare` → tag) para rehacer el manifest.
- [ ] 8. Verificar con `docker buildx imagetools inspect ghcr.io/atareao/kesto:latest` que el manifest lista `linux/amd64` y `linux/arm64`.
- [ ] 9. Cerrar el issue #129.

## Decisiones y notas

- No se toca el job `smoke` (sigue validando solo amd64) ni el job `manifest` (ya es multi-arquitectura por `digests-*`).
- No se requiere change de OpenSpec: es un cambio de infraestructura de CI, sin delta de capability (no existe spec para el pipeline de release).
