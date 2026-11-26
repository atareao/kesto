# Propuesta: Migrar de chrono/chrono-tz a time o jiff en kesto

## Issue
- #185: Migrar de chrono/chrono-tz a time o jiff (salida de dos excepciones de deny.toml)

## Plan
- `plans/PLAN-053.md` (108 líneas)

## Intención
Eliminar las dependencias `chrono` y `chrono-tz` del crate kesto, reemplazarlas con `time` (más cercano a chrono y ya usado en sqlx) o `jiff` (sucesor recomendado), y eliminar las dos excepciones declaradas en `deny.toml` (RUSTSEC-2026-0543 y RUSTSEC-2026-0555). Mantener la excepción en `deny.toml` solo si chrono sigue presente vía la cadena FTP (suppaftp → opendal → rustic_backend); documentar claramente por qué sigue siendo necesaria.

## Alcance
- Backend Scheduler: migrar 8 referencias a chrono/chrono-tz en `src/scheduler/validacion.rs`, `src/scheduler/mod.rs`, `src/scheduler/planificacion.rs`.
- Cargo.toml: eliminar `chrono` y `chrono-tz` de `dependencies` y `dev-dependencies`.
- deny.toml: eliminar las dos excepciones (RUSTSEC-2026-0543, RUSTSEC-2026-0555) y documentar si chrono persiste vía FTP.
- Documentación: actualizar `docs/ENTORNO.md`, `docs/FEATURE-053-chrono-migration.md`, `docs/ESTADO.md`.

## Impacto
- CI: eliminar excepciones de deny.toml, CI sigue verde.
- Dependencias: eliminar chrono, añadir time o jiff.
- Documentación: nuevas secciones, sin cambios funcionales.

## Criterios de éxito
- No hay dependencias directas de `chrono` o `chrono-tz` en el crate kesto; `chrono` solo aparece de forma transitiva vía la cadena FTP inherente a `rustic_backend` (`suppaftp → opendal-service-ftp → opendal → rustic_backend`), documentada en `deny.toml`.
- `cargo deny --workspace check` pasa sin excepciones nuevas: solo se mantiene `RUSTSEC-2026-0543`, documentada por esa cadena FTP, y se retira `RUSTSEC-2026-0555`.
- `cargo build` y `cargo test` pasan en todas las arquitecturas.
- Scheduler sigue funcionando igual (timezone, cron parsing, scheduling).
- No hay regresiones en x86_64 ni arm64.

## Riesgos
- `croner` y `tokio-cron-scheduler` son los principales anclajes; su reemplazo puede requerir reescritura del scheduler.
- `jiff` tiene API distinta a `chrono`; la migración en scheduler module puede ser costosa.
- La cadena FTP puede mantener chrono en dependencias transitivas; debe documentarse claramente por qué la excepción sigue siendo necesaria.
