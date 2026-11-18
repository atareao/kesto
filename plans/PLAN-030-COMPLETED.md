# PLAN-030 — Servidor MCP para gestionar las copias con IA

- **Issue:** #119
- **Estado:** en curso
- **Rama:** `feature/mcp`
- **Base:** `development`

## Objetivo

Que un agente de IA pueda operar Kesto con criterio: consultar el estado de las copias, lanzar una cuando toca y comprobar que el repositorio responde. Sin operaciones destructivas.

Dos entregas encadenadas:

1. **Tokens de API con scopes** en Kesto — prerrequisito. Hoy la única autenticación es la cookie de sesión OIDC, y una máquina no puede usarla.
2. **Crate `kesto-mcp`** — servidor MCP con el SDK oficial `rmcp`, transportes `stdio` y `Streamable HTTP`, contra la API de Kesto con `Authorization: Bearer `.

## Contexto actual

- Backend Rust (axum) en `src/`, con `src/api/*` por recurso y `src/auth/` para sesión OIDC.
- La autenticación por cookie se resuelve en la capa de sesión; en modo `disabled` no hay capa y todo `/api/v1/*` queda abierto.
- El panel consume la API con `fetch` y cookie; no hay ningún cliente con token.
- El proyecto usa `sqlx` con migraciones numéricas (`0001_init.sql`): una tabla nueva de tokens exige migración nueva, nunca editar una existente.

## Fase 1 — Tokens de API (en Kesto)

- [ ] **T1.** Migración `00NN_api_tokens.sql`: tabla `api_tokens` (id, nombre, hash del token, scopes, creado, último uso, revocado). Guardar **solo el hash**, nunca el token en claro.
- [ ] **T2.** Endpoints `/api/v1/tokens`: crear (devuelve el token una sola vez), listar (sin el token), revocar. Protegidos por sesión OIDC.
- [ ] **T3.** Capa de autenticación: aceptar `Authorization: Bearer <token>` en `/api/v1/*` junto a la cookie; resolver scopes (`read`, `run`) y aplicarlos por ruta. En modo `disabled` no se exige nada, como hoy.
- [ ] **T4.** Tests: token válido, revocado, caducado, scope insuficiente, token ausente. Que un `read` no pueda lanzar una copia.

## Fase 2 — Servidor MCP (`kesto-mcp`)

- [ ] **T5.** Nuevo crate en el workspace (`kesto-mcp/`) con `rmcp`; cliente HTTP de la API y configuración por entorno (`KESTO_URL`, `KESTO_API_TOKEN`).
- [ ] **T6.** Herramientas de lectura: `list_repositories`, `list_targets`, `list_jobs`, `list_snapshots`, `list_executions`, `get_execution`, `get_health`.
- [ ] **T7.** Herramientas de escritura: `run_backup` (scope `run`) y `check_repository_health`.
- [ ] **T8.** Transporte `stdio` y `Streamable HTTP`; en HTTP, token por cabecera. Sin herramientas destructivas.
- [ ] **T9.** Tests: cada herramienta contra un servidor de pruebas; error claro cuando falta token o scope.

## Criterios de aceptación

1. Un token `read` consulta todo y **no** puede lanzar copias; un token `run` sí.
2. El token se muestra una sola vez al crearlo y nunca se guarda en claro.
3. Un token revocado deja de funcionar de inmediato.
4. El MCP arranca por `stdio` y por HTTP, y las herramientas listadas funcionan contra la API real.
5. `just check` y `just check-all` en verde; `cargo clippy -- -D warnings` sin avisos.

## Verificación

```bash
just check
just check-all
# MCP contra co1
KESTO_URL=https://kesto.territoriolinux.es KESTO_API_TOKEN=... ./target/release/kesto-mcp
```

## Riesgos

- **Token en claro en logs o en la UI:** se enseña una vez y se guarda el hash; nada de `Debug` sobre el token.
- **Scopes que se quedan cortos o se pasan:** `read` y `run` primero; cualquier scope destructivo exige un change aparte.
- **Compatibilidad con el modo `disabled`:** no romper el arranque sin OIDC, que es el modo de desarrollo.
- **Dependencia nueva:** `rmcp` y su versión fijada; revisar que no arrastre un runtime distinto.

## Documentación

Al cerrar: `docs/FEATURE-030-mcp.md` con las herramientas, variables de entorno y un ejemplo de configuración para opencode/Claude Desktop; renombrar este plan a `PLAN-030-COMPLETED.md`.