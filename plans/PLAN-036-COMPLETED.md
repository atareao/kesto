# PLAN-036 — Exponer `limit`/`offset` en la MCP `list_snapshots` (#143)

- **Issue:** #143 · **Change OpenSpec:** `paginacion-mcp-snapshots-143` · **Rama:** `feature/paginacion-mcp-snapshots-143` (desde `development`).
- **Tipo:** servidor MCP. **No** toca la API, el motor ni el catálogo (siguen nueve herramientas, todas de lectura).

## 1. Objetivo

Que la herramienta MCP `list_snapshots` acepte `limit` y `offset` **opcionales** para que un agente pueda paginar un repositorio con más de 200 instantáneas. Hoy su entrada `PorRepositorio` (con `deny_unknown_fields`) solo tiene `repository_id` y la herramienta fija un `limit=200` propio: el listado de >200 instantáneas queda truncado y el agente no puede pedir el resto.

## 2. Contexto

- La API `GET /api/v1/snapshots` (paginación de #140) ya acepta `limit`/`offset`, con defecto `LIMITE_POR_DEFECTO=50`, tope `LIMITE_MAXIMO=200` y `400` fuera de rango, y devuelve la envoltura `{snapshots, total, truncated}`.
- `kesto-mcp/src/server.rs` tiene la constante `LIMITE_DE_SNAPSHOTS=200` —copia de `LIMITE_MAXIMO`— porque el paquete raíz `kesto` es un binario sin `lib.rs` y no es enlazable; el `limit=200` se manda fijo.
- Precedente a imitar: `FiltroDeEjecuciones` (en el mismo `server.rs`) ya modela `limit`/`offset` como `Option<i64>` con `#[serde(default)]`.

## 3. Decisión de arquitectura

- Exponer `limit`/`offset` opcionales en `PorRepositorio` (con `#[serde(default)]`), reenviándolos solo si el agente los informa.
- **Eliminar** el `limit=200` fijo y la constante `LIMITE_DE_SNAPSHOTS`: si el agente no informa `limit`, el MCP no manda `limit` y aplica el defecto de la API (50). Así MCP y API coinciden en defecto y tope y desaparece la constante duplicada.
- El indicador de truncado **ya viaja** al agente: el MCP reenvía el cuerpo de la API (`snapshots`, `total`, `truncated`). Solo hay que documentarlo en la descripción de la herramienta.
- Alternativa descartada: mantener el `limit=200` por defecto para no reducir la ventana de quien no pagina; se descarta porque perpetúa la duplicación y la inconsistencia con la API, y la descripción nueva enseña al agente a paginar con `offset`.

## 4. Alcance

- `kesto-mcp/src/server.rs`: `PorRepositorio` (+`limit`/`offset`), `list_snapshots` (reenvío condicional), eliminación de `LIMITE_DE_SNAPSHOTS`, descripción de la herramienta.
- `kesto-mcp/tests/herramientas.rs`: actualizar el test que exige `limit=200` y añadir casos de reenvío y de descripción.
- Spec: requisito nuevo en la capability `mcp`. Doc: `docs/FEATURE-036-paginacion-mcp-snapshots-143.md`.

## 5. Tareas (TDD)

### 0. Preparación
- [ ] 0.1 `just check` en verde antes de tocar nada.
- [ ] 0.2 Localizar `LIMITE_DE_SNAPSHOTS`, el `limit=200` fijo y el test que lo exige.

### 1. `limit`/`offset` en `list_snapshots`
- **RED**
  - [ ] 1.1 Test: invocar solo con `repository_id` → la petición a la API NO lleva `limit`.
  - [ ] 1.2 Test: invocar con `limit` y `offset` → se reenvían tal cual, sin recortar el `limit`.
  - [ ] 1.3 Test: la descripción anunciada de `list_snapshots` menciona `total`, `truncated` y `offset`.
- **GREEN**
  - [ ] 1.4 `PorRepositorio` gana `limit`/`offset` `Option<i64>` con `#[serde(default)]`, conservando `deny_unknown_fields`.
  - [ ] 1.5 `list_snapshots` arma la consulta con `repository_id` y solo los de `limit`/`offset` informados.
  - [ ] 1.6 Eliminar `LIMITE_DE_SNAPSHOTS` + su rustdoc y actualizar la descripción de la herramienta.
- **REFACTOR**
  - [ ] 1.7 Rustdoc de `PorRepositorio` (por qué no fija un `limit` propio).
  - [ ] 1.8 `just check` en verde.

### 2. Verificación
- [ ] 2.1 `cargo test --locked -p kesto-mcp` en verde.
- [ ] 2.2 `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios.

### 3. Cierre
- [ ] 3.1 PR a `development` con `Closes #143`.
- [ ] 3.2 `docs/FEATURE-036-paginacion-mcp-snapshots-143.md`.
- [ ] 3.3 Renombrar `plans/PLAN-036.md` → `plans/PLAN-036-COMPLETED.md` con `git mv`.
- [ ] 3.4 `openspec archive paginacion-mcp-snapshots-143`.

## 6. Criterios de aceptación

- Un agente que invoca `list_snapshots` sin `limit` obtiene el defecto de la API (la petición no lleva `limit`).
- Un agente que informa `limit`/`offset` los ve reenviados tal cual; un `limit` fuera de rango lo rechaza la API con `400` (el MCP no lo recorta).
- La descripción de la herramienta cita `total`, `truncated` y la paginación con `offset`.
- `LIMITE_DE_SNAPSHOTS` ya no existe (sin código muerto ni duplicación).
- Catálogo intacto: nueve herramientas, todas de lectura; `list_snapshots` con scope `read`.
