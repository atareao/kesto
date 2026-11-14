# Relevo — FEATURE-013 (HSTS + backup) en development; #11 pendiente

`main` en **0.5.2** (`19d5c2f`); `development` en **`7304485`** (FEATURE-013 + este relevo, **sin releasear**). Issues **#25**, **#22**, **#41**, **#38**, **#44** cerrados en esta sesión.

## Lo hecho en esta sesión

### 1. Housekeeping

- **#25** (publicar el crate) y **#22** (override de minimatch) ya estaban **resueltos**: se cerraron con la evidencia (crate `kesto` en crates.io hasta 0.5.2; override en `ui/pnpm-workspace.yaml`, minimatch → 11.0.8, sin alertas abiertas).
- Borradas 5 ramas remotas de PRs ya mergeados.

### 2. FEATURE-013 — HSTS opcional + backup del volumen de datos (#44)

- Change `deploy-endurecimiento` (SDD), TDD RED→GREEN, PR **#45** mergeado a `development`.
- **HSTS:** variable opcional `KESTO_HSTS_MAX_AGE` (entero ≥ 0). Ausente → sin cabecera (defecto; la HSTS del proxy); presente → toda respuesta lleva `Strict-Transport-Security: max-age=<n>`, incluidos los `404`/`405` (la capa va en el router raíz). Vacía o no numérica → error de arranque.
- **Backup:** receta `just backup-datos [destino]` (localiza el volumen `kesto_data`, para el contenedor, exporta y rearranca). Recuperación documentada en `docs/FEATURE-013-deploy-endurecimiento.md`.
- `just check` verde (**431** passed, +6); `openspec validate --specs --strict` → 12 passed.

## Verificado

- `just check` verde; la receta `backup-datos` probada contra un volumen real (con y sin contenedor en marcha): produce `.tar.gz` con `kesto.db`+`-wal` y deja el contenedor arriba.
- CI del PR #45 en verde.

## Pendiente / siguiente

1. **Release:** FEATURE-013 vive en `development`; el próximo merge `development → main` la publica (bump automático por los conventional commits).
2. **#11 — SQLite como motor de copia:** feature grande (nuevo motor de extracción; toca `src/engine/`, `src/rustic/`, el esquema de `jobs`/`targets` y el panel). **Recomendado abrirla en sesión nueva** con su ciclo SDD completo.
3. Recordatorios heredados: `podman run` no re-pule si la imagen existe en local (forzar `podman pull` al verificar tags); la prueba del socket real no se puede ejecutar aquí (sin daemon accesible en el CI).
