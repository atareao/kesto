# Proposal

## Why

Hoy la única forma de entrar en Kesto es la cookie de sesión que emite el flujo OIDC. Un agente de IA —o cualquier cliente que no sea un navegador— no puede consultar el estado ni disparar una copia: no hay credencial que pueda llevar, y en modo `disabled` la superficie está abierta pero tampoco existe un token que identifique al llamante. El issue **#119** pide un servidor MCP para que un agente gestione las copias con herramientas acotadas. El plan de trabajo es **`plans/PLAN-030.md`**.

## What Changes

La entrega se parte en **dos fases** que se especifican y se prueban por separado, aunque viajen en el mismo change:

1. **Tokens de API.** Una tabla nueva `api_tokens` guarda **solo el hash** de cada token; el valor en claro se entrega una única vez al crearlo. Se añaden las rutas de gestión (`GET`/`POST`/`DELETE /api/v1/tokens`) y una capa de autenticación que admite `Authorization: Bearer <token>` **conviviendo** con la cookie de sesión. Los tokens llevan scopes `read` y `run`, aplicados por ruta: `read` cubre las lecturas y `run` las lecturas más las acciones que disparan trabajo. Restaurar y borrar quedan fuera del alcance de cualquier token. En modo `disabled` no se exige nada, como hoy.
2. **Crate `kesto-mcp`.** Un crate aparte del workspace, sobre `rmcp`, que expone por `stdio` y por `Streamable HTTP` un conjunto **cerrado** de herramientas de lectura y de acción. El crate es un cliente de la API con token: no toca la base, ni el socket de Podman, ni rustic. Reenvía el token del llamante y deja que la API decida los scopes.

El único cambio de superficie existente es aditivo: el listado de ejecuciones gana un filtro opcional por identificador para que `get_execution` resuelva una fila concreta sin añadir una ruta de detalle.

## Capabilities

### New Capabilities

- `api-tokens`: emisión, listado y revocación de tokens; verificación por hash; convivencia de `Authorization: Bearer` con la cookie; scopes `read`/`run` por ruta; comportamiento en modo `disabled`.
- `mcp`: crate `kesto-mcp` con `rmcp`; herramientas de lectura y de acción; transportes `stdio` y `Streamable HTTP`; errores claros sin token o con scope insuficiente; ausencia de herramientas destructivas.

### Modified Capabilities

- `api-resources`: el listado de ejecuciones acepta un filtro opcional `id`, necesario para la herramienta `get_execution`.

## Impact

- **Base de datos:** migración nueva `0009_api_tokens.sql`; entidad y capa de acceso de tokens.
- **Backend:** `src/api/tokens.rs` (rutas de gestión), `src/auth/` (verificación Bearer y scopes), `src/api/mod.rs` (montaje de la capa), `src/api/executions.rs` (filtro `id`).
- **Workspace:** crate nuevo `kesto-mcp/` con su `Cargo.toml` y su binario; `Cargo.toml` raíz del workspace.
- **Documentación:** `docs/FEATURE-030-mcp.md`.
- **Sin cambios incompatibles:** el flujo de cookie sigue igual, el modo `disabled` sigue sin capa y las rutas existentes conservan su contrato. La tabla es nueva y el filtro `id` es opcional.
