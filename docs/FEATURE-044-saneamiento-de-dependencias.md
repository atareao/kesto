# FEATURE-044 — Saneamiento de dependencias y vigilancia de la cadena de suministro (#167)

- **Issue:** #167 (SEC-11), hijo de la auditoría de seguridad **#156** (bloque E). **Change OpenSpec:** `saneamiento-de-dependencias`. **Plan:** `plans/PLAN-044.md`.
- **Tipo:** dependencias + configuración + CI + documentación. **No** toca el motor, el esquema de la base ni el panel.
- **MSRV declarado:** `rust-version = "1.94"` en `Cargo.toml` (lo exige `sqlx 0.9.1`). El CI usa `stable`; el entorno local, `1.97`.

## Qué es

La **puerta de la cadena de suministro** del repositorio y el **saneamiento de las dependencias** del bloque E de la campaña de seguridad. Antes de este change no había ninguna: `cargo audit` reportaba **10 vulnerabilidades** y **16 avisos** (11 *unsound*, 4 *unmaintained*, 1 *yanked*) sobre las 612 dependencias del workspace, y nada los vigilaba —ni `deny.toml`, ni `cargo audit`/`cargo deny` en el CI, ni Renovate/Dependabot—.

El bloque hace tres cosas, en este orden de valor:

1. **Cierra los avisos alcanzables** —los que tocan código que el producto **siempre** ejecuta—: el CVE de SQLite (`libsqlite3-sys`), `zstd`/`zstd-safe` y `rmcp`.
2. **Declara con motivo los siete que no lo son** —tres avisos no alcanzables y cuatro crates *unmaintained* sin versión que los cierre— en un `deny.toml` versionado, para que «no nos alcanza» se distinga de «lo silenciamos y ya nadie se acordó».
3. **Pone la vigilancia en el CI y en local**, y cubre el workspace entero (el job `backend` no compilaba ni probaba `kesto-mcp`, justo donde vivía `rmcp`), con Dependabot abriendo las actualizaciones por PR revisable.

## Causa / motivo

Tres de los avisos estaban en **código alcanzable**, y no había puerta que los delatara:

- **`libsqlite3-sys 0.30.1` (RUSTSEC-2026-0459, CVE-2026-119375, medio 6.8).** Es el SQLite que **es** el almacén de Kesto (`src/db/mod.rs`, `opciones_del_dsn`), empaquetado y compilado dentro del binario. Llega por `sqlx 0.8.6`, que fija `^0.30.1`; cerrarlo exige `sqlx 0.9.1`, que abre el rango a `>=0.30.1, <0.41.0`.
- **`zstd 0.13.3` / `zstd-safe 7.3.0` (RUSTSEC-2026-0391/0398/0403 y 10 avisos *unsound*).** La compresión de los datos que van al repositorio, por `rustic_core 0.13.0`. Se cierran con un parche.
- **`rmcp 3.5.1` (RUSTSEC-2026-0457/0461/0462).** El SDK del servidor MCP, declarado directo en `kesto-mcp/Cargo.toml`. La -0461 (caché de esquemas sin límite en el transporte Streamable HTTP) es del **servidor** que Kesto sirve.

Y un agujero de forma: el CI corría `cargo clippy --all-targets` y `cargo test --locked` **sin `--workspace`**, mientras las recetas locales sí lo llevaban, así que la paridad que documenta `AGENTS.md` estaba rota justo en la crate donde vive `rmcp`.

## Ficheros implicados

- `deny.toml` (nuevo, raíz): las cuatro familias —advisories con siete excepciones y su motivo, licencias con allowlist explícita, prohibiciones y fuentes—.
- `.github/workflows/ci.yml`: job nuevo `Dependencies (cargo deny)` y `--workspace` en los pasos de clippy y test.
- `.github/dependabot.yml` (nuevo): `cargo`, `github-actions` y `npm` en `/ui`.
- `scripts/dependencias.test.sh` (nuevo): prueba **estructural** del cableado.
- `.justfile`: receta `just deny`, su encaje en `check-all` y `set -e` en `scripts-test`.
- `Cargo.toml` / `Cargo.lock`: `sqlx 0.9.1` con `sqlite-bundled`, `libsqlite3-sys 0.40.0`, `rust-version = "1.94"`, `zstd 0.13.4` y `async-executor 1.14.1`.
- `kesto-mcp/Cargo.toml`: `rmcp 3.11`.
- `src/test_support.rs`: el literal `Migrator` y su `Migration::new`.
- `src/db/migrate.rs`: un `query*()` con cadena no estática y la llamada a `run_direct`; además, la prueba de caracterización del checksum de una migración.
- `src/db/entities/restauracion.rs`: otro `query*()` con cadena no estática.
- `docs/COMANDOS.md`, `docs/ENTORNO.md`, `GIT_FLOW.md`, `AGENTS.md`, `docs/FEATURE-044-saneamiento-de-dependencias.md` (este documento).
- `openspec/changes/saneamiento-de-dependencias/` · `plans/PLAN-044.md`.
- `migrations/` y `ui/`: **sin cambios**.

## Cómo funciona

- **La puerta es `cargo deny`, no `cargo audit`.** `audit` solo mira avisos; `deny` mira **cuatro familias** —`advisories`, `licenses`, `bans` y `sources`— con la configuración **versionada en el repositorio**, que es lo que permite revisar cada excepción en el PR. Las licencias y las fuentes no son adorno: son lo que atrapa a la dependencia nueva que entra con una licencia inesperada o desde un registro ajeno, el cambio que nadie revisa a mano en un PR de bumps. Se corre con `cargo deny --workspace --locked check`; el `--locked` no es cosmético: sin él la comprobación resolvería el árbol por su cuenta y podría juzgar una versión que no es la del PR (y en `cargo deny` el flag es **global**, va antes del subcomando).
- **Cada excepción lleva su motivo por escrito y es refutable.** El `ignore` admite entradas sin `reason`, y también se puede apagar la comprobación entera; las dos se descartan. La lista es el **único** sitio donde un lector futuro distingue «este aviso no nos alcanza» de «lo silenciamos y ya nadie se acordó de por qué». Cada entrada nombra **quién retiene la versión** o **qué camino no se ejecuta**, de modo que se puede refutar con un `cargo tree -i <crate>` y retirar en cuanto su causa desaparezca —Dependabot abre el PR del bump y el `ignore` se va en el mismo PR—.
- **El job `Dependencies (cargo deny)`.** En `ci.yml`, con `cargo-deny` instalado por `taiki-e/install-action` —la convención del repositorio— y sin `needs: preflight`, porque no depende de la detección del panel y corre en paralelo. Los pasos de clippy y de test del job `backend` pasan a `--workspace`: sin él Cargo se queda en el paquete raíz y `kesto-mcp` no se mira nunca.
- **La puerta cubre el workspace entero (`--workspace`).** `cargo deny` construye su grafo desde los miembros por defecto del workspace, y este repositorio declara el paquete raíz `kesto` y el miembro `kesto-mcp` **sin** `default-members`: el defecto es solo `kesto`, y como `kesto-mcp` no es dependencia suya, `cargo deny --locked check` **no veía** `kesto-mcp` ni, por tanto, `rmcp` —justo el crate cuyos tres avisos motivan el saneamiento—. El manifiesto raíz es un **paquete** (`[package] name = "kesto"` junto a `[workspace]`), no un virtual, y ahí está la diferencia entre vigilar y aparentar: con el flag entran `kesto-mcp`, `rmcp` y `rmcp-macros`. Va en el job del CI y en la receta `deny` —el espejo local—, y las comprobaciones **B** y **E** de `scripts/dependencias.test.sh` lo vigilan (D11).
- **Las recetas locales son el espejo del CI.** `just deny` (`cargo deny --workspace --locked check`) es la puerta; `just check` encadena los tres checks del backend en el orden del CI y `just check-all` añade `deny`, `scripts-test` y los cuatro pasos del panel. `scripts-test` corre con `set -e`: sin él un bucle `for` devuelve el código de su **último** script y un rojo se enmascara según el orden alfabético —una puerta que devuelve éxito con una prueba en rojo es un adorno—.

### Las siete excepciones declaradas

| Aviso | Crate | Por qué no se cierra | Quién lo retiene / qué camino no se ejecuta |
|---|---|---|---|
| RUSTSEC-2023-0071 (Marvin) | `rsa 0.9.10` | El ataque es de **descifrado** PKCS#1 v1.5 con la clave privada, y Kesto solo **verifica** firmas RS256 con la pública (el único otro uso es exportar un PEM en el proveedor falso de pruebas) | `reqsign-core 3.3.2` —entra por `rustic_backend`— fija `rsa ^0.9.2`; subir a `0.10` exige cambiar de cliente de almacenamiento |
| RUSTSEC-2026-0271 (inyección CRLF) | `suppaftp 8.0.5` | El transporte FTP **nunca** se configura: Kesto habla S3 y el `endpoint` de un repositorio rechaza cualquier esquema que no sea `http`/`https` (`src/rustic/endpoint.rs`) | `opendal-service-ftp → opendal → rustic_backend`; cerrarlo pide `suppaftp 10`, con salto mayor de `opendal` por un camino que no se ejecuta |
| RUSTSEC-2026-0387 (*timing*) | `crypto-bigint 0.5.5` | Entra por las rutas **ECDSA** (`p256`/`p384 → elliptic-curve → crypto-bigint`) y Kesto solo usa `RS256` y `HS256` | Cerrarlo pide `jsonwebtoken 12`, que mete una segunda generación de `sha2`/`hmac` por un camino que no se ejecuta |
| RUSTSEC-2025-0052 | `async-std 1.13.2` | *Unmaintained* en **todas** sus versiones: no hay versión que lo cierre | Cadena FTP sin configurar (`suppaftp → async-std`); cae cuando caiga ese transporte |
| RUSTSEC-2026-0441 | `pin-utils 0.1.1` | *Unmaintained* (el repositorio se archivó); sin versión que lo cierre | Arrastrado por `async-std` en la misma cadena FTP sin configurar |
| RUSTSEC-2026-0543 | `chrono 0.4.45` | *Unmaintained* en todas sus versiones. Aquí **sí** es dependencia real del producto (las fechas) | La salida no es un bump sino una **migración propia a `time`/`jiff`**, fuera del alcance de este change |
| RUSTSEC-2026-0555 | `chrono-tz 0.10.4` | *Unmaintained*; dependencia real (el `tokio-cron-scheduler`) | Cae cuando caiga `chrono`, en la misma migración a `time`/`jiff` |

Las tres primeras son avisos **no alcanzables**; las cuatro últimas son crates *unmaintained* sin versión que los cierre. **Coste aceptado**: una excepción puede quedarse obsoleta —el tercero publica la versión que la cierra y el CI no lo recuerda—, porque `cargo deny` solo comprueba que el aviso siga estando; lo mitiga Dependabot.

El *yanked* va aparte, porque **no** es una excepción declarada: `async-executor 1.14.0`, por la misma cadena FTP, se sube a `1.14.1` (parche) porque es gratis.

### Lo que se cerró de verdad

- **`zstd 0.13.4` / `zstd-safe 7.4.1`**: parche, el segundo arrastrado por el primero; los avisos de compresión desaparecen.
- **`rmcp 3.5.1 → 3.11.0`** en `kesto-mcp`: minor dentro de la misma rama; `transport-io` y `transport-streamable-http-server` siguen existiendo con esos nombres.
- **`async-executor 1.14.1`**: parche, sale del *yanked*.
- **El CVE de SQLite (`libsqlite3-sys 0.30.1 → 0.40.0`)**: `RUSTSEC-2026-0459` **no estaba** en el `ignore` —las siete excepciones son otros avisos—, así que desaparece por el salto y no hay entrada que retirar.

