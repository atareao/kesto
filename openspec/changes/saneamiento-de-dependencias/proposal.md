# Proposal — saneamiento de dependencias y vigilancia de la cadena de suministro

Issue **#167 (SEC-11)**, hijo de la auditoría de seguridad **#156** (bloque E).

## Why

`cargo audit` reporta **10 vulnerabilidades** y **16 avisos** (11 *unsound*, 4 *unmaintained*, 1 *yanked*) sobre las 612 dependencias del workspace, y el repositorio no tiene ninguna puerta que lo vigile: no hay `deny.toml`, ni `cargo audit`/`cargo deny` en el CI, ni Renovate/Dependabot. Tres de esos avisos están en **código alcanzable**:

1. **`libsqlite3-sys 0.30.1` (RUSTSEC-2026-0459, CVE-2026-119375, medio 6.8)** — es el SQLite que **es** el almacén de Kesto (`src/db/mod.rs:255`, `opciones_del_dsn`), empaquetado y compilado dentro del binario. Llega por `sqlx 0.8.6`, que fija `^0.30.1`. Cerrarlo exige `sqlx 0.9.1`, que abre el rango a `>=0.30.1, <0.41.0` (`sqlx-sqlite 0.9.1`). El `Cargo.toml` (líneas 72-108) **documenta** por qué se quedó en 0.8.6: en `0.9.0` la feature `sqlite` expande a `sqlite-bundled + sqlite-deserialize + sqlite-load-extension + sqlite-unlock-notify` y abría `libsqlite3-sys` a `<0.38.0` —por debajo del `0.39.0` que cierra el CVE—. **`0.9.1` cambia justo eso** (el techo sube a `0.41`), así que el motivo que decidió la versión ya no se sostiene; lo que sigue en pie es el MSRV (1.94) y la expansión de features, que se evita declarando `sqlite-bundled` en vez de `sqlite`.
2. **`zstd 0.13.3` y `zstd-safe 7.3.0` (RUSTSEC-2026-0391/0398/0403 y 10 avisos *unsound*)** — la compresión de los datos que van al repositorio, por `rustic_core 0.13.0`. Los dos se cierran con un **parche**: `zstd 0.13.4` arrastra `zstd-safe 7.4.x`.
3. **`rmcp 3.5.1` (RUSTSEC-2026-0457/0461/0462)** — el SDK del servidor MCP, declarado directo en `kesto-mcp/Cargo.toml:23`. La -0461 (caché de esquemas sin límite en el transporte Streamable HTTP) es del **servidor** que Kesto sirve. Se cierra en `3.9.0` y la última es `3.11.0`: es un salto **minor** dentro de la misma rama.

Los avisos que no se pueden cerrar sin arrastrar duplicados o una migración propia se **declaran** en `deny.toml` —no se dejan en silencio—. Son **siete excepciones**, y no todas del mismo tipo.

**Tres son avisos *no alcanzables*** —el camino vulnerable no se ejecuta, o un tercero retiene la versión—:

- **`rsa 0.9.10` (RUSTSEC-2023-0071, Marvin)** — el ataque es de **descifrado** PKCS#1 v1.5; Kesto solo **verifica** firmas RS256 con la clave pública y exporta un PEM en el proveedor falso de pruebas. Y no es cerrable por decisión propia: `reqsign-core 3.3.2` —que entra por `rustic_backend`— fija `rsa ^0.9.2`.
- **`suppaftp 8.0.5` (RUSTSEC-2026-0271, inyección CRLF)** — llega por `opendal-service-ftp → opendal → rustic_backend`, y el transporte FTP **nunca se configura**: Kesto habla S3, y el `endpoint` de un repositorio rechaza cualquier esquema que no sea `http`/`https` (`src/rustic/endpoint.rs`).
- **`crypto-bigint 0.5.5` (RUSTSEC-2026-0387, *timing*)** — entra por `jsonwebtoken 11.1.0 → p256/p384 → elliptic-curve → crypto-bigint`, es decir por las rutas **ECDSA**, y Kesto solo usa `RS256` (`src/auth/oidc.rs:423`) y `HS256`. Cerrarlo pide `jsonwebtoken 12`, que mete en el árbol una segunda generación de `sha2`/`hmac` (11/13) por un camino que no se ejecuta.

