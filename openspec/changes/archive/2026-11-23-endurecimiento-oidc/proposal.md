# Proposal — endurecimiento del flujo OIDC y de los tokens de API

**SEC-22** del bloque **F** de la auditoría de seguridad **#156** (#168). Los NIT de SEC-22 que tocan el flujo OIDC van en este change, `endurecimiento-oidc`.

## Why

1. **El JWKS se descarga en cada verificación.** `fetch_jwks` (`src/auth/oidc.rs`) crea su propio `reqwest::Client` por llamada y va al `jwks_uri` del proveedor cada vez que se verifica un ID token. Es un coste de red innecesario, un punto de fallo por verificación y sin caché el proveedor tiene que estar vivo en cada callback.

2. **El `nonce` se compara con `!=`.** En `verificar_id_token` (`src/auth/oidc.rs:441`) el `nonce` del ID token se compara con el del login usando igualdad simple, que no es en tiempo constante. Una comparación no constante deja una huella de tiempo que, aunque pequeña, es una superficie de ataque contra un valor que el atacante puede hacer adivinar.

3. **El flujo no usa PKCE.** `build_authorization_url` (`src/auth/oidc.rs:165-172`) manda `response_type`, `client_id`, `redirect_uri`, `scope`, `state`, `nonce` pero no `code_challenge`. `canjear_codigo` (`src/auth/oidc.rs:292`) no envía `code_verifier`. Un código de autorización interceptado vale sin más, porque nadie exige el verifier: es el flujo "authorization code" puro, sin la segunda pieza de prueba que añade PKCE.

4. **El hash del token de API es SHA-256 sin secreto.** `hash_de_token` (`src/db/entities/api_token.rs:216`) es `base64(SHA-256(valor))`. Si la base se filtra, un atacante puede atacar los hashes offline sin necesidad de ninguna clave del servidor: no hay un secreto que proteja los valores de los tokens.

## What Changes

### 1. Caché JWKS con TTL (1 hora)
- El JWKS se cachea en `AuthState` (`src/state.rs`), junto a `discovery`, con un TTL de una hora y se reutiliza entre verificaciones.
- Si llega un `kid` que la caché no conoce, se fuerza una recarga del JWKS. Si tras recargar el `kid` sigue ausente, la verificación falla.
- Una recarga fallida con una caché fresca válida no tumba una verificación que la caché puede servir.

### 2. `nonce` en tiempo constante
- La comparación del `nonce` pasa a usar `subtle` (constante en tiempo), sin cambiar la semántica: mismo rechazo si no coincide.

### 3. PKCE S256
- Se genera un `code_verifier` (43-128 caracteres de alta entropía) por login.
- El `code_challenge` es `base64url(sin relleno)` del SHA-256 del verifier, con `code_challenge_method=S256`.
- El verifier se guarda junto al `state`/`nonce` del login pendiente y se envía como `code_verifier` en el canje del código.
- Un login sin verifier no se emite.

### 4. Hash del token de API con secreto de servidor
- `hash_de_token` deja de ser SHA-256 pelado y pasa a ser HMAC-SHA256 con un secreto de servidor derivado de `KESTO_MASTER_KEY` con HKDF y etiqueta `kesto/api-tokens/v1` (etiqueta nueva, distinta de `kesto/secrets/v1`).
- El resultado se codifica en base64 como hoy.
- Es determinista, así que la búsqueda por `token_hash` (UNIQUE) sigue funcionando sin migración ni columna nueva.
- Los tokens de API existentes dejan de validar (su hash cambió) y hay que reemitirlos; rotar `KESTO_MASTER_KEY` invalida los tokens.

## Non-goals

- No se añade un cliente HTTP compartido en este change (cada función de `oidc.rs` crea el suyo); la caché de JWKS convive con eso.
- No se rota la etiqueta `kesto/secrets/v1` de los secretos cifrados: solo se añade la nueva `kesto/api-tokens/v1` para los tokens de API.
- No se cambia el modelo de identidad ni se añaden roles o scopes.

## New Capabilities

_(ninguna)_

## Modified Capabilities

- `auth`: caché JWKS con TTL, `nonce` en tiempo constante, PKCE S256 en el flujo de login y canje.
- `api-tokens`: el hash de los tokens de API pasa a ser HMAC-SHA256 con un secreto derivado de `KESTO_MASTER_KEY` (etiqueta `kesto/api-tokens/v1`).

## Impact

- **Código**: `src/auth/oidc.rs` (caché JWKS, `nonce` constante, PKCE), `src/state.rs` (caché JWKS en `AuthState`), `src/db/entities/api_token.rs` (HMAC del token).
- **API**: ninguna ruta ni forma nueva; la verificación del ID token y del token de API mantienen sus contratos externos.
- **Base**: no hay migración ni columna nueva (el hash sigue en `token_hash TEXT NOT NULL UNIQUE`).
- **Despliegue**: los tokens de API existentes dejan de validar y deben reemitirse; rotar `KESTO_MASTER_KEY` invalida los tokens.
- **Dependencias nuevas**: `hmac` y `subtle` (como dependencias directas; `sha2`, `base64`, `rand`, `reqwest`, `hkdf`, `aes-gcm`, `jsonwebtoken` ya están).