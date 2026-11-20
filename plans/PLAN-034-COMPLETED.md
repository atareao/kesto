# PLAN-034 — Arreglo del flake e2e: `POST /api/v1/repositories` → 502 bajo carga

- **Issue:** #141
- **Change OpenSpec:** `openspec/changes/arreglo-flake-e2e-141/`
- **Capability:** `arnes-de-pruebas` (delta de tipo ADDED)
- **Estado:** en curso
- **Fecha:** 2026-11-20

## Objetivo

Eliminar el flake de la batería e2e en la que el alta de un repositorio falla con `502` cuando los escenarios corren en paralelo, sin tocar código de producto.

## Diagnóstico (cerrado, no repetir)

- **Causa raíz:** el tope fijo `TOPE_DEL_DESTINO = 10 s` (`src/rustic/repositorio.rs:1247`), aplicado a toda apertura/init del destino (`repositorio.rs:532`), se queda corto bajo carga. NO es readiness: la sonda `esperar_el_s3` (`tests/e2e/arnes.rs:870`) y el `stat` de `config` responden bien (`404` correcto). Lo que se pasa del plazo es el **`init` de rustic** (Argon2 + escritura de config/keys/índice) contra un SeaweedFS recién arrancado: ~8 s con 4 escenarios, >10 s con ~16. Al agotarse → `Error::DestinoNoAlcanzable` → `502` (`src/error.rs:185`).
- **¿Fila parcial?** No: en el alta la fila va DESPUÉS del destino (`abrir_repositorio` `src/api/repositories.rs:548` precede al `INSERT` `:553`). Un `502` = 0 filas → **reintentar es seguro**.
- **Reproducción:** `./target/debug/deps/e2e-*` (16 hilos) → 14/23 fallan con `502`; con `--test-threads=8` y 5 escenarios pasan.
- **Suite oficial** = `--test-threads=1` (`just e2e`, job e2e del CI): por eso el CI no lo reproduce.

## Enfoque

Reintento acotado (2–3 intentos, solo ante `502`) con backoff, encapsulado en un bucle puro y testeable del arnés y usado por `crear_repositorio`.

Descartado:
- endurecer `esperar_el_s3` (no es el problema);
- subir `TOPE_DEL_DESTINO` (política de producto del #100);
- semáforo que limite los `Mundo::nuevo` concurrentes (más invasivo).

## Ficheros implicados

- `tests/e2e/arnes.rs` — helper de reintento y `crear_repositorio` (único cambio de código).
- `openspec/changes/arreglo-flake-e2e-141/` — propuesta, tareas y delta de spec.
- `docs/FEATURE-034-arreglo-flake-e2e-141.md` — documentación de la feature (al cerrar).

## Tareas

Ver `openspec/changes/arreglo-flake-e2e-141/tasks.md` (TDD RED → GREEN → REFACTOR + verificación + cierre).

## Criterios de aceptación

1. Test unitario del bucle de reintento: `502`→`200` reintenta y devuelve datos; `502` persistente falla en voz alta; `2xx` no reintenta.
2. `cargo test --locked --features e2e --all-targets` en paralelo → 0 fallos.
3. `just e2e` (serie) sigue verde.
4. `just check` en verde (fmt + clippy + tests).
5. Sin cambios en producto, contrato HTTP, base de datos ni panel.

## Verificación

`just check` + `cargo test --locked --features e2e --all-targets` (paralelo) + `just e2e` (serie).
