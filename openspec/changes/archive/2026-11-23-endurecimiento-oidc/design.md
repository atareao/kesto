# Design — endurecimiento del flujo OIDC y de los tokens de API

## Contexto

Hoy `src/auth/oidc.rs` es stateless salvo por la caché de `discovery` en `AuthState` (`src/state.rs`, `Arc<RwLock<Option<Discovery>>>`). Cada verificación de ID token llamba a `fetch_jwks` (`src/auth/oidc.rs`), que crea un `reqwest::Client` nuevo y va al `jwks_uri`. El `nonce` se compara en la línea 441 con `!=`. No hay PKCE. `src/db/entities/api_token.rs:216` hashea con SHA-256 pelado y busca por `token_hash`.

`AppState` no tiene cliente HTTP compartido; cada función crea el suyo. El mapa de `logins` pendientes vive en `AuthState` (patrón `MAX_LOGINS` + purga).

## Decisiones

### 1. Caché JWKS con TTL (1 hora)

**Alternativas descartadas:**

- **TTL simple sin reintento.** Si la caché expira y el proveedor rota la clave con un `kid` nuevo, un token válido firmado con la clave nueva se rechaza hasta el siguiente TTL: un login en regla falla por azar de rotación. Descartado porque rompe la promesa de que un token en regla no se rechaza por una recarga que se pudo intentar.
- **Caché indefinida.** Sin TTL, una clave revocada por el proveedor sigue en la caché hasta el reinicio: un token firmado con una clave ya retirada se acepta. Descartado porque la caché se convierte en una ventana de aceptación de claves malas.

**Decisión: TTL de 1 hora + reintento único por `kid` desconocido.**

- Si llega un `kid` que la caché no conoce, se fuerza una recarga del JWKS.
- Si tras recargar el `kid` sigue ausente, la verificación falla (401).
- Una recarga fallida con una caché fresca válida **no** tumba una verificación que la caché puede servir: la verificación usa la caché existente.

**Dónde vive:** en `AuthState` (`src/state.rs`), junto a `discovery`, como `Arc<RwLock<Option<JwkSet>>>` más su marca de tiempo de expiración. No se añade cliente HTTP compartido: las funciones de `oidc.rs` siguen creando el suyo; la caché solo guarda el resultado de la descarga.

### 2. `nonce` en tiempo constante

**Alternativas descartadas:**

- **Comparación manual con `==` en bucle.** Es fácil introducir un bug (salir antes de tiempo) y no hay forma de probar que es constante salvo inspección manual. Descartado: `subtle` está probado y su coste es despreciable.
- **No tocar el `nonce`.** El problema es real: la comparación con `!=` sobre `String` es sensible al tiempo. Descartado.

**Decisión: `subtle` (dependencia directa).**

- Se usa `ct_eq` de `subtle` sobre los bytes del `nonce` esperado y el recibido.
- Mismo rechazo si no coincide: no cambia la semántica, solo el tiempo de ejecución.

### 3. PKCE S256

**Alternativas descartadas:**

- **PKCE `plain`.** `code_challenge = code_verifier`. Es vulnerable a un atacante que intercepta el `code_verifier` en tránsito o en el log. Descartado: S256 es el método recomendado y no añade coste.
- **PKCE opcional** (`code_challenge` si el proveedor lo soporta). Hace el flujo inseguro por defecto y complica la spec. Descartado: S256 obligatorio.

**Decisión: S256 obligatorio.**

- Se genera un `code_verifier` de 43-128 caracteres de alta entropía por login (mismo generador que el `state`/`nonce`, o su equivalente).
- `code_challenge` = `base64url(SHA-256(code_verifier))` sin relleno.
- El verifier se guarda junto al `state`/`nonce` del login pendiente en el mapa de `logins` de `AuthState`, sin cambiar el modelo del mapa (solo se añade un campo más al registro).
- `build_authorization_url` añade `code_challenge` y `code_challenge_method=S256` a los parámetros de la redirección.
- `canjear_codigo` añade `code_verifier` al cuerpo del POST al `token_endpoint`.
- Un login sin verifier (p. ej. uno ya en curso que no lo tenga) no se emite: el cambio se aplica a todos los logins nuevos.

### 4. Hash del token de API con HMAC-SHA256

**Alternativas descartadas:**

- **Sal aleatoria por token (bcrypt/argon2).** Rompería la búsqueda por `token_hash` UNIQUE: cada token tendría un hash distinto y no se podría buscar por `WHERE token_hash = ?1`. Descartado: el diseño exige lookup determinista.
- **SHA-256 pelado (lo que hay hoy).** Sin secreto, una base filtrada permite atacar los hashes offline sin ninguna clave. Descartado.

**Decisión: HMAC-SHA256 con secreto de servidor.**

- Se deriva una clave de servidor de `KESTO_MASTER_KEY` con HKDF y la etiqueta `kesto/api-tokens/v1` (etiqueta **nueva**, distinta de `kesto/secrets/v1` que ya se usa para los secretos cifrados).
- `hash_de_token` pasa a ser `base64(HMAC-SHA256(clave_servidor, token_en_claro))`.
- Es determinista: la búsqueda por `token_hash` (UNIQUE) sigue funcionando sin migración ni columna nueva.

**Consecuencias:**

- **Los tokens de API existentes dejan de validar** tras el despliegue: su hash (SHA-256) no coincide con el nuevo (HMAC). Hay que reemitirlos. Se documenta.
- **Rotar `KESTO_MASTER_KEY` invalida los tokens**, igual que ya invalida los secretos cifrados.

## Impacto en la compatibilidad

- **Tokens de API**: los existentes dejan de validar tras el despliegue; hay que reemitirlos. Rotar `KESTO_MASTER_KEY` los invalida.
- **Flujo OIDC**: sin cambios de forma en las respuestas ni en las rutas; la redirección de login añade `code_challenge` y `code_challenge_method`, y el canje añade `code_verifier`. El resto es transparente para el usuario.
- **Sin migración de base de datos** para el cambio del hash: la columna `token_hash TEXT NOT NULL UNIQUE` se mantiene.