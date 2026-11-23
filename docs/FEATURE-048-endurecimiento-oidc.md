# FEATURE-048 — Endurecimiento del flujo OIDC y de los tokens de API

## Qué es

Este change cierra los NIT de SEC‑22 que tocaban el flujo OIDC y el hash de los
tokens de API:

1. **Caché del JWKS con TTL de una hora** y recarga única cuando el `kid` del ID
   token no está en el conjunto cacheado.
2. **Comparación del `nonce` en tiempo constante** con `subtle::ct_eq`.
3. **PKCE S256** en el login y en el canje del código.
4. **Hash HMAC‑SHA256** del token de API con una clave derivada de
   `KESTO_MASTER_KEY` y la etiqueta `kesto/api-tokens/v1`.

No cambia el contrato de los endpoints ni el esquema de la base: el hash sigue
ocupando la columna `api_tokens.token_hash`, pero ahora es un HMAC y no un
SHA‑256 pelado. Los tokens emitidos antes de este cambio dejan de validar y hay
que reemitirlos.

## Ficheros implicados

| Fichero | Papel |
|---|---|
| `src/auth/oidc.rs` | `build_authorization_url` con PKCE, `canjear_codigo` con `code_verifier`, `fetch_jwks_cached`, `verificar_id_token` y `comparar_nonce`. |
| `src/state.rs` | `JWKS_TTL`, `JwksCache`, `AuthState.jwks_cache`, `PendingLogin.code_verifier` y `registrar_login`. |
| `src/db/crypto.rs` | `Clave` con la clave de cifrado y la clave HMAC de tokens; `API_TOKEN_ETIQUETA`. |
| `src/db/entities/api_token.rs` | `hash_de_token` con HMAC, `Tokens.hmac_key` y `AltaDeToken.en_claro`. |
| `src/api/tokens.rs` | Alta de token: pasa el valor en claro a la capa y deja que ella hashee. |
| `src/api/auth.rs` | Login guarda `code_verifier`; el callback rechaza un pendiente sin él y lo envía al canje. |
| `src/auth/middleware.rs` | Verificación de un bearer con el HMAC derivado. |
| `src/test_support/proveedor_falso.rs` | `token_endpoint` dinámico para probar dos logins seguidos y la caché JWKS. |
| `Cargo.toml` / `Cargo.lock` | Dependencias directas `hmac` y `subtle`. |

## Cómo funciona

### Caché JWKS

`AuthState` guarda `Arc<RwLock<Option<(JwkSet, Instant)>>>`. La primera
verificación pide el `jwks_uri`, guarda el conjunto y su caducidad
(`Instant::now() + JWKS_TTL`, una hora). Las siguientes reutilizan el conjunto
mientras no haya caducado.

Si el `kid` del ID token no está en el conjunto cacheado, `verificar_id_token`
fuerza **una** recarga contra el `jwks_uri` —ignorando el TTL—, actualiza la
caché y vuelve a buscar. Si sigue sin aparecer, rechaza el token. El `kid` no
está en la caché por sí solo: la recarga se hace una vez por verificación, no en
bucle.

La lectura y la escritura del `RwLock` van en bloques separados: el guard de
lectura se suelta antes de pedir el `jwks_uri`, así que una petición lenta no
bloquea a las demás durante la recarga.

### `nonce` en tiempo constante

`comparar_nonce` usa `subtle::ConstantTimeEq` sobre los bytes de ambos valores.
La comparación no corta en el primer byte distinto, así que no filtra el prefijo
del `nonce` esperado por el tiempo de respuesta.

### PKCE S256

`build_authorization_url` genera un `code_verifier` alfanumérico de 128
caracteres, calcula `code_challenge = base64url(SHA-256(code_verifier))` sin
relleno y añade `code_challenge` y `code_challenge_method=S256` a la URL de
autorización. El `code_verifier` viaja en `AuthorizationRequest` y se guarda
junto al `state` y al `nonce` en `AuthState.logins`.

El callback recupera el pendiente por el `state`, rechaza el login si el
`code_verifier` está vacío y lo envía como `code_verifier` en el `POST` al
`token_endpoint`. Sin ese valor no se canjea nada.

### Hash HMAC de los tokens de API

`Clave` deriva dos materiales de `KESTO_MASTER_KEY` con HKDF‑SHA256:

- `kesto/secrets/v1`, la clave AES‑256‑GCM de los secretos en reposo.
- `kesto/api-tokens/v1`, la clave HMAC de los tokens de API.

`hash_de_token(en_claro, key)` calcula `HMAC-SHA256(key, en_claro)` y lo codifica
en base64 estándar. La columna `token_hash` guarda ese valor; el token en claro
no se persiste. `Tokens` recibe la clave ya derivada y la envuelve en
`Zeroizing` para borrarla al caer.

La etiqueta es distinta de la de los secretos a propósito: cambiar la etiqueta de
los secretos invalidaría las filas ya cifradas, y la de los tokens debe poder
rotarse por su cuenta.

## Variables y configuración

No hay variables nuevas.

- `KESTO_MASTER_KEY` sigue siendo obligatoria: de ella salen las dos claves
  derivadas.
- Rotar `KESTO_MASTER_KEY` deja ilegibles los secretos guardados y **también**
  invalida todos los tokens de API emitidos, porque su HMAC ya no coincide.

## Cómo habilitar / desplegar

Es un cambio de código, no de configuración. Se despliega con el binario nuevo.
Tras el despliegue hay que **reemitir** los tokens de API que estuvieran en uso:
los antiguos se hashearon con SHA‑256 pelado y ya no validan.

## Decisiones

- **TTL de una hora para el JWKS.** Suficiente para no pedir el conjunto en cada
  login y corto como para recoger una rotación de claves del proveedor sin
  reiniciar Kesto.
- **Recarga única por `kid` ausente.** Si el proveedor publica una clave nueva,
  el primer token firmado con ella fuerza la recarga; no se recarga en cada
  petición ni se entra en un bucle si el `kid` no existe.
- **PKCE obligatorio en el flujo.** El callback rechaza un pendiente sin
  `code_verifier`; no hay un camino degradado sin PKCE.
- **HMAC en vez de SHA‑256 pelado.** Un atacante con lectura de la tabla ya no
  puede precomputar hashes de tokens sin la clave derivada de la maestra.
- **Etiqueta `kesto/api-tokens/v1` separada de `kesto/secrets/v1`.** Dominios de
  separación distintos para dos usos distintos de la misma maestra.

## Follow‑ups

- Reemitir los tokens de API existentes tras el despliegue.
- Actualizar `docs/ARQUITECTURA.md` con la separación de etiquetas y el TTL del
  JWKS.
- Archivar el change en `openspec/changes/endurecimiento-oidc/` cuando la
  verificación final esté en verde.
- El comentario en línea de `migrations/0009_api_tokens.sql` sobre `token_hash`
  sigue diciendo «SHA-256 del valor en claro»; ahora es HMAC-SHA256. **No se
  edita la migración ya aplicada** (cambiaría el checksum de sqlx y rompería
  `sqlx::migrate!`); el comentario se corrige aquí y en `docs/ARQUITECTURA.md`.
