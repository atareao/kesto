# FEATURE-036 — `limit`/`offset` en la MCP `list_snapshots` (#143)

- **Issue:** #143 · **Change OpenSpec:** `paginacion-mcp-snapshots-143` · **Plan:** `plans/PLAN-036.md`.
- **Tipo:** servidor MCP. No toca la API, el motor ni el catálogo (siguen nueve herramientas, todas de lectura).

## Qué es

La herramienta MCP `list_snapshots` ahora acepta `limit` y `offset` **opcionales**, además de `repository_id`, para que un agente pueda paginar las instantáneas de un repositorio con más de una página. Antes no exponía ninguno de los dos: fijaba un `limit=200` propio y un listado de más de 200 instantáneas quedaba truncado sin forma de pedir el resto.

## Causa / motivo

La paginación de `GET /api/v1/snapshots` (#140) añadió `limit`/`offset` (defecto `50`, tope `200`, `400` fuera de rango) y una envoltura `{snapshots, total, truncated}`. Para no regresar, el MCP fijó un `limit=200` **explícito** mediante la constante `LIMITE_DE_SNAPSHOTS` —copia de `LIMITE_MAXIMO` de `src/api/snapshots.rs`, no enlazable porque el paquete raíz es un binario sin `lib.rs`—. El agente no tenía forma de paginar.

## Ficheros implicados

- `kesto-mcp/src/server.rs`: `PorRepositorio` (+`limit`/`offset`), `list_snapshots` (reenvío condicional), eliminación de `LIMITE_DE_SNAPSHOTS`, descripción de la herramienta.
- `kesto-mcp/tests/herramientas.rs`: test del listado sin `limit` y dos tests nuevos (reenvío de `limit`/`offset` y descripción/esquema).
- `openspec/changes/paginacion-mcp-snapshots-143/` · `docs/FEATURE-036-paginacion-mcp-snapshots-143.md` · `plans/PLAN-036.md`.

## Cómo funciona

- `PorRepositorio` gana `limit: Option<i64>` y `offset: Option<i64>` (`#[serde(default)]`), conservando `deny_unknown_fields` y con `repository_id` todavía **obligatorio**.
- `list_snapshots` arma la consulta con `repository_id` y **solo** los campos que el agente informe:
  - sin `limit` → no se manda `limit` → la API aplica su defecto (`50`);
  - con `limit`/`offset` → se reenvían **tal cual**; el tope `200` y el `400` los decide la API (el MCP no recorta).
- La descripción de la herramienta pasa a decir que la respuesta trae `total` y `truncated` y que, si viene truncada, el resto se pide con `offset`. El indicador de truncado **no** requiere código: el MCP ya reenvía el cuerpo de la API entero.

## Decisiones

- **Se elimina el `limit=200` fijo y la constante `LIMITE_DE_SNAPSHOTS`.** El defecto y el tope del MCP pasan a ser **los mismos** que los de la API: desaparece la constante duplicada y la necesidad de mantenerla a mano.
- **Contrapartida asumida:** quien invocaba sin `limit` pasaba de ver hasta 200 a ver 50. Es aceptable porque la respuesta trae `total`/`truncated` y la descripción enseña a paginar. Alternativa descartada: mantener el 200 por defecto (perpetúa la duplicación y la inconsistencia).
- **Sin indicador de truncado nuevo:** el cuerpo de la API (`total`, `truncated`) ya viaja al agente; solo se documentó.
- **`repository_id` sigue siendo obligatorio:** no se usó `#[serde(default)]` a nivel de struct (que volvería opcional `repository_id`), sino por campo.

## Cómo verificar

```sh
cargo test --locked -p kesto-mcp           # las nueve herramientas del MCP
just check                                 # fmt + clippy --workspace -D warnings + test --workspace
```

## Follow-ups

- Ninguno de esta feature. Nota: el paquete raíz `kesto` sigue sin `lib.rs`, así que sigue sin poder compartir tipos con `kesto-mcp`; no hace falta para esto.
