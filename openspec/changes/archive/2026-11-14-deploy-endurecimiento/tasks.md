# Tasks

## SDD

- [x] Aprobar este change proposal (`deploy-endurecimiento`).

## Preparación

- [x] Crear rama `feature/deploy-endurecimiento` desde `development`.

## TDD — HSTS

- [x] RED: tests de config (ausente → `None`; válida → `Some(n)`; vacía → error; no numérica → error) y test de cabecera (configurada → presente; ausente → sin cabecera); deben fallar.
- [x] GREEN: campo `hsts_max_age` en `Config`, parseo de `KESTO_HSTS_MAX_AGE`, y capa `SetResponseHeaderLayer` en `src/api/mod.rs`; los tests deben pasar.

## TDD — backup

- [x] Receta `just backup-datos <destino>` (parar → exportar → arrancar) y su documentación.

## Verificación

- [x] `just check` verde (fmt + clippy + tests).
- [x] `just backup-datos` produce un archivo y la recuperación está documentada.

## Cierre

- [x] `docs/FEATURE-013-*.md` y actualizar `docs/ENTORNO.md` con la variable.
- [x] Renombrar `plans/PLAN-013.md` → `PLAN-013-COMPLETED.md`.
- [x] Archivar el change (`openspec archive deploy-endurecimiento`).
