# Proposal: auth-capability

## Why

`docs/DEFINITION.md` §5 lista los recursos de negocio y no define ni un endpoint de autenticación: hoy quien alcance el `8080` del §8 puede crear repositorios con credenciales S3 y disparar dumps. Es el cabo suelto que este change cierra.

La plantilla es `shuul/backend/src/http/auth.rs`, y Kesto diverge en dos cosas. De forma: shuul devuelve una página HTML que guarda el JWT en `sessionStorage`, y aquí no vale — la `EventSource` del navegador no permite enviar cabeceras, así que la sesión viaja en cookie `HttpOnly`, que el `EventSource` manda sola al ser mismo origen. De fondo: shuul guarda la marca del `state` sin comprobarla nunca y no verifica el ID token; aquí la caducidad se aplica al consumirlo y el token se verifica contra el JWKS.

Sin las tres variables `OIDC_*` no se monta capa alguna: es el modo de desarrollo.

## What Changes

1. **`Cargo.toml` estrena cinco dependencias y una de test.** `jsonwebtoken` —con el backend `rust_crypto`, que evita la biblioteca criptográfica en C a la que apunta su backend por defecto—, `reqwest` sin features por defecto y con `rustls-tls` + `json`, `axum-extra` con `cookie`, `rand` y `urlencoding`; en dev, `wiremock` para el proveedor falso. **Las versiones son las ya cacheadas en esta máquina**, no las que suenen bien: resolverlas y dejarlas escritas es la tarea 0.1 de `tasks.md`, igual que hizo `bootstrap-core`. `openidconnect` y `tower-sessions` **no** se usan: no están en la caché local de Cargo y el repositorio compila sin red.

2. **El flujo OIDC se escribe a mano.** Descubrimiento en `{issuer}/.well-known/openid-configuration` —`authorization_endpoint`, `token_endpoint` y `jwks_uri`—, redirección al proveedor, canje del código y verificación del ID token. Se copia la forma de shuul, con `reqwest` + `jsonwebtoken`, porque no hay biblioteca OIDC completa disponible sin red. Del token **verificado** salen `sub` y `email`; no se llama a `userinfo`.

3. **Dos defectos de shuul no se copian.** Shuul guarda la marca de tiempo del `state` y jamás la comprueba, así que su TTL de cinco minutos es decorativo: aquí la caducidad se aplica al consumirlo, con escenario propio. Y shuul nunca verifica el ID token —canjea y llama a `userinfo`—: aquí se verifica firma, `iss`, `aud`, `exp` y `nonce`, con escenarios para el `aud` ajeno, el token caducado y el `nonce` distinto.

4. **La sesión es un JWT firmado en una cookie, no una cabecera.** `HS256` con `KESTO_SECRET`, con `sub`, `email`, `iat` y `exp`, y **sin roles**: el SRS no define ni uno. Cookie `HttpOnly`, `SameSite=Strict`, `Path=/` y `Max-Age` igual a la vida de la sesión —una constante de 8 horas, no una variable de entorno nueva—, con `Secure` solo si la URL de retorno es `https`. Sin estado en servidor: no necesita almacén, sobrevive a un reinicio y no depende de que exista `db-schema`.

5. **Cuatro endpoints bajo `/api/v1/auth/`.** `GET /login` y `GET /callback` públicos; `POST /logout` y `GET /session` protegidos. El `401` sale con la envoltura `{status, message}` del change anterior, nunca con cuerpo vacío ni con una redirección: la API contesta JSON y el panel decide qué hacer.

6. **El router se parte en público y protegido.** Público: la salud y las dos rutas del flujo de entrada. Protegido: **todo** lo demás de `/api/v1`, el SSE de `/api/v1/events` incluido, colgado con `route_layer`. Lo anunció el proposal de `bootstrap-core` y llega con quien lo necesita.

7. **El arranque no depende de que el proveedor esté vivo.** Si el descubrimiento falla, el proceso arranca con un aviso y reintenta en el primer login, cacheando el resultado. Kesto es el orquestador de las **copias**, y negarse a arrancar porque el proveedor de identidad está caído convertiría un problema de autenticación en ausencia de copias: el mismo criterio por el que una credencial débil avisa y arranca.

**Decisiones tomadas al abrir el change:**