### El salto de `sqlx` (0.8.6 → 0.9.1)

- **Por qué ahora.** El `Cargo.toml` documentaba por qué se eligió `0.8.6` y no `0.9.0`: en `0.9.0` el techo de `libsqlite3-sys` bajaba a `<0.38.0` —por debajo del `0.39.0` que cierra el CVE—, así que el salto no cerraba el advisory que lo motivaba. **En `0.9.1` el techo sube a `<0.41.0`**, el CVE pasa a ser cerrable y el argumento que decidió la versión deja de sostenerse.
- **La feature es `sqlite-bundled`, no `sqlite`.** En `0.9` la feature `sqlite` expande a `sqlite-bundled + sqlite-deserialize + sqlite-load-extension + sqlite-unlock-notify`; se declara `sqlite-bundled`, el equivalente **exacto** de lo que hacía `sqlite` en `0.8.6`, sin encender la carga de extensiones, el aviso de desbloqueo ni la deserialización, que Kesto no usa.
- **El MSRV se declara.** `sqlx 0.9.1` exige Rust 1.94; se pone `rust-version = "1.94"` en `[package]` en lugar de dejarlo en un comentario, para que un toolchain por debajo falle al resolver con el número en la mano.
- **No se toca `migrations/` ni el esquema.** El algoritmo del checksum (`Sha384` del SQL) es el mismo, la tabla `_sqlx_migrations` conserva su nombre y su forma por defecto, y los nombres de `migrations/` ya cumplen la validación nueva. Hay una **prueba de caracterización** que fija el checksum hexadecimal de una migración: si una versión futura cambia el algoritmo, la prueba se pone roja **antes** de que un operador vea un `VersionMismatch` en producción (D7).
- **El planificador se movió.** El salto arrastra el SQLite empaquetado de **3.46.0 a 3.54.1**, y con él se movió el planificador bajo `ANALYZE`: la caracterización del plan de la `0004` se volvió a medir y pasa a fijar el **acceso indexado** (`!SCAN e`) en vez de una puerta concreta, que es la propiedad que no depende de la versión (D10). Las aserciones del bloque sin estadísticas y del filtro suelto no se tocan, y la prueba sigue mordiendo.

## Cómo verificar

```sh
just check                       # los tres checks del backend, en el orden del CI
just deny                        # la puerta de la cadena de suministro (necesita red la primera vez)
just check-all                   # backend + deny + scripts-test + panel: el análogo local del CI
cargo deny --workspace --locked check  # la misma puerta, sobre el workspace y con el lockfile congelado
bash scripts/dependencias.test.sh  # prueba estructural del cableado (sin red)
openspec validate --strict saneamiento-de-dependencias
```

`scripts/dependencias.test.sh` lee `deny.toml`, `ci.yml`, `.github/dependabot.yml` y el `.justfile` **como texto** y comprueba la forma: las cuatro secciones y una allowlist no vacía, que **cada** `ignore` lleve motivo, que el CI ejecute `cargo deny --workspace --locked check` (los dos flags en la misma línea), que sus pasos de clippy y test lleven `--workspace`, que la receta `deny` del `.justfile` también lo lleve, que Dependabot declare los tres ecosistemas, y que `scripts-test` propague el fallo (`set -e`). Lo que **no** prueba: que el trabajo llegue a ejecutarse —eso lo demuestra el CI— ni que la allowlist de licencias esté completa —eso lo demuestra `cargo deny` corriendo de verdad—. Lo que atrapa es el modo de fallo que motivó SEC-11: que alguien borre el cableado y la vigilancia desaparezca sin que nada lo delate (D6).

## Variables / configuración

- **Ninguna variable de entorno nueva.** La puerta no lee configuración del despliegue: `deny.toml` va versionado en la raíz.
- **`cargo deny` necesita red la primera vez** —clona la base de avisos de RustSec—. Por eso la receta `deny` va en `check-all` y **no** en `check`: `check` es lo que se corre antes de cada commit y tiene que poder correr sin conexión.
- **`cargo deny` construye su grafo desde los miembros por defecto del workspace**, y este repositorio no declara `default-members`, así que sin `--workspace` el grafo arranca en el paquete raíz `kesto` y deja fuera a `kesto-mcp` y a `rmcp`. Por eso el job del CI y la receta `deny` invocan la puerta con `--workspace`: ver «Cómo funciona» (D11).

## Cómo habilitar / desplegar

- **Para el operador no hay nada que hacer.** No hay variable nueva, ni migración nueva, ni cambio de esquema. El binario cambia de **SQLite empaquetado** (3.46.0 → 3.54.1), sin cambio de formato del fichero de la base ni de esquema: las bases ya desplegadas se abren tal cual.
- **La incompatibilidad es de toolchain, no de despliegue.** El MSRV declarado es 1.94: quien compile desde fuente con un Rust anterior falla al resolver, con el número en la mano. El binario y la imagen publicados no cambian de contrato.

## Decisiones

Remite a `openspec/changes/saneamiento-de-dependencias/design.md` para el porqué completo (D1–D11). En resumen: **D1** `sqlx 0.9.1` ahora y con `sqlite-bundled`; **D2** las excepciones se declaran con motivo, no se silencian; **D3** `multiple-versions = "warn"` (las 126 duplicidades son de árboles de terceros) y `wildcards = "deny"` (hoy no hay ninguno); **D4** la puerta es `cargo deny`, no `cargo audit`; **D5** Dependabot, no Renovate; **D6** la prueba del cableado es estructural y tiene un límite declarado; **D7** el checksum de migración se fija en una caracterización; **D8** la receta `scripts-test` enmascaraba el fallo; **D9** el grafo de `cargo deny` no incluía `kesto-mcp`; **D10** la caracterización del planificador es de la versión de SQLite; **D11** la puerta no veía `kesto-mcp` sin `--workspace`, y el flag la arregla.

## Follow-ups

- **Bloque F:** **#168**. **Resto de la campaña #156:** **#169** (KDF con sal persistida), **#173** (privilegio del motor), **#175** (`ON_ERROR_STOP` en la restauración de PostgreSQL) y **#182** (resolución DNS del `endpoint`). Ninguno entra en este bloque.
- **Migración de `chrono`/`chrono-tz` a `time`/`jiff`** — **#185**. Es la **salida** de dos de las excepciones declaradas (`RUSTSEC-2026-0543` y `-0555`). Los dos crates están *unmaintained* en **todas** sus versiones —RustSec los reporta como error y no hay versión a la que subir—, así que el `ignore` no es una comodidad: no hay bump posible. `chrono` es dependencia **real** del producto (las fechas) y `chrono-tz` lo arrastra el `tokio-cron-scheduler`; sustituirlos por `time` o `jiff` es un trabajo propio que toca las fechas del producto y el programador de trabajos, con su propio change y su propio plan. La medida del trabajo la da que `chrono` entra por **tres caminos** —el uso propio (3 ficheros, 8 referencias, en `src/scheduler/`), `croner` y `tokio-cron-scheduler` (los dos dependencias **directas** del crate raíz) y la cadena FTP de `suppaftp`—, así que el grueso no es el uso propio sino salir de los dos crates del programador, y mientras la cadena FTP siga ahí `chrono` seguirá en el árbol aunque se migre lo demás. La excepción en `deny.toml` es lo único que nombra la salida hasta que #185 la cierre.
