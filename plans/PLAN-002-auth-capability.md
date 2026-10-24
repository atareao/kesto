# PLAN-002 — `auth-capability`

> **Estado:** en curso (implementación terminada y auditada; queda el cierre documental y el archivo del change).
> **Rama:** `feature/auth-capability`, desde `development`.
> **Change de OpenSpec:** `openspec/changes/auth-capability/`.
> **Especificación de origen:** `docs/DEFINITION.md` §5 (endpoints `/api/v1/*`) y §8 (variables de entorno). El SRS **no define ningún endpoint de autenticación**: ese es el hueco que cierra este change.
> **Cambio anterior:** `bootstrap-core` (`PLAN-001-COMPLETED.md`, `docs/FEATURE-001-bootstrap-core.md`, 25 tests).

## Objetivo

Cerrar las dos primeras entradas de «cabos sueltos del SRS»: no existe autenticación en ninguna capa y el panel publica el puerto 8080 sin puerta. Al terminar, todo `/api/v1/*` —**el stream SSE incluido**— exige sesión, salvo la salud y los propios endpoints de autenticación; la autenticación se delega en un proveedor OIDC externo; y sin proveedor configurado el modo de desarrollo sigue funcionando sin capa, anunciándolo en el arranque.

## Alcance

**Dentro:**

1. Sesión sin estado en el cliente: JWT HS256 en cookie `HttpOnly`.
2. Flujo Authorization Code + PKCE-libre contra un proveedor OIDC: descubrimiento, `state`, `nonce`, canje del código y verificación del ID token contra el JWKS.
3. Middleware de puerta sobre la mitad protegida del router.
4. Modo `Disabled` (sin las tres variables `OIDC_*`) que no pone puerta ninguna.
5. Errores envueltos en la envoltura única de `api-shell`, sin filtrar detalle interno.

**Fuera:** refresh tokens, revocación de sesiones (el JWT vive hasta su `exp`), roles y autorización por recurso, `/api/v1/auth/me` con datos del proveedor, logout del lado del proveedor (el logout es solo local), y cualquier forma de autenticación básica o por API key.

## Módulos

El change se reparte en cuatro módulos entregables, cada uno con su ciclo RED → GREEN → auditoría independiente → correcciones:

| # | Módulo | Contenido | Ficheros |
|---|---|---|---|
| M1 | Sesión y errores | JWT HS256, cookie, `AuthState`, los errores nuevos con su envoltura | `src/auth/session.rs`, `src/error.rs`, `src/state.rs` |
| M2 | Puerta y partición del router | Middleware que exige sesión, `protected_router()` y el orden que lo sostiene | `src/auth/middleware.rs`, `src/api/mod.rs` |
| M3 | Descubrimiento y login | Metadatos del emisor, `state`/`nonce`, URL de autorización, redirección | `src/auth/mod.rs`, `src/auth/oidc.rs`, `src/api/auth.rs` |
| M4 | Callback y verificación | Canje del código, JWKS, verificación del ID token, emisión de la cookie | `src/auth/oidc.rs`, `src/api/auth.rs` |

## Decisiones tomadas

### Sesión

- **JWT HS256 firmado con `KESTO_SECRET`** en cookie `HttpOnly`, `SameSite=Strict`, `Path=/`, `Max-Age` de 8 horas, **sin `Secure`** (el servicio se sirve por HTTP dentro de la red interna; ponerlo impediría la cookie en el despliegue previsto). `KESTO_SECRET` pasa a ser obligatoria en modo OIDC.
- **La clave vive en memoria** mientras el proceso vive: no hay tabla de sesiones ni consulta a la base de datos en cada petición. Consecuencia aceptada y documentada: rotar `KESTO_SECRET` invalida todas las sesiones.
- **Sin `tower-sessions` ni `openidconnect`**: la caché local de cargo es el límite duro del repositorio (compila sin red) y ninguno de los dos estaba en ella. El flujo se escribe a mano con `reqwest` y `jsonwebtoken`. Consecuencia: más código propio que auditar, a cambio de poder compilar.

### Forma del contrato HTTP

- **`401` sin sesión, `405` solo con ella.** En la mitad protegida, el fallback de método no permitido se inyecta **antes** del `route_layer` en el código del router, no después: medido sobre la forma exacta del router, el orden inverso hace que el `405` **se salte la puerta entera** y filtre qué métodos existen sin sesión. Es el detalle más frágil del change y está candadeado por test.
- **Ruta no declarada → `404` sin puerta**, comportamiento deliberado de `Router::route_layer` (no envuelve `fallback_router`), fijado por test.
- **`502` cuando el login no puede alcanzar al proveedor**, no `500`: es un fallo de un servicio de terceros, no del propio. El cuerpo lleva el mensaje genérico de todo `5xx`; la causa va al log.
- Todo rechazo sale en la envoltura `{status, message}` de `api-shell`. El rechazo del extractor de la consulta del callback se traduce a mano: si no, `?code=a&code=b` contesta `400` en `text/plain` con la estructura interna del struct en el cuerpo.

### Modo `Disabled`

- Sin ninguna de las tres variables `OIDC_*`, **no hay capa**: la mitad protegida deja de existir y se sirve el router entero sin puerta. El arranque lo anuncia con un `WARN` y `GET /api/v1/health` contesta `auth_mode: "disabled"`, que es lo que hay que consultar para saber qué modo está activo.
- Las tres variables son **un todo o nada**: las tres presentes encienden la autenticación; unas sí y otras no es **error de arranque**, no un modo degradado. Degradar dejaría el panel abierto sin que nadie se entere.

### Arnés de pruebas

