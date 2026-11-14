# Proposal

## Why

Quedan dos *follow-ups* de la feature de despliegue (FEATURE-008) sin cerrar:

1. **HSTS:** en el despliegue estándar el TLS lo termina un proxy, así que la cabecera `Strict-Transport-Security` es del proxy. Pero no hay forma de que la propia aplicación la emita si el operador lo quiere, y el SRS pide *considerarla*.
2. **Backup del volumen de datos:** el volumen nombrado `kesto_data` (la SQLite) persiste, pero no hay un procedimiento ni una receta para respaldarlo y recuperarlo.

Issue de entrada: [#44](https://github.com/atareao/kesto/issues/44). Plan: `plans/PLAN-013.md`.

## What Changes

- Nueva variable **opcional** `KESTO_HSTS_MAX_AGE` (segundos, entero). Ausente → **sin cabecera** (defecto). Con valor `>= 0`, toda respuesta lleva `Strict-Transport-Security: max-age=<n>`. Presente pero vacía o no numérica → **error de arranque**, coherente con `BIND_ADDRESS`/`RUST_LOG`.
- Nueva receta `just backup-datos <destino>`: exporta el volumen `kesto_data` de forma consistente (parar el contenedor → exportar → arrancar) a un `.tar.gz`, más documentación del procedimiento y de su recuperación.
- La capa de cabecera usa `tower-http` con la feature `set-header` (ya presente).

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `deploy`: la aplicación SHALL poder emitir `Strict-Transport-Security` de forma opcional y configurable, y SHALL existir un procedimiento reproducible para respaldar el volumen de datos.

## Impact

- **Ficheros:** `src/config.rs`, `src/api/mod.rs`, `.justfile`, `compose.yaml`, `.env.example`, `docs/ENTORNO.md`, `docs/FEATURE-013-*.md`.
- Sin cambios en el contrato externo de la API salvo la cabecera opcional.
