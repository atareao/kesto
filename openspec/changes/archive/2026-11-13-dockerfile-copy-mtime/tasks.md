# Tasks

## SDD

- [x] Aprobar este change proposal (`dockerfile-copy-mtime`).

## Preparación

- [x] Crear rama `hotfix/dockerfile-copy-mtime` desde `main` (gitflow: `main` está afectado).

## TDD

- [x] RED: añadir a `scripts/release-image-workflow.test.sh` la comprobación de que el `Dockerfile` adelanta el mtime (`find src -type f -exec touch`); el test debe fallar.
- [x] GREEN: añadir `RUN find src -type f -exec touch {} +` antes del build real en el `Dockerfile`; el test debe pasar.

## Verificación

- [x] `just scripts-test` y `just check` verdes.
- [x] Build local de la imagen (`podman build`) y comprobar que el binario arranca.
- [x] Release nueva y verificar `podman manifest inspect ghcr.io/atareao/kesto:latest` == `:0.5.2` con `amd64` + `arm64` y smoke test verde (release v0.5.2).

## Cierre

- [x] `docs/FEATURE-012-<slug>.md`.
- [x] Renombrar `plans/PLAN-012.md` → `PLAN-012-COMPLETED.md`.
- [x] Archivar el change (`openspec archive dockerfile-copy-mtime`).
