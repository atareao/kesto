# Tasks — saneamiento de dependencias y vigilancia de la cadena de suministro

Issue **#167 (SEC-11)**. Rama `feature/saneamiento-de-dependencias` (desde `development`).

## 0. Preparación

- [ ] 0.1 Rama `feature/saneamiento-de-dependencias` desde `development`.
- [ ] 0.2 Change `saneamiento-de-dependencias` en `openspec/changes/` (proposal, design, tasks y los deltas de `cadena-de-suministro` y `db`).
- [ ] 0.3 `plans/PLAN-044.md` con las tareas de este fichero.
- [ ] 0.4 `openspec validate --strict saneamiento-de-dependencias` válido (verificación: termina sin error).

## 1. RED — la puerta y su cableado, que hoy no existen

- [ ] 1.1 `scripts/dependencias.test.sh`: prueba estructural del cableado (verificación: falla hoy, porque `deny.toml`, el trabajo del CI y `.github/dependabot.yml` no existen).
- [ ] 1.2 Prueba de caracterización del checksum de una migración en `src/db/migrate.rs` (verificación: verde antes y después del salto).
- [ ] 1.3 Verificar el rojo: `just scripts-test` falla en `dependencias.test.sh` y el resto sigue verde.

## 2. GREEN — `deny.toml` y la receta

- [ ] 2.1 `deny.toml` con las cuatro familias: advisories (los tres avisos no alcanzables, con motivo), licencias (allowlist explícita), prohibiciones (comodines denegados, duplicidades avisadas) y fuentes (solo crates.io).
- [ ] 2.2 Receta `just deny` y su encaje en `check-all` (verificación: `just --list` la muestra).
- [ ] 2.3 `cargo deny check` en verde con el `deny.toml` nuevo (verificación: termina sin error).
- [ ] 2.4 El script estructural pasa (verificación: `just scripts-test` sin error).
- [ ] 2.5 `docs/COMANDOS.md` documenta la receta `deny` y su coste (necesita red la primera vez).

## 3. GREEN — los saltos baratos

- [ ] 3.1 `zstd 0.13.4` (arrastra `zstd-safe 7.4.x`) (verificación: los avisos de zstd y zstd-safe desaparecen de `cargo deny check advisories`).
- [ ] 3.2 `rmcp 3.11.0` en `kesto-mcp`, comprobando que `transport-io` y `transport-streamable-http-server` siguen existiendo (verificación: las pruebas de `kesto-mcp` pasan).
- [ ] 3.3 `async-executor 1.14.1` (verificación: deja de estar *yanked* en `cargo deny check advisories`).
- [ ] 3.4 `cargo test --locked --workspace` en verde.

## 4. GREEN — `sqlx 0.9.1` y `libsqlite3-sys` saneado

- [ ] 4.1 `Cargo.toml`: `sqlx 0.9.1`, features con `sqlite-bundled` en vez de `sqlite`, `rust-version = "1.94"` y el bloque de comentarios actualizado con el motivo nuevo.
- [ ] 4.2 `libsqlite3-sys` a `0.40.0` dentro del rango que abre `sqlx-sqlite 0.9.1` (verificación: `cargo tree -i libsqlite3-sys` lo muestra).
- [ ] 4.3 Los tres puntos de API que señala el compilador: el literal `Migrator` de `src/test_support.rs`, su `Migration::new` y los dos `query*()` con cadena no estática.
- [ ] 4.4 La caracterización del checksum sigue verde (verificación: la prueba pasa con la versión nueva).
- [ ] 4.5 `cargo test --locked --workspace` en verde y `cargo deny check advisories` con una excepción menos (verificación: RUSTSEC-2026-0459 ya no aparece).

## 5. GREEN — CI y Dependabot

- [ ] 5.1 Job `Dependencies (cargo deny)` en `ci.yml`, con `cargo-deny` por `taiki-e/install-action` y `--locked`.
- [ ] 5.2 `--workspace` en los pasos de análisis y de prueba del CI.
- [ ] 5.3 `.github/dependabot.yml` con `cargo`, `github-actions` y `npm` en `/ui`.
- [ ] 5.4 El script estructural pasa y `just check-all` queda en verde.

## 6. Cierre

- [ ] 6.1 `just e2e` en verde.
- [ ] 6.2 `docs/FEATURE-044-saneamiento-de-dependencias.md`.
- [ ] 6.3 `docs/ENTORNO.md` (versiones y MSRV), `GIT_FLOW.md` (la tabla del CI) y `AGENTS.md` (el gate y la paridad del workspace).
- [ ] 6.4 Revisión de `@rust-reviewer` sin bloqueantes (verificación: las observaciones aplicadas o contestadas).
- [ ] 6.5 PR a `development` con `Closes #167`; CI en verde; fusionar y cerrar #167 a mano.
- [ ] 6.6 `just spec-archive saneamiento-de-dependencias` (verificación: el delta entra en `openspec/specs/`).
- [ ] 6.7 `plans/PLAN-044.md` → `plans/PLAN-044-COMPLETED.md` (verificación: `git mv` hecho).
- [ ] 6.8 PR del archivado a `development`.
