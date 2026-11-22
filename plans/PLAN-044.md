# PLAN-044 — Saneamiento de dependencias y vigilancia de la cadena de suministro

- **Issue**: #167 (SEC-11), hijo de la auditoría de seguridad #156 — bloque E de la campaña.
- **Rama**: `feature/saneamiento-de-dependencias` (desde `development`).
- **Change de OpenSpec**: `openspec/changes/saneamiento-de-dependencias/` (aprobado por el usuario antes de escribir código).
- **Documento de la feature**: `docs/FEATURE-044-saneamiento-de-dependencias.md` (al cerrar).
- **Bump esperado**: minor, deducido por `release-prepare` de los commits. Los commits van `feat:`/`fix:` **sin** `!` ni `BREAKING CHANGE`.

## Objetivo

`cargo audit` reporta **10 vulnerabilidades** y **16 avisos** (11 *unsound*, 4 *unmaintained*, 1 *yanked*) sobre las 612 dependencias del workspace, y no hay ninguna puerta que lo vigile: ni `deny.toml`, ni `cargo audit`/`cargo deny` en el CI, ni Renovate/Dependabot. El bloque cierra los tres avisos **alcanzables**, declara con motivo los **siete** que no lo son (tres no alcanzables y cuatro *unmaintained* sin versión que los cierre), pone la puerta en el CI y activa Dependabot.

| Crate | Advisory | ¿Alcanzable? | Cómo se cierra |
|---|---|---|---|
| `libsqlite3-sys 0.30.1` | RUSTSEC-2026-0459 (CVE, medio 6.8) | **sí** — SQLite es el almacén | `sqlx 0.9.1` + `libsqlite3-sys 0.40.0` |
| `zstd 0.13.3` / `zstd-safe 7.3.0` | 2 vulns + 10 *unsound* | **sí** — compresión | `zstd 0.13.4` (parche, arrastra `zstd-safe 7.4.x`) |
| `rmcp 3.5.1` | 3 (una del servidor HTTP) | **sí** — el servidor MCP | `rmcp 3.11.0` (minor) |
| `async-executor 1.14.0` | *yanked* | no — cadena FTP | `1.14.1` (parche) |
| `rsa 0.9.10` | Marvin (descifrado) | no — solo verifica RS256 | excepción: `reqsign-core` fija `^0.9` |
| `suppaftp 8.0.5` | inyección CRLF | no — transporte FTP sin configurar | excepción |
| `crypto-bigint 0.5.5` | *timing* | no — solo ECDSA, que no se usa | excepción: cerrarlo pediría `jsonwebtoken 12` |

Y un agujero de forma: **el CI no compila ni prueba `kesto-mcp`** (`ci.yml:72` y `:75` sin `--workspace`, mientras `.justfile:64` y `:68` sí lo llevan), justo donde vive `rmcp`.

## Alcance aprobado por el usuario

1. **Todo salvo `jsonwebtoken 12`**: se cierran los advisories alcanzables (`zstd`/`zstd-safe`, `rmcp`, `sqlx`/`libsqlite3-sys`, `async-executor`) y `crypto-bigint`, `rsa` y `suppaftp` quedan como excepción justificada en `deny.toml`.
2. **La puerta es `cargo deny` con `deny.toml`** versionado (advisories + licencias + prohibiciones + fuentes).
3. **Dependabot** para los tres ecosistemas (`cargo`, `github-actions`, `npm` en `/ui`).
4. **Alinear el CI con `just check`**: `--workspace` en clippy y test.

## Fuera de alcance

- `jsonwebtoken 11 → 12` (cerraría `crypto-bigint` por un camino que no se ejecuta y duplicaría `sha2`/`hmac`).
- `rsa 0.9 → 0.10` (lo retiene `reqsign-core`; el advisory es de descifrado).
- Subir `opendal`/`rustic_backend` para cerrar `suppaftp`.
- `cargo-machete` en el CI (verificado limpio con `0.10.1 --with-metadata`; el `0.7.0` local no entiende `resolver = "3"`).
- `migrations/` y el esquema: no se tocan (el checksum de `sqlx` no cambia y la tabla de control conserva su forma).
- #168, #169, #173, #175 y #182.

## Ficheros implicados

`Cargo.toml`, `Cargo.lock`, `kesto-mcp/Cargo.toml`, `src/test_support.rs` (literal `Migrator` y `Migration::new`), `src/db/migrate.rs` (caracterización del checksum y un `query*()` con cadena no estática), `src/db/entities/restauracion.rs` (otro `query*()`), `deny.toml` (nuevo), `scripts/dependencias.test.sh` (nuevo), `.github/dependabot.yml` (nuevo), `.github/workflows/ci.yml`, `.justfile`, `docs/COMANDOS.md`, `docs/ENTORNO.md`, `GIT_FLOW.md`, `AGENTS.md`.

## Riesgos y ripples

- **El salto de `sqlx` toca casi toda la suite** (≈399 pruebas `#[tokio::test]`, ≈126 puntos que abren base). El compilador señala tres sitios y nada más; el riesgo es de compilación, no de esquema.
- **MSRV**: `sqlx 0.9.1` exige Rust 1.94; se declara `rust-version` en lugar de dejarlo en un comentario. El CI usa `stable` y el entorno local, 1.97.
- **La caracterización del checksum** es el guardián de que un cambio de algoritmo no invalide bases ya desplegadas: verde antes y después del salto.
- **Una excepción puede quedar obsoleta** (el tercero publica la versión que la cierra); lo mitiga Dependabot, que abre el PR del bump.
- **`cargo deny` necesita red** la primera vez (clona la base de avisos): por eso la receta va en `check-all` y no en `check`.