**Y cuatro son crates *unmaintained* sin versión que los cierre** —RustSec los reporta como error y no hay versión a la que subir—:

- **`async-std 1.13.2` (RUSTSEC-2025-0052)** y **`pin-utils 0.1.1` (RUSTSEC-2026-0441)** — descontinuados, arrastrados por la misma cadena FTP (`suppaftp → async-std`) que Kesto no enciende: caen cuando caiga ese transporte, que es lo que la excepción declara.
- **`chrono 0.4.45` (RUSTSEC-2026-0543)** y **`chrono-tz 0.10.4` (RUSTSEC-2026-0555)** — descontinuados, pero aquí **sí** son dependencia real del producto: las fechas y el `tokio-cron-scheduler`. La salida no es un bump sino una **migración propia a `time`/`jiff`**, fuera del alcance de este change; la excepción nombra esa migración y por qué no entra aquí.

El *yanked* va aparte, porque **no** es una excepción declarada: **`async-executor 1.14.0`**, por la misma cadena FTP, se sube a `1.14.1` (parche) porque es gratis.

El árbol, además, **no tiene crates sin uso**: verificado con `cargo-machete 0.10.1 --with-metadata` sobre el workspace entero (el `0.7.0` instalado no entiende `resolver = "3"` y no sirve como prueba).

Y la vigilancia tiene un agujero de forma: **el CI no compila ni prueba `kesto-mcp`**. `ci.yml` corre `cargo clippy --all-targets` y `cargo test --locked` **sin `--workspace`** (líneas 72 y 75), mientras las recetas locales sí lo llevan (`.justfile:64` y `:68`), así que la paridad que documenta `AGENTS.md` —«los tres checks del CI, en su orden»— está rota, y justo en la crate donde vive `rmcp`.

## What Changes

### Las dependencias alcanzables se sanean (SEC-11)

- `zstd 0.13.3 → 0.13.4` y `zstd-safe 7.3.0 → 7.4.x`: parche, el segundo arrastrado por el primero.
- `rmcp 3.5.1 → 3.11.0` en `kesto-mcp`: minor. Se comprueba que `transport-io` y `transport-streamable-http-server` siguen existiendo con esos nombres.
- `sqlx 0.8.6 → 0.9.1` y `libsqlite3-sys 0.30.1 → 0.40.0`: la feature `sqlite` se sustituye por `sqlite-bundled` —el equivalente exacto de lo que hacía `0.8.6`— y se declara el MSRV real (`rust-version = "1.94"`), que `sqlx 0.9` exige. Tres puntos de API cambian, y son los únicos que el compilador señala: el literal `Migrator` de `src/test_support.rs`, el `Migration::new` de al lado —ahora `SqlStr`— y los dos `query*()` que reciben una cadena no estática (`src/db/migrate.rs`, `src/db/entities/restauracion.rs`), que pasan por `AssertSqlSafe`.
- `async-executor 1.14.0 → 1.14.1`: parche, sale del *yanked*.

### Las excepciones se declaran con su motivo

- `deny.toml` nuevo en la raíz, con las cuatro familias: `advisories` (**siete** excepciones —tres avisos no alcanzables y cuatro crates *unmaintained* sin versión que los cierre—, cada una con su `reason`), `licenses` (allowlist explícita de las licencias que hoy están en el árbol), `bans` (`wildcards = "deny"` y `multiple-versions = "warn"`, porque las 126 duplicidades actuales vienen de árboles de terceros: `rustic_core`, `opendal`, `wiremock`) y `sources` (`unknown-registry` y `unknown-git` a `deny`: aquí solo se usa crates.io).
- Receta `just deny` y su encaje en `check-all`, que es el análogo local del CI entero.

### El CI vigila la cadena de suministro y cubre el workspace entero