- **No hay login local.** Sin proveedor no hay autenticación: el modo `Disabled` deja la superficie pública y lo anuncia. Añadir un usuario y una contraseña locales inventaría gestión de usuarios que nadie ha pedido.
- **No hay refresco de token del proveedor.** La sesión es de Kesto y vive 8 horas; cuando caduca, se vuelve a entrar. El refresh token del proveedor sería un segundo flujo que custodiar sin nadie que lo use.
- **El estado del flujo (`state`/`nonce`) vive en memoria del proceso.** Es coherente con que la sesión no tenga estado en servidor, y una entrada caducada se purga al consumirse: el mapa no crece sin límite.
- **`AGENTS.md` gana una fila de obligatoriedad.** En modo OIDC, `KESTO_SECRET` deja de ser «la clave de firma» y pasa a ser imprescindible; su ausencia ya es error de arranque desde `bootstrap-core`, pero el documento no lo dice todavía.

## Non-goals

- **No hay login local** con usuario y contraseña, ni gestión de usuarios, ni invitaciones, ni altas.
- **No hay roles ni permisos por rol.** El SRS no define un solo rol y el panel es de un operador: una sesión autenticada vale para todo, y la superficie de autorización no se inventa aquí.
- **No hay refresco de token del proveedor.** Si la sesión caduca, se vuelve a entrar.
- **No hay panel.** `ui/` sigue sin existir: este change solo publica el contrato que el panel consumirá.
- **No hay base de datos.** Ni `sqlx`, ni `migrations/`, ni el esquema del §3. La sesión no la necesita, y por eso este change puede ir antes que `db-schema`.
- **No hay comprobación de disponibilidad en la salud**: sigue siendo vida, con su `auth_mode`, como la dejó `bootstrap-core`.
- **No hay traza de peticiones.** Cuando llegue su primer consumidor, su span **no** podrá incluir la cadena de consulta: el callback llega con `?code=` y `?state=`. Queda escrito aquí y en la especificación para que no se descubra en producción.
- **No se toca el pipeline, ni la versión, ni el layout del §7:** `src/auth/` es un directorio nuevo, no un módulo del SRS.

## Impact

- `Cargo.toml` / `Cargo.lock` — las cinco dependencias nuevas y `wiremock` en dev
- `src/api/mod.rs` — la partición del router en superficie pública y protegida
- `src/api/auth.rs` — **nuevo**: los cuatro endpoints del flujo
- `src/auth/mod.rs` — **nuevo**: el descubrimiento del proveedor, con su caché y su reintento
- `src/auth/oidc.rs` — **nuevo**: la URL de autorización, el canje del código, el `state`/`nonce` y la verificación del ID token
- `src/auth/session.rs` — **nuevo**: el JWT de sesión —emisión y verificación— y los atributos de la cookie
- `src/auth/middleware.rs` — **nuevo**: el *gate* de la superficie protegida
- `src/error.rs` — **dos** variantes nuevas, ambas con la envoltura del change anterior: `Unauthorized`, que traduce a `401`, y `BadGateway`, que traduce a `502` —el login que no alcanza al proveedor de identidad falla por un servicio de terceros, y el operador tiene que ir a mirar ahí y no a buscar un fallo interno—
- `src/state.rs` — el estado de autenticación: el descubrimiento cacheado y el mapa de `state`/`nonce`
- `src/main.rs` — el descubrimiento al arrancar, su aviso y la construcción del router partido
- `openspec/specs/auth/spec.md` — capability **nueva** (`ADDED`); al archivar hay que redactar su `Purpose`, no dejarlo en `TBD`
- `openspec/specs/api-shell/spec.md` — delta (`ADDED`) con la partición y el `401` de la superficie protegida
- `AGENTS.md` — la tabla de variables gana la obligatoriedad de `KESTO_SECRET` en modo OIDC, y el modo de desarrollo deja de ser «sin autenticar» para describir el flujo de sesión
- `README.md` — el estado deja de decir que no hay autenticación
- tests: unitarios del flujo —descubrimiento, `state`/`nonce`, verificación del ID token—, del JWT de sesión, de la cookie y del `401`, contra un proveedor **falso local**: ninguno sale a internet
- **Ninguna aserción existente se debilita**: la salud sigue pública y la suite de `bootstrap-core` sigue verde sin tocar un escenario
