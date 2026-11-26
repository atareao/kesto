# Tasks — saneamiento de dependencias y vigilancia de la cadena de suministro

Issue **#167 (SEC-11)**. Rama `feature/saneamiento-de-dependencias` (desde `development`).

## 0. Preparación

- [x] 0.1 Rama `feature/saneamiento-de-dependencias` desde `development`.
- [x] 0.2 Change `saneamiento-de-dependencias` en `openspec/changes/` (proposal, design, tasks y los deltas de `cadena-de-suministro` y `db`).
- [x] 0.3 `plans/PLAN-044.md` con las tareas de este fichero.
- [x] 0.4 `openspec validate --strict saneamiento-de-dependencias` válido (verificación: termina sin error).

## 1. RED — la puerta y su cableado, que hoy no existen

- [x] 1.1 `scripts/dependencias.test.sh`: prueba estructural del cableado (verificación: falla hoy, porque `deny.toml`, el trabajo del CI y `.github/dependabot.yml` no existen).
- [x] 1.2 Prueba de caracterización del checksum de una migración en `src/db/migrate.rs` (verificación: verde antes y después del salto).
- [x] 1.3 Verificar el rojo: `just scripts-test` falla en `dependencias.test.sh` y el resto sigue verde.

## 2. GREEN — `deny.toml` y la receta

- [x] 2.1 `deny.toml` con las cuatro familias: advisories (**siete** excepciones con motivo: tres avisos no alcanzables y cuatro crates *unmaintained* sin versión que los cierre), licencias (allowlist explícita), prohibiciones (comodines denegados, duplicidades avisadas) y fuentes (solo crates.io).
- [x] 2.2 Receta `just deny` y su encaje en `check-all` (verificación: `just --list` la muestra).
- [x] 2.3 `cargo deny check licenses bans sources` en verde con el `deny.toml` nuevo (verificación: termina sin error). La comprobación **entera**, `cargo deny check`, no puede estar verde hasta la 4.5: el CVE de SQLite sigue rojo hasta el salto de `sqlx`.
- [x] 2.4 El script estructural pasa (verificación: `just scripts-test` sin error).
- [x] 2.5 `docs/COMANDOS.md` documenta la receta `deny` y su coste (necesita red la primera vez).
- [x] 2.6 `set -e` en la receta `scripts-test` del `.justfile`, que enmascaraba el fallo de un script con el verde del último (verificación: la comprobación **D** del script estructural pasa).
- [x] 2.7 `--workspace` en la invocación de `cargo deny` —job del CI y receta `deny`—: sin él el grafo arranca en el paquete raíz y `kesto-mcp`, con `rmcp` dentro, quedaba fuera de la puerta, que salía verde sin mirarlo (D11). Lo destapó la revisión del bloque; lo vigilan las comprobaciones **B** y **E** de `scripts/dependencias.test.sh`.

## 3. GREEN — los saltos baratos

- [x] 3.1 `zstd 0.13.4` (arrastra `zstd-safe 7.4.x`) (verificación: los avisos de zstd y zstd-safe desaparecen de `cargo deny check advisories`).
- [x] 3.2 `rmcp 3.11.0` en `kesto-mcp`, comprobando que `transport-io` y `transport-streamable-http-server` siguen existiendo (verificación: las pruebas de `kesto-mcp` pasan).
- [x] 3.3 `async-executor 1.14.1` (verificación: deja de estar *yanked* en `cargo deny check advisories`).
- [x] 3.4 `cargo test --locked --workspace` en verde.

## 4. GREEN — `sqlx 0.9.1` y `libsqlite3-sys` saneado

- [x] 4.1 `Cargo.toml`: `sqlx 0.9.1`, features con `sqlite-bundled` en vez de `sqlite`, `rust-version = "1.94"` y el bloque de comentarios actualizado con el motivo nuevo.
- [x] 4.2 `libsqlite3-sys` a `0.40.0` dentro del rango que abre `sqlx-sqlite 0.9.1` (verificación: `cargo tree -i libsqlite3-sys` lo muestra).
- [x] 4.3 Los tres puntos de API que señala el compilador: el literal `Migrator` de `src/test_support.rs`, su `Migration::new` y los dos `query*()` con cadena no estática. (El compilador señala además `run_direct`, que en `0.9` pasa a `(Option<i64>, &mut C, bool)`: `None` y `false` son el par que `run` le pasa.)
- [x] 4.4 La caracterización del checksum sigue verde (verificación: la prueba pasa con la versión nueva).
- [x] 4.5 `cargo test --locked --workspace` en verde y `cargo deny check` **entero** en verde, con una excepción menos (verificación: RUSTSEC-2026-0459 ya no aparece y el trabajo de dependencias pasa completo). Nota: RUSTSEC-2026-0459 **no estaba** en el `ignore` —las siete excepciones son otros avisos—, así que desaparece por el salto y no hay entrada que retirar. El salto sube además el SQLite empaquetado de 3.46.0 a 3.54.1, y eso movió el planificador bajo `ANALYZE`: la caracterización del plan de la `0004` se volvió a medir y pasa a fijar el acceso indexado (`!SCAN e`) en vez de una puerta concreta, con las aserciones del bloque sin estadísticas y del filtro suelto intactas (D10). Mordida comprobada: con un `DROP INDEX idx_job_executions_snapshot_id` tras el migrador la prueba se pone roja.

## 5. GREEN — CI y Dependabot

- [x] 5.1 Job `Dependencies (cargo deny)` en `ci.yml`, con `cargo-deny` por `taiki-e/install-action` y `--locked`.
- [x] 5.2 `--workspace` en los pasos de análisis y de prueba del CI.
- [x] 5.3 `.github/dependabot.yml` con `cargo`, `github-actions` y `npm` en `/ui`.
- [x] 5.4 El script estructural pasa (verificación: `bash scripts/dependencias.test.sh` termina con `0`). `just check-all` queda en verde al cerrar las tareas 3 y 4, que son las que apagan los avisos alcanzables.

## 6. Cierre

- [x] 6.1 `just e2e` en verde.
- [x] 6.2 `docs/FEATURE-044-saneamiento-de-dependencias.md`.
- [x] 6.3 `docs/ENTORNO.md` (versiones y MSRV), `GIT_FLOW.md` (la tabla del CI) y `AGENTS.md` (el gate y la paridad del workspace).
- [x] 6.4 Revisión de `@rust-reviewer` sin bloqueantes (verificación: las observaciones aplicadas o contestadas). Las cinco observaciones se aplicaron: (1) los dos ayudantes de pruebas a `&'static str`; (2) el `assert!` de `ANALYZE` invita a re-medir; (3) D9, coherente en pasado; (4) comentario de Dependabot ajustado; (5) toolchain fijada en el job `dependencies`.
- [x] 6.5 PR a `development` con `Closes #167`; CI en verde; fusionar y cerrar #167 a mano.
- [x] 6.6 `just spec-archive saneamiento-de-dependencias` (verificación: el delta entra en `openspec/specs/`).
- [x] 6.7 `plans/PLAN-044.md` → `plans/PLAN-044-COMPLETED.md` (verificación: `git mv` hecho).
- [x] 6.8 PR del archivado a `development`.