- Job nuevo `Dependencies (cargo deny)` en `ci.yml`, con `cargo-deny` instalado por `taiki-e/install-action` —la convención del repositorio— y `--locked`.
- `cargo clippy --workspace --all-targets` y `cargo test --locked --workspace`: la paridad con `just clippy`/`just test` y la cobertura de `kesto-mcp`.
- `set -e` en la receta `scripts-test` del `.justfile`: sin él, el bucle devolvía el código de salida del último script, así que un rojo se enmascaraba según el orden alfabético.

### Las actualizaciones llegan solas

- `.github/dependabot.yml` con los tres ecosistemas: `cargo` —una entrada en la raíz cubre el workspace—, `github-actions` y `npm` en `/ui`; periodicidad semanal, agrupadas por naturaleza y con el prefijo de commit de la casa.

### Documentación

- `docs/FEATURE-044-...`, `docs/COMANDOS.md` (la receta `deny`), `docs/ENTORNO.md` (versiones y MSRV), `GIT_FLOW.md` (la tabla del CI) y `AGENTS.md` (el gate y la paridad).

## Non-goals

- NO se sube `jsonwebtoken` a 12: cerraría `crypto-bigint` por un camino (ECDSA) que no se ejecuta y duplicaría `sha2`/`hmac` en el árbol. Queda como excepción declarada.
- NO se sube `rsa` a 0.10: `reqsign-core` lo fija en `^0.9` y el advisory es de descifrado.
- NO se sube `opendal`/`rustic_backend` para cerrar `suppaftp`: el transporte FTP no se configura.
- NO se sustituyen `chrono`/`chrono-tz` por `time`/`jiff`: los dos están *unmaintained* en todas sus versiones y esa migración —las fechas del producto y el programador de trabajos— es un trabajo propio, fuera del alcance de este change; quedan como excepción declarada con su motivo.
- NO se toca `migrations/` ni el esquema: el checksum de migración de `sqlx` no cambia entre `0.8.6` y `0.9.1` y la tabla de control no se renombra.
- NO se añade `cargo-machete` al CI: hoy informa limpio y su instalación sería un coste sin hallazgo; la comprobación queda como verificación puntual documentada.
- NO se toca el panel (`ui/`): `pnpm audit` sale limpio y Dependabot solo abre PRs.
- NO se abordan los hallazgos de #168, ni #169, #173, #175 y #182.

## Capabilities

### New Capabilities

- `cadena-de-suministro`: las dependencias del workspace se vigilan (advisories, licencias, duplicados y fuentes), las excepciones se declaran con su motivo, el CI cubre el workspace entero y las actualizaciones llegan por PR revisable.

### Modified Capabilities

- `db`: se añade el requisito de que el motor siga siendo el SQLite empaquetado y de que las migraciones ya aplicadas no se invaliden.

## Impact

- **Código**: `Cargo.toml`, `Cargo.lock`, `kesto-mcp/Cargo.toml`, `src/test_support.rs`, `src/db/migrate.rs`, `src/db/entities/restauracion.rs`.
- **CI y raíz**: `ci.yml` (job nuevo y `--workspace`), `deny.toml` (nuevo), `.github/dependabot.yml` (nuevo), `scripts/dependencias.test.sh` (nuevo), `.justfile` (receta `deny` y `set -e` en `scripts-test`).
- **Toolchain**: MSRV declarado `1.94` (lo exige `sqlx 0.9.1`); el CI usa `stable` y el entorno local, `1.97`.
- **Base de datos**: ninguna migración nueva y ningún cambio de esquema; el SQLite empaquetado sube de `0.30.1` a `0.40.0`.
- **Dependencias**: un salto mayor (`sqlx`), un minor (`rmcp`), dos parches (`zstd`, `async-executor`) y siete excepciones justificadas en `deny.toml` (tres avisos no alcanzables y cuatro crates *unmaintained* sin versión que los cierre).
- **Despliegue**: ningún cambio para el operador; el binario cambia de SQLite empaquetado, sin cambio de formato de fichero.