- **`wiremock` con un par de claves RSA real y firma RS256 real**, en vez de dobles de trait. Se descartó el doble porque dejaría sin probar justo el requisito central del change: la verificación del ID token. Coste: ~5 tests dependen de un `127.0.0.1` efímero y el arnés necesita saber levantar el bucle local.
- **Sin reloj inyectable**: `AppState` no se deforma para los tests. El TTL de los `state` pendientes se prueba construyendo la entrada con una marca de tiempo ya caducada.
- **El RED de Rust se monta con firmas que compilan** (`todo!()`), porque un test que no compila se lleva por delante la suite entera, incluidos los tests heredados.

### Versiones fijadas por la caché

| Crate | Versión | Por qué |
|---|---|---|
| `jsonwebtoken` | 11.1.0, `default-features = false` + `rust_crypto` + `use_pem` | `aws_lc_rs` es opcional, **no** el defecto: la justificación inicial era errónea y se corrigió en `tasks.md` y `Cargo.toml` |
| `reqwest` | 0.12.28 | `rustls-tls` dejó de existir en 0.13 |
| `rand` | 0.8.8 | `jsonwebtoken` exige `rand ^0.8.5` |

## Trampas de `jsonwebtoken` 11.1.0 encontradas y candadeadas

Las cinco costaron un hallazgo de auditoría cada una o fueron la causa directa de un defecto:

1. `Validation::new` **solo exige `exp`** por defecto: `iss` y `aud` **no** son obligatorios salvo que se añadan con `set_required_spec_claims`.
2. La `leeway` por defecto es de **60 s**.
3. `iat` **nunca** lo valida la biblioteca: hay que comprobarlo a mano.
4. `Jwk::from_encoding_key` espera DER **PKCS#1**, no PKCS#8.
5. El `kid` se lee con `decode_header`.

Corolario: la comprobación de presencia de `exp` corre contra el **mapa crudo de claims**, no contra la struct `IdTokenClaims`; por eso `exp` **no** se declara en esa struct, deliberadamente.

## Auditado y corregido

Cada módulo pasó por `rust-reviewer` (solo lectura) y sus hallazgos se aplicaron antes de seguir. Resumen de lo que encontró la auditoría y lo que cambió el código:

- **M1**: candado de la `leeway`.
- **M2**: regresión del puerto en el test de apagado ordenado, y la corrección de que son **dos candados y no uno** los que sostienen el orden del router.
- **M3**: tres `SHALL` de la especificación sin ningún candado, y DNS en los tests (resuelto con el bucle local).
- **M4**: un «blocker» que resultó ser **falso positivo** —refutado con medición: un token **sin `exp` sí** se rechaza— y **un defecto real** encontrado y corregido: `?code=a&code=b` devolvía `400` en `text/plain` con el detalle de la struct, rompiendo la envoltura.
- **M4b**: auditoría de cierre, **sin defectos**; sus dos hallazgos son de calidad de la prueba, y se aplicaron en el commit siguiente: el escenario 1.38 podía pasar por vacío porque su marca de control se emitía desde el hilo del test (`set_default` es **de hilo**), y los dos candados de vigencia no fijaban el motivo del rechazo.

## Verificación

Puertas del repositorio, todas en verde:

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

**91 tests** (90 unitarios + 1 de integración del arranque), frente a los 25 de `bootstrap-core`.

Especificación del change: **12 requisitos / 49 escenarios** (`auth` 10/41, `api-shell` 2/8), `openspec validate auth-capability --strict` válido.

Verificación manual contra el binario (fase 4 de `tasks.md`), nueve de las diez comprobaciones hechas por el Director con `curl` y variables de entorno; la décima —completar el flujo en el navegador contra un proveedor OIDC real y comprobar los atributos de la cookie en las herramientas de desarrollo— la ejecuta el operador, porque necesita su proveedor de identidad y un navegador. Todas las pruebas automatizadas van contra un proveedor falso: la comprobación 4.7 es la única que ejercita el flujo contra un proveedor real.

## Deuda declarada

- `session_cookie` devuelve `Result` porque `Cookie::max_age` quiere un `time::Duration` que no se puede nombrar sin añadir `cookie`/`time` como dependencia directa; la conversión va por inferencia con `try_into`.
- El mapa de logins pendientes se purga por TTL pero **no tiene tope duro**.
- El comentario de `src/auth/session.rs` que apunta el escenario 1.38 a `src/auth/mod.rs` cuando en realidad vive en `src/api/auth.rs`.
- Escenarios de robustez aún sin candado: fallo del JWKS, fallo del `token_endpoint` por causas distintas del código de estado, descubrimiento incompleto y el límite exacto del TTL de los `state` consumidos. (La confusión de algoritmo y la ausencia de `kid` **sí** quedaron candadeadas.)

## Cierre

- [ ] Tildar `tasks.md` completo (obligatorio: `openspec archive` se niega si queda una casilla sin marcar).
- [ ] `docs/FEATURE-002-auth-capability.md`.
- [ ] Actualizar `AGENTS.md` (obligatoriedad de `KESTO_SECRET` en modo OIDC; el modo de desarrollo deja de describirse como «panel sin autenticar») y `README.md`.
- [ ] Escribir el `Purpose` de `openspec/specs/auth/spec.md`: es capability nueva y no puede quedarse en `TBD`.
- [ ] Arreglar en commit aparte las dos recetas rotas del `.justfile` (`spec-status` pasa un argumento posicional que `openspec status` no acepta; `spec-archive` no pasa `--yes` y se queda esperando por `stdin`).
- [ ] `openspec archive auth-capability`.
- [ ] Renombrar este fichero a `PLAN-002-COMPLETED.md`.
- [ ] PR `feature/auth-capability` → `development` con el CI en verde. **Sin release a `main`**: se decidió publicar solo hasta `development`.
