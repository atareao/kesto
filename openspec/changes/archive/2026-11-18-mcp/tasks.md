# Tasks

## 1. Fase 1 — Tokens de API (RED)

- [x] 1.1 Tests de la migración `0009_api_tokens.sql`: tabla con `id`, `name`, `token_hash`, `scopes`, `created_at`, `expires_at`, `revoked_at`, `last_used_at`; el hash es único → verificación: `cargo test` en rojo por la tabla ausente.
- [x] 1.2 Tests de creación de token: `POST /api/v1/tokens` devuelve el valor en claro una sola vez y la base guarda solo el hash → verificación: test de integración en rojo.
- [x] 1.3 Tests de listado: `GET /api/v1/tokens` no devuelve el valor ni el hash → verificación: test de integración en rojo.
- [x] 1.4 Tests de revocación: `DELETE /api/v1/tokens/{id}` deja de aceptar el token en la petición siguiente y es idempotente → verificación: test de integración en rojo.
- [x] 1.5 Tests de la capa de auth: `Authorization: Bearer` convive con la cookie, se prefiere el token, sin credencial `401`, token inválido/revocado/caducado `401` → verificación: tests en rojo.
- [x] 1.6 Tests de scopes por ruta: `read` lee y no lanza; `run` lee y lanza; restauración y borrado `403`; la gestión de tokens exige sesión → verificación: tests en rojo.
- [x] 1.7 Tests del modo `disabled`: sin variables `OIDC_*` nada se exige, ni siquiera con un Bearer inválido → verificación: tests en rojo.

## 2. Fase 1 — Tokens de API (GREEN)

- [x] 2.1 Migración `0009_api_tokens.sql` con su `CHECK` de scopes y su índice único sobre el hash → verificación: 1.1 en verde.
- [x] 2.2 Entidad `TokenDeApi` y capa de acceso sobre el pool: crear, listar, buscar por hash, revocar, anotar último uso → verificación: 1.2–1.4 en verde.
- [x] 2.3 `src/api/tokens.rs` con las tres rutas y su contrato de respuesta → verificación: 1.2–1.4 en verde.
- [x] 2.4 Verificación de Bearer en la capa de auth, con preferencia sobre la cookie y `401` uniforme → verificación: 1.5 en verde.
- [x] 2.5 Aplicación de scopes por ruta y por método, como lista blanca de acciones `run` → verificación: 1.6 en verde.
- [x] 2.6 Montaje condicionado al modo: en `Disabled` la capa no se pone delante → verificación: 1.7 en verde.
- [x] 2.7 Filtro opcional `id` en `GET /api/v1/executions` y su test → verificación: test en verde y contrato anterior intacto.

## 3. Fase 1 — Tokens de API (REFACTOR)

- [x] 3.1 Revisar que el token no aparece en `Debug`, logs ni respuestas de error → verificación: test de que el `Debug` de la entidad no expone el valor.
- [x] 3.2 `cargo fmt --all` → `cargo clippy --all-targets -- -D warnings` → `cargo test --locked` → verificación: `just check` en verde.

## 4. Fase 2 — Crate `kesto-mcp` (RED)

- [x] 4.1 Tests del cliente HTTP contra un servidor de pruebas: `KESTO_URL` obligatoria, `KESTO_API_TOKEN` por entorno y por cabecera → verificación: `cargo test -p kesto-mcp` en rojo.
- [x] 4.2 Tests de las siete herramientas de lectura, una por una, contra el servidor de pruebas → verificación: en rojo.
- [x] 4.3 Tests de las dos herramientas de acción: `run_backup` y `check_repository_health` con `run`; rechazo con `read` → verificación: en rojo.
- [x] 4.4 Tests de error: sin token, token revocado, scope insuficiente; el servidor sigue vivo; y `get_health` sin token cuando la API está en modo `disabled` → verificación: en rojo.
- [x] 4.5 Tests de transporte: arranque por `stdio` y por `Streamable HTTP`, mismo catálogo de herramientas → verificación: en rojo.
- [x] 4.6 Test del catálogo: exactamente nueve herramientas y ninguna destructiva → verificación: en rojo.

## 5. Fase 2 — Crate `kesto-mcp` (GREEN)

- [x] 5.1 Crate `kesto-mcp/` en el workspace con `rmcp` y su binario → verificación: `cargo build -p kesto-mcp` compila.
- [x] 5.2 Cliente HTTP de la API con configuración por entorno y token fuera de logs → verificación: 4.1 en verde.
- [x] 5.3 Las siete herramientas de lectura con resultado estructurado → verificación: 4.2 en verde.
- [x] 5.4 Las dos herramientas de acción con scope `run` → verificación: 4.3 en verde.
- [x] 5.5 Mapeo de errores a mensajes que dicen qué falta → verificación: 4.4 en verde.
- [x] 5.6 Transportes `stdio` y `Streamable HTTP` con el mismo catálogo → verificación: 4.5 y 4.6 en verde.

## 6. Fase 2 — Crate `kesto-mcp` (REFACTOR)

- [x] 6.1 Revisar el catálogo y confirmar que no hay ninguna herramienta destructiva → verificación: test de 4.6 en verde.
- [x] 6.2 `cargo fmt --all` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --locked --workspace` → verificación: `just check` en verde.

## 7. Documentación y cierre

- [x] 7.1 `docs/FEATURE-030-mcp.md` con herramientas, variables de entorno y ejemplo de configuración para opencode/Claude Desktop → verificación: revisión.
- [x] 7.2 `just check-all` en verde y renombrar el plan a `plans/PLAN-030-COMPLETED.md` → verificación: `just check-all`.
