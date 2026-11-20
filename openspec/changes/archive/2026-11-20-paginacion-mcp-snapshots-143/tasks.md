# Tasks — paginacion-mcp-snapshots-143

> Change del **servidor MCP**: expone `limit`/`offset` en `list_snapshots` y deja de fijar un
> límite propio, para que el agente pueda paginar repositorios con más de 200 instantáneas.
> Issue #143. No toca la API, el motor ni el catálogo (siguen nueve herramientas, todas de lectura).
> TDD: primero los tests que hoy fallan, luego el mínimo, luego el refactor. Sin dependencias nuevas.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Localizar el punto de partida: `LIMITE_DE_SNAPSHOTS` y el `limit=200` fijo en `kesto-mcp/src/server.rs`; el test que lo exige en `kesto-mcp/tests/herramientas.rs` (verificación: se citan líneas concretas).

## 1. `limit`/`offset` en `list_snapshots`

### RED

- [x] 1.1 MCP: test que invoca `list_snapshots` **solo** con `repository_id` y exige que la petición a la API **no** lleve `limit` (verificación: en rojo hoy —la herramienta manda `limit=200`—; se ejecuta con `cargo test --locked -p kesto-mcp`).
- [x] 1.2 MCP: test que invoca `list_snapshots` con `limit` y `offset` y exige que la petición los reenvíe tal cual, sin recortar el `limit` (verificación: en rojo hoy —`deny_unknown_fields` rechaza los campos y el `limit` va fijo a 200—).
- [x] 1.3 MCP: test que lee la descripción anunciada de `list_snapshots` y exige que mencione `total`, `truncated` y `offset` (verificación: en rojo hoy —la descripción no los nombra—).

### GREEN

- [x] 1.4 `kesto-mcp/src/server.rs`: `PorRepositorio` gana `limit: Option<i64>` y `offset: Option<i64>` con `#[serde(default)]`, conservando `deny_unknown_fields` (verificación: 1.2 en verde en cuanto al parseo).
- [x] 1.5 `kesto-mcp/src/server.rs`: `list_snapshots` arma la consulta con `repository_id` y **solo** los de `limit`/`offset` que vengan informados (verificación: 1.1 y 1.2 en verde).
- [x] 1.6 `kesto-mcp/src/server.rs`: eliminar `LIMITE_DE_SNAPSHOTS` y su rustdoc; actualizar la descripción de la herramienta para citar `total`, `truncated` y la paginación con `offset` (verificación: 1.3 en verde; sin referencias colgando a la constante —`rg LIMITE_DE_SNAPSHOTS` vacío—).

### REFACTOR

- [x] 1.7 Rustdoc de `PorRepositorio` explicando por qué no fija un `limit` propio (defecto y tope **los mismos** que la API; la duplicación desaparece) (verificación: `cargo doc` sin warnings; comportamiento intacto).
- [x] 1.8 `just check` en verde (fmt + clippy + tests del workspace) (verificación: termina sin error).

## 2. Verificación

- [x] 2.1 `cargo test --locked -p kesto-mcp` en verde (verificación: 0 fallos).
- [x] 2.2 `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios (verificación: 0 warnings).

## 3. Cierre

- [x] 3.1 Marcar las tareas hechas y abrir el PR a `development` con `Closes #143` (verificación: PR creado).
- [x] 3.2 `docs/FEATURE-036-paginacion-mcp-snapshots-143.md` con qué es, ficheros, decisiones y cómo verificar (verificación: el documento existe).
- [x] 3.3 Renombrar `plans/PLAN-036.md` a `plans/PLAN-036-COMPLETED.md` con `git mv` (verificación: renombrado).
- [x] 3.4 Archivar el change: `openspec archive paginacion-mcp-snapshots-143` (verificación: pasa a `openspec/changes/archive/` y el requisito nuevo entra en `openspec/specs/mcp/`).
