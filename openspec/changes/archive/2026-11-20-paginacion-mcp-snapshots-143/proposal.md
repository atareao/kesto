# Proposal

## Why

La herramienta MCP `list_snapshots` no expone `limit` ni `offset`: su entrada `PorRepositorio` tiene `deny_unknown_fields` y un único campo (`repository_id`). Para no regresar al introducir la paginación en `GET /api/v1/snapshots` (#140), el MCP fijó un `limit=200` **explícito** en la llamada (constante `LIMITE_DE_SNAPSHOTS`, `kesto-mcp/src/server.rs`). Consecuencia: el listado de un repositorio con **más de 200** instantáneas queda truncado y el agente **no tiene forma de pedir el resto** (issue #143).

La constante es, además, una **copia** de `LIMITE_MAXIMO` de `src/api/snapshots.rs`: el paquete raíz `kesto` es un binario sin `lib.rs` y no es enlazable, así que el tope vive duplicado y hay que acompañarlo a mano.

## What Changes

- **Entrada** de `list_snapshots`: `PorRepositorio` gana `limit: Option<i64>` y `offset: Option<i64>`, ambos con `#[serde(default)]`, **preservando** `deny_unknown_fields`. Se envía a la API solo lo que el agente informe.
- **Defecto**: se **elimina** el `limit=200` fijo y la constante `LIMITE_DE_SNAPSHOTS`. Cuando el agente no informa `limit`, el MCP **no manda** `limit` y aplica el defecto de la API (`LIMITE_POR_DEFECTO`, 50); cuando lo informa, lo reenvía tal cual (el tope `200` y el `400` por `limit` fuera de rango los valida la API). Así el MCP y la API **coinciden** en defecto y tope, y desaparece la constante duplicada.
- **Contrato publicado**: la descripción de la herramienta pasa a decir que la respuesta trae `total`, que puede venir **truncada** (`truncated`) y que, si lo está, hay que paginar con `offset`. El cuerpo de la API ya viaja entero al agente —`snapshots`, `total` y `truncated`—, así que el indicador de truncado no requiere código nuevo: solo documentarlo.
- **Pruebas** (`kesto-mcp/tests/herramientas.rs`): se actualiza la que hoy exige `limit=200` (deja de mandarse `limit` si el agente no lo pide) y se añaden casos de reenvío de `limit` y `offset`, y de la descripción/paginación.

## Non-goals

- NO se toca `GET /api/v1/snapshots` ni el motor: la paginación con `total` y `truncated` ya existe desde #140.
- NO se cambia el catálogo de herramientas (siguen siendo nueve) ni el scope `read` de `list_snapshots`.
- NO se unifica la constante en un crate compartido (requeriría un `lib.rs` en el paquete raíz, fuera de alcance): al eliminar el valor fijo, la duplicación **desaparece** por sí sola.
- NO se añaden herramientas nuevas ni se hace paginar a las demás de lectura.

## Capabilities

### New Capabilities

_(ninguna)_

### Modified Capabilities

- `mcp`: la herramienta de lectura `list_snapshots` pasa a aceptar `limit`/`offset` opcionales, a no fijar un límite propio y a documentar `total`/`truncated` para que el agente sepa cuándo paginar.
