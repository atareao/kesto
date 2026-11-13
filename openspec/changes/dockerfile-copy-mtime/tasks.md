# Tasks

## SDD

- [ ] Aprobar este change proposal (`dockerfile-copy-mtime`).

## Preparación

- [ ] Crear rama `hotfix/dockerfile-copy-mtime` desde `main` (gitflow: `main` está afectado).

## TDD

- [ ] RED: añadir a `scripts/release-image-workflow.test.sh` la comprobación de que el `Dockerfile` adelanta el mtime (`find src -type f -exec touch`); el test debe fallar.
- [ ] GREEN: añadir `RUN find src -type f -exec touch {} +` antes del build real en el `Dockerfile`; el test debe pasar.

## Verificación

- [ ] `just scripts-test` y `just check` verdes.
- [ ] Build local de la imagen (`podman build`) y comprobar que el binario arranca.
- [ ] Release nueva y verificar `podman manifest inspect ghcr.io/atareao/kesto:latest` == `:X.Y.Z` con `amd64` + `arm64` y smoke test verde.

## Cierre

- [ ] `docs/FEATURE-012-<slug>.md`.
- [ ] Renombrar `plans/PLAN-012.md` → `PLAN-012-COMPLETED.md`.
- [ ] Archivar el change (`openspec archive dockerfile-copy-mtime`).