## Tareas

### 0. Preparación

- [ ] 0.1 Rama `feature/saneamiento-de-dependencias` desde `development`.
- [ ] 0.2 Change `saneamiento-de-dependencias` (proposal, design, tasks y los deltas de `cadena-de-suministro` y `db`).
- [ ] 0.3 Este plan.
- [ ] 0.4 `openspec validate --strict saneamiento-de-dependencias` válido.

### 1. RED — la puerta y su cableado, que hoy no existen

- [x] 1.1 `scripts/dependencias.test.sh`: prueba estructural (el `deny.toml` con sus cuatro secciones y cada excepción con motivo; el CI con `cargo deny` y `--locked` y con `--workspace` en clippy y test; Dependabot con los tres ecosistemas). Falla hoy: los tres ficheros no existen.
- [x] 1.2 Caracterización del checksum de una migración en `src/db/migrate.rs` (verde antes y después del salto).
- [x] 1.3 Verificar el rojo: `just scripts-test` falla solo en `dependencias.test.sh`.

### 2. GREEN — `deny.toml` y la receta

- [x] 2.1 `deny.toml`: advisories (**siete** excepciones con motivo: tres avisos no alcanzables y cuatro crates *unmaintained* sin versión que los cierre), licencias (allowlist explícita), prohibiciones (comodines denegados; duplicidades avisadas, porque las 126 actuales son de terceros) y fuentes (solo crates.io).
- [x] 2.2 Receta `just deny` y su encaje en `check-all`.
- [x] 2.3 `cargo deny check licenses bans sources` verde. La comprobación entera, `cargo deny check`, no lo puede estar hasta la 4.5: el CVE de SQLite sigue rojo hasta el salto de `sqlx`.
- [x] 2.4 `just scripts-test` verde.
- [x] 2.5 `docs/COMANDOS.md` documenta la receta.
- [x] 2.6 `set -e` en la receta `scripts-test` del `.justfile` (el bucle devolvía el código del último script y enmascaraba un rojo según el orden alfabético).

### 3. GREEN — los saltos baratos

- [x] 3.1 `zstd 0.13.4` (arrastra `zstd-safe 7.4.x`).
- [x] 3.2 `rmcp 3.11.0` en `kesto-mcp`, comprobando sus features.
- [x] 3.3 `async-executor 1.14.1`.
- [x] 3.4 `cargo test --locked --workspace` en verde.

### 4. GREEN — `sqlx 0.9.1` y `libsqlite3-sys` saneado

- [x] 4.1 `Cargo.toml`: `sqlx 0.9.1` con `sqlite-bundled` (no `sqlite`), `rust-version = "1.94"` y el bloque de comentarios actualizado.
- [x] 4.2 `libsqlite3-sys` a `0.40.0` dentro del rango que abre `sqlx-sqlite 0.9.1`.
- [x] 4.3 Los tres puntos de API: `Migrator` y `Migration::new` en `src/test_support.rs`; los dos `query*()` con cadena no estática. (Cuatro en realidad: el compilador señala además `run_direct`, que en `0.9` pasa a `(Option<i64>, &mut C, bool)`.)
- [x] 4.4 La caracterización del checksum sigue verde.
- [x] 4.5 `cargo test --locked --workspace` verde y `cargo deny check` **entero** verde (RUSTSEC-2026-0459 fuera de los avisos). El salto sube el SQLite empaquetado de 3.46.0 a 3.54.1 y movió el planificador bajo `ANALYZE`: la caracterización del plan de la `0004` se volvió a medir y fija el acceso indexado en vez de una puerta concreta (D10), sin tocar el bloque sin estadísticas ni el filtro suelto.

### 5. GREEN — CI y Dependabot

- [x] 5.1 Job `Dependencies (cargo deny)` en `ci.yml` (`taiki-e/install-action`, `--locked`).
- [x] 5.2 `--workspace` en los pasos de análisis y de prueba del CI.
- [x] 5.3 `.github/dependabot.yml` con `cargo`, `github-actions` y `npm` en `/ui`.
- [x] 5.4 `bash scripts/dependencias.test.sh` verde. `just check-all` queda verde al cerrar las tareas 3 y 4, que apagan los avisos alcanzables.

### 6. Cierre

- [ ] 6.1 `just e2e` verde.
- [ ] 6.2 `docs/FEATURE-044-saneamiento-de-dependencias.md`.
- [ ] 6.3 `docs/ENTORNO.md`, `GIT_FLOW.md` y `AGENTS.md`.
- [ ] 6.4 `@rust-reviewer` sin bloqueantes.
- [ ] 6.5 PR a `development` con `Closes #167`; CI verde; fusionar y cerrar #167 a mano.
- [ ] 6.6 `just spec-archive saneamiento-de-dependencias`.
- [ ] 6.7 `plans/PLAN-044.md` → `plans/PLAN-044-COMPLETED.md`.
- [ ] 6.8 PR del archivado a `development`.
