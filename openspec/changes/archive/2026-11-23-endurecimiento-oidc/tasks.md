# Tasks — endurecimiento-oidc

SEC-22 del bloque F, hija de la auditoría #156 (#168). TDD: primero los tests que hoy fallan (rojo), luego el mínimo (verde), luego el refactor.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Rama desde `development`: `just gf-feature endurecimiento-oidc` (verificación: `git branch --show-current`).
- [x] 0.3 Change aprobado por el usuario (verificación: aprobación explícita y `just check-spec` encuentra el change activo).
- [x] 0.4 Plan: `plans/PLAN-048.md`. Documento de la feature: `docs/FEATURE-048-endurecimiento-oidc.md`.

## 1. Caché JWKS con TTL

### RED

- [x] 1.1 `src/auth/oidc.rs`: añadir caché de JWKS con TTL en `AuthState` (`src/state.rs`). La primera verificación de un `kid` nuevo fuerza recarga; la caché se reutiliza tras eso. Verificación: rojo hoy, que no hay caché.
- [x] 1.2 `src/state.rs`: exponer `Option<JwkSet>` con TTL de una hora en `AuthState` junto a `discovery`. Verificación: rojo hoy.

### GREEN

- [x] 1.3 Añadir `fetch_jwks_cached` que revise la TTL y use la caché o recarge. Verificación: 1.1 en verde.

## 2. `nonce` en tiempo constante

### RED

- [x] 2.1 `src/auth/oidc.rs`: cambiar la comparación `!=` por `subtle::ct_eq` sobre los bytes del `nonce`. Verificación: rojo hoy, que usa `!=`.

### GREEN

- [x] 2.2 La comparación usa `subtle::ct_eq`; mismo rechazo si no coincide. Verificación: 2.1 en verde.

## 3. PKCE S256

### RED

- [x] 3.1 `src/auth/oidc.rs`: `build_authorization_url` debe añadir `code_challenge` y `code_challenge_method=S256`. Verificación: rojo hoy, que no los manda.
- [x] 3.2 `src/auth/oidc.rs`: `canjear_codigo` debe enviar `code_verifier`. Verificación: rojo hoy, que no lo envía.
- [x] 3.3 `src/state.rs`: el registro de `login` pendiente debe guardar el `code_verifier`. Verificación: rojo hoy, que el mapa no guarda verifier.

### GREEN

- [x] 3.4 Generar `code_verifier` (43-128 caracteres alta entropía) por login; `code_challenge` = `base64url(SHA-256(verifier))` sin relleno. Verificación: 3.1, 3.2 en verde.

## 4. Hash del token con HMAC-SHA256

### RED

- [x] 4.1 `src/db/entities/api_token.rs`: cambiar `hash_de_token` de SHA-256 pelado a HMAC-SHA256 con clave derivada de `KESTO_MASTER_KEY` (etiqueta `kesto/api-tokens/v1`) y base64 codificar. Verificación: rojo hoy, que es SHA-256 puro.
- [x] 4.2 `src/db/entities/api_token.rs`: actualizar la validación por `token_hash` (la búsqueda por `WHERE token_hash = ?1` sigue funcionando, sin columna nueva). Verificación: rojo hoy.

### GREEN

- [x] 4.3 `hash_de_token` usa HMAC-SHA256; los nuevos tokens tienen hash HMAC. Verificación: 4.1 en verde.
- [x] 4.4 Documentar que los tokens existentes dejan de validar y deben reemitirse. Verificación: revisado.

## 5. Verificación

- [x] 5.1 `just check` (verificación: fmt + clippy + test en verde).
- [x] 5.2 `just check-all` (verificación: análogo local del CI en verde: fmt + clippy + deny + scripts + ui-check).
- [x] 5.3 Ejecutar suite completa `cargo test --locked --workspace` y confirmar todo en verde.

## 6. Documentación y cierre

- [x] 6.1 `docs/FEATURE-048-endurecimiento-oidc.md`.
- [ ] 6.2 `openspec archive endurecimiento-oidc`.
- [ ] 6.3 Renombrar `plans/PLAN-048.md` a `plans/PLAN-048-COMPLETED.md`.