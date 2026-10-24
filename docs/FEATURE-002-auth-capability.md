# FEATURE-002 — auth-capability

> Implementación del change proposal `auth-capability`, archivado en `openspec/changes/archive/2026-10-24-auth-capability/` (12 requisitos, 50 escenarios, `openspec validate --strict` válido).
> Cerrada el 2026-10-24. Rama: `feature/auth-capability`, desde `development`. Plan: `plans/PLAN-002-COMPLETED.md`. Especificación de origen: `docs/DEFINITION.md` §5 y §8.
> Estado: **cerrado** — **92 tests** (91 unitarios + 1 de integración). La especificación vive ya en `openspec/specs/` —`auth`, capability nueva, y `api-shell`, que creció—, con su `Purpose` redactado tras el archivado.

**El archivo está hecho.** El delta se fundió en dos sitios: `openspec/specs/auth/spec.md`, que **nació** con este change —10 requisitos, 42 escenarios—, y `openspec/specs/api-shell/spec.md`, que **creció** de 5 requisitos / 12 escenarios a **7 / 20** con los 2 requisitos y 8 escenarios nuevos. El change, con sus 90 tareas tildadas, queda en `openspec/changes/archive/2026-10-24-auth-capability/`.

## Qué quedó construido

El SRS (`docs/DEFINITION.md`) **no define ningún endpoint de autenticación** en su §5, y su §8 publica el puerto 8080 sin puerta: era la primera entrada de «cabos sueltos del SRS». Esta feature cierra ese hueco. Todo `/api/v1/*` exige sesión salvo la salud y las dos rutas públicas del flujo de entrada; la autenticación se delega en un proveedor OIDC externo; y sin proveedor configurado el modo de desarrollo sigue funcionando sin capa, anunciándolo en el arranque.

Cifras del diff contra `development`: 16 ficheros, +6502/−204.

| Fichero | Líneas | Qué es |
|---|---|---|
| `src/auth/oidc.rs` | +1320 | Descubrimiento del emisor, URL de autorización, canje del código, JWKS y verificación del ID token, con `MARGEN_DE_RELOJ` |
| `src/api/auth.rs` | +813 | Los cinco endpoints del flujo y de la sesión |
| `src/auth/session.rs` | +471 | El JWT HS256, sus cuatro claims, la cookie y su verificación |
| `src/api/mod.rs` | +400 | La partición del router en público y protegido |
| `src/test_support/proveedor_falso.rs` | +385 | El proveedor OIDC falso: `wiremock` y un par RSA real, firma RS256 real |
| `src/state.rs` | +314 | `AuthState` |
| `src/test_support.rs` | +355 | Utilidades compartidas de las pruebas |
| `src/auth/middleware.rs` | +190 | El *gate* de sesión |
| `src/auth/mod.rs` | +125 | El montaje del estado de autenticación |
| `tests/arranque.rs` | +163 | El arranque en modo OIDC con el proveedor caído |
| `src/error.rs` | +81 | Las dos variantes nuevas |
| `src/main.rs` | +26 | El descubrimiento al arrancar y el router partido |
| `src/api/health.rs` | +17 | El `auth_mode` en el cuerpo de la salud |
| `Cargo.toml` / `Cargo.lock` | +38 / +2004 | Las dependencias del flujo |
| `.justfile` | 2 líneas | Las dos recetas de `openspec` que estaban rotas: `spec-status` pasaba el change como argumento posicional —`openspec status` solo acepta `--change`— y `spec-archive` no pasaba `--yes`, así que se quedaba esperando por `stdin` e inutilizaba la receta desde un agente o un CI |

**92 tests**, frente a los 25 de `bootstrap-core`, repartidos así: `auth/oidc.rs` 24, `api/mod.rs` 13, `config.rs` 13, `api/auth.rs` 12, `auth/session.rs` 9, `api/health.rs` 4, `error.rs` 4, `state.rs` 4, `auth/middleware.rs` 3, `secret.rs` 3, `auth/mod.rs` 2 y `tests/arranque.rs` 1.

## Contratos

Lo caro de cambiar después.

### Modos de autenticación

Dos modos, decididos por las variables de entorno y **no** por un fichero de configuración:

- **`Oidc`**: las **tres** variables `OIDC_ISSUER_URL`, `OIDC_CLIENT_ID` y `OIDC_CLIENT_SECRET` presentes. Se monta la capa.
- **`Disabled`**: **ninguna** de las tres. No hay capa ninguna: la mitad protegida no se monta.

Las tres son **un todo o nada**: unas sí y otras no es **error de arranque**, no un modo degradado —medido: `kesto no puede arrancar: la configuración de OIDC está incompleta: falta OIDC_CLIENT_ID`, exit 1—. Degradar a «apagada» dejaría el panel abierto sin que nadie se entere. `OIDC_REDIRECT_URL` a solas, sin las otras tres, **se ignora en silencio** y vale su defecto, `http://localhost:8080/api/v1/auth/callback`.

`KESTO_SECRET` pasa a ser **obligatoria en modo `Oidc`** —es la clave con la que se firma la sesión— y sigue siendo opcional en `Disabled`. Medido sin ella: `kesto no puede arrancar: falta la variable de entorno obligatoria KESTO_SECRET`, exit 1.

En modo `Disabled` el arranque lo anuncia con un `WARN` —`la autenticación está apagada: modo de desarrollo, sin sesión que verificar`— y `GET /api/v1/health` contesta `auth_mode: "disabled"`, que es lo que hay que consultar para saber qué modo está activo. **El modo de desarrollo deja el panel sin autenticar**: es para desarrollo, no para un despliegue expuesto.

### La sesión

| Elemento | Contrato |
|---|---|
| Algoritmo | JWT **HS256**, firmado con `KESTO_SECRET` |
| Claims | **exactamente cuatro**: `sub`, `email`, `iat`, `exp` |
| Transporte | Cookie `HttpOnly` llamada `kesto_session`. Nunca en cabecera |
| Vida | 8 horas: `SESSION_LIFETIME` = 28 800 s |
| Atributos | `HttpOnly`, `SameSite=Strict`, `Path=/`, `Max-Age` 28 800 |
| `Secure` | **Condicional**: se añade **solo** si el esquema de `OIDC_REDIRECT_URL` es `https` |

El `Secure` es condicional porque en desarrollo se entra por `http://localhost` y una cookie `Secure` no viajaría, así que la sesión no se establecería nunca. Una URL que no se pueda leer como `esquema://…` cuenta como no segura: el defecto útil es el de desarrollo.

La clave vive **sólo en memoria**: no hay tabla de sesiones ni consulta a la base de datos en cada petición. Consecuencia aceptada y documentada: **rotar `KESTO_SECRET` invalida todas las sesiones**. La verificación es por firma: una cookie manipulada contesta `401` con el motivo `la cookie de sesión no es válida` —medido, también con una firma hecha con otra clave y con una cookie caducada—.

### El modelo de error

Todo rechazo sale en la envoltura única de `api-shell`, `{status, message}`, y los errores **no** llevan la clave `data`.

| Variante de `AppError` | Código | Cuerpo |
|---|---|---|
| `InvalidInput` | `400` | El motivo, explicado |
| `Unauthorized` | `401` | El motivo, explicado |
| `NotFound` | `404` | El motivo |
| `MethodNotAllowed` | `405` | El motivo |
| `Other` | `500` | Mensaje genérico: `error interno del servidor`, con la causa al log |
| `BadGateway` | `502` | **El mismo mensaje genérico** que el `500`, con la causa al log |
| `Config` | `500` | El mismo mensaje genérico, con la causa al log |

Mensajes medidos en la verificación manual: sin cookie, `hace falta una sesión válida para esta ruta`; cookie inválida, `la cookie de sesión no es válida`; método no declarado, `el método no está permitido en esta ruta`; ruta no declarada, `la ruta solicitada no existe`.

## Endpoints

| Método | Ruta | Mitad | Sin sesión | Con sesión |
|---|---|---|---|---|
| `GET` | `/api/v1/health` | Pública, **en cualquier modo** | `200` con `version` y `auth_mode` | igual |
| `GET` | `/api/v1/auth/login` | Pública (solo modo `Oidc`) | `302` al proveedor | igual |
| `GET` | `/api/v1/auth/callback` | Pública (solo modo `Oidc`) | `302` de vuelta con `Set-Cookie` | igual |
| `POST` | `/api/v1/auth/logout` | Protegida | `401` envuelto | `200` y **borra** la cookie |
| `GET` | `/api/v1/auth/session` | Protegida | `401` envuelto | `200` con `sub` y `email` |

La lista blanca es **por construcción**: una ruta nueva queda del lado protegido sin que nadie tenga que acordarse. `/api/v1/events` (el SSE) quedará detrás del *gate* el día que su change lo declare; hoy no está declarada y contesta `404` con y sin cookie —medido—, porque una ruta no declarada no pasa por el *gate*.

Detalles medidos de las respuestas:

- **`login`**: `302` con una `Location` que lleva **seis** parámetros —`response_type`, `client_id`, `redirect_uri`, `scope`, `state`, `nonce`—; el `state` y el `nonce` son nuevos en cada login. El login **no** establece sesión. Si no alcanza al proveedor: `502` (no `500`), con el mensaje genérico en el cuerpo y la causa en el log.
- **`callback`**: consume el `state` **una sola vez** y comprueba su caducidad; un `state` desconocido, caducado o reutilizado se rechaza sin cookie. La `Location` de la vuelta **no** lleva el código de autorización. Un error del proveedor no llega a canjear nada.
- **`logout`**: `200` con `sesión cerrada` y la cookie retirada con `make_removal`, no con un borrado a mano.
- **`session`**: `200` con `sub` y `email` —medido: `{"status":200,"message":"sesión en vigor","data":{"email":"lorenzo@atareao.es","sub":"operador-del-podcast"}}`—.
- **En modo `Disabled`**, `/api/v1/auth/*` **no se declara**: contesta `404` envuelto en vez de `401`, porque sin proveedor no hay flujo que ofrecer.
- **Método no declarado sobre una ruta protegida**: `401` **sin** sesión y `405` **con** ella. No es casualidad, ver las decisiones.

## Decisiones tomadas durante la implementación

Las que se decidieron al construir, y los porqués que conviene tener delante:

1. **Sesión sin estado en el cliente.** El JWT vive hasta su `exp`; no hay tabla de sesiones. Se acepta que rotar `KESTO_SECRET` invalide todas las sesiones.
2. **El flujo OIDC, escrito a mano con `reqwest` y `jsonwebtoken`.** Se descartaron `tower-sessions` y `openidconnect` porque **la caché local de cargo es el límite duro del repositorio** (compila sin red) y ninguno de los dos estaba en ella. Coste: más código propio que auditar.
3. **`401` sin sesión, `405` sólo con ella.** En la mitad protegida, `method_not_allowed_fallback` se declara **antes** de colgar la capa con `route_layer`. Medido: el orden inverso hace que el `405` **se salte la puerta entera** y revele sin sesión qué métodos existen. Es el detalle más frágil de la feature —invertir las dos líneas no rompe la compilación ni hace caer ninguna prueba de ruta válida— y lo candadean **dos** tests, con `54 passed; 2 failed` medido al invertirlas.
4. **Ruta no declarada → `404` sin puerta**, comportamiento deliberado de `Router::route_layer`, que no envuelve el *fallback* que las resuelve. Envolverlo a mano acoplaría las dos mitades y rompería la partición; un `404` no revela ningún recurso ni ningún método.
5. **`502` cuando el login no alcanza al proveedor**, no `500`: el que falla está fuera. El cuerpo lleva el mismo mensaje genérico que todo `5xx`.
6. **El rechazo del extractor de la consulta del callback se traduce a mano.** Sin eso, `?code=a&code=b` contestaba `400` en `text/plain` con la estructura interna del struct en el cuerpo: rompía la envoltura única. Fue un defecto real que encontró la auditoría del último módulo.
7. **`jsonwebtoken` 11.1.0 con `default-features = false`, `rust_crypto` y `use_pem`.** La justificación inicial —que el backend `aws_lc_rs` fuera el defecto— era **errónea** y se corrigió: en la línea 11.x el backend se elige por feature y `aws_lc_rs` es opcional.
8. **Arnés de pruebas con `wiremock` y un par RSA real**, firma RS256 real, nunca un doble de trait: un doble habría dejado sin probar justo el requisito central, la verificación del ID token.
9. **Sin reloj inyectable**: `AppState` no se deforma para los tests. El TTL de los `state` pendientes se prueba construyendo la entrada con una marca de tiempo ya caducada.
10. **Versiones fijadas por la caché** (el repositorio compila sin conexión): `jsonwebtoken` 11.1.0, `reqwest` 0.12.28 —`rustls-tls` dejó de existir en 0.13—, `rand` 0.8.8 —`jsonwebtoken` exige `^0.8.5`—, `wiremock` 0.6.5, `axum-extra` 0.10.3 con la feature `cookie`.
11. **El `RED` de Rust se monta con firmas que compilan** (`todo!()`), porque un test que no compila se lleva por delante la suite entera, incluidos los tests heredados.

## El margen de reloj del `iat`

Va en su propio apartado, porque es la única cosa de la feature que nació de un hallazgo contra un proveedor **real** y no de una prueba.

`verificar_id_token` comparaba el `iat` del ID token con el reloj de Kesto **sin margen**, y la justificación escrita en el código decía que «no hay dos relojes en juego». Para el ID token esa frase es falsa: el `iat` y el `exp` los firma el reloj del proveedor y los valida el de Kesto, y el `iat` va redondeado al segundo, así que un desfase de un segundo tumbaba un login por lo demás en regla con un `401` genérico. Y ese es justo el caso que **ninguna prueba podía pillar**: el proveedor falso firma con el reloj local. Aparece sólo contra un proveedor de verdad.

El arreglo: la constante `MARGEN_DE_RELOJ` = **60 s**, aplicada **sólo** a la comparación del `iat`, con `saturating_add` para que no pueda desbordar. Sólo tolera el futuro: **nunca** estira la vigencia de nada.

**El `exp` se queda sin margen, y es deliberado**: el `leeway` de `Validation` sigue en `0`. El `exp` de un ID token va una hora visto —un desfase de segundos no lo alcanza— y tolerarlo alargaría la vida de un token recién caducado. El candado de esa decisión ya existía y sigue verde sin tocarlo: `un_token_caducado_no_se_acepta`, el escenario 1.16.

El candado nuevo, `un_iat_dentro_del_margen_de_reloj_no_tumba_el_login`, fija el otro filo: un `iat` adelantado al reloj dentro del margen completa el flujo con `302` y cookie. El desfase se deriva de la constante —su mitad— para que el candado siga valiendo si el margen cambia, con un **suelo de cinco segundos**. Ese suelo es lo que hace que muerda: sin él la mitad del margen se anula con el margen mismo, el test pasaría por vacío y poner el margen a cero daría **verde**; y con un suelo de un segundo tampoco bastaba —la ventana del arnés cruza la frontera del segundo y **de 60 repeticiones una salía verde**—, mientras que con cinco el control negativo cae siempre.

Se investigó si `jsonwebtoken` valida `nbf` con el mismo `leeway` cero —lo que sería un desajuste real, porque muchos proveedores emiten `nbf` ligeramente al alza— y **se midió que no**: `validate_nbf` viene en `false` por defecto (`validation.rs:130`) y `nbf` sólo se lee cuando está activado (`:274`, `:289`). No es deuda; queda el aviso escrito en el doc-comment de la constante, por si algún día se activa.

## Trampas de `jsonwebtoken` 11.1.0 que costaron hallazgos

Las cinco están candadeadas por tests:

1. `Validation::new` **sólo exige `exp`** por defecto: `iss` y `aud` **no** son obligatorios salvo que se añadan con `set_required_spec_claims`.
2. La `leeway` por defecto es de **60 s**: por eso el `exp` se valida con `leeway = 0` a propósito.
3. `iat` **nunca** lo valida la biblioteca: hay que comprobarlo a mano.
4. `Jwk::from_encoding_key` espera DER **PKCS#1**, no PKCS#8.
5. El `kid` se lee con `decode_header`.

Corolario: la comprobación de presencia de `exp` corre contra el **mapa crudo de claims**, no contra la struct `IdTokenClaims`; por eso `exp` **no** se declara en esa struct, deliberadamente.

## Cobertura de la especificación

12 requisitos y 50 escenarios (`auth` 10/42, `api-shell` 2/8), validados en modo estricto. Los tests que los candadean, 92.

`api-shell`: «El router se parte en público y protegido» y «Una petición sin sesión válida contesta con la misma envoltura».

`auth`: «El modo de autenticación decide si hay capa», «El arranque no depende de que el proveedor de identidad esté vivo», «El login redirige al proveedor y no emite sesión», «El callback consume el `state` una sola vez y comprueba su caducidad», «El ID token se verifica antes de emitir sesión», «La sesión se emite como cookie y no como cabecera», «La sesión se verifica contra `KESTO_SECRET` en cada petición protegida», «El cierre de sesión borra la cookie», «`GET /api/v1/auth/session` contesta quién es el usuario» y «Ninguna credencial del proveedor acaba en el log ni en la cookie».

**La especificación se escribió antes que el código** (protocolo SDD) y el test de cada escenario se escribió antes que su implementación (TDD), con `todo!()` como señal de rojo.

## Auditoría y qué se hizo con cada hallazgo

Cada uno de los cuatro módulos pasó por un revisor independiente en sólo lectura, y sus hallazgos se aplicaron **antes** de empezar el siguiente.

| Módulo | Qué encontró | Qué se hizo |
|---|---|---|
| M1 — sesión y errores | Faltaba candado para el `leeway` cero | Se añadió el test |
| M2 — puerta y partición | Una regresión del puerto en el test de apagado ordenado, y que son **dos** candados y no uno los que sostienen el orden del router | Se corrigió el test del puerto y se documentó el segundo candado |
| M3 — descubrimiento y login | Tres `SHALL` de la especificación sin ningún candado, y DNS en los tests | Se candadearon los tres; el DNS se resolvió con un bucle local |
| M4 — callback y verificación | **Un «blocker» que resultó falso positivo** —refutado midiendo: un token **sin `exp` sí** se rechaza— y **un defecto real**: `?code=a&code=b` devolvía `400` en `text/plain` con el detalle del struct, rompiendo la envoltura | Se refutó el falso positivo con la medición y se corrigió el defecto |
| M4b — auditoría de cierre | **Sin defectos.** Dos hallazgos de calidad de la prueba | Se aplicaron en el commit siguiente |
| El margen de reloj | Veredicto: **apto para commitear y cerrar**, cero bloqueantes. Tres hallazgos menores, uno **medido** | Los tres corregidos |

Lo que cambiaron esos hallazgos de calidad de prueba, que conviene contar porque es lo que separa un test verde de un test que prueba algo:

- El escenario 1.38 —el que fija que con `RUST_LOG=trace` el flujo entero no deja ninguna credencial en el log— **podía pasar por vacío**: su marca de control se emitía desde el hilo del test, y `set_default` es **de hilo**, así que un despacho en otro hilo habría dejado las cuatro búsquedas en verde sin probar nada. Ahora afirma dos líneas que emite el propio flujo y ejercita también el camino de error del canje, con un cuerpo hostil que lleva credenciales de verdad.
- Los dos candados de vigencia del ID token sólo afirmaban el `401`, sin fijar **el motivo**: un token rechazado por firma, `kid`, `iss` o `aud` daba el mismo resultado, así que no quedaba probado que el rechazo viniera de la comprobación que anuncian. Ahora afirman la causa registrada. Y cuatro ataques clásicos pasaron a estar candadeados —`alg: none`, un ID token firmado en HS256 usando la clave pública del JWKS como secreto HMAC (confusión de algoritmo), uno sin `kid` y otro con un `kid` que el JWKS no publica—, cada uno con la demostración de que cae al desactivar su comprobación.
- El control negativo del margen de reloj **no era determinista**: con un suelo de un segundo, de 60 repeticiones una salía verde. El suelo subió a cinco, que está holgadamente por encima de la ventana medida del arnés (decenas de milisegundos).

## Verificación

Puertas del repositorio, todas en verde (el CI corre exactamente estos comandos):

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

**92 tests** (91 unitarios + 1 de integración), frente a los 25 de `bootstrap-core`.

Verificación manual (fase 4 del `tasks.md`), diez comprobaciones. Nueve se ejecutaron con `curl` contra el binario real y un proveedor de descubrimiento local, y la décima —el flujo completo contra un proveedor **real**, un `PocketId` 2.18.0 en contenedor— se completó también con un cliente HTTP, para **medir** la cabecera en vez de mirarla a ojo en las herramientas de desarrollo:

| # | Qué se comprobó | Resultado medido |
|---|---|---|
| 4.1 | Modo desarrollo sin ninguna `OIDC_*` | `WARN` en el arranque y `/api/v1/auth/session` → `404` envuelto, no `401`; salud con `auth_mode: "disabled"` |
| 4.2 | Modo `Oidc` con el emisor caído | El proceso **arranca**, avisa del descubrimiento fallido y la salud contesta `200` |
| 4.3 | La salud sin cookie | `200` con la versión y `auth_mode: "oidc"` |
| 4.4 | `/auth/session` sin cookie | `401` con la envoltura |
| 4.5 | `/auth/login` sin seguir el redirector | `302` con la `Location` y sus **seis** parámetros; siguiendo el redirector, la URL del proveedor conserva `state` y `nonce` |
| 4.6 | `logout`: `POST` sin cookie, `GET` sin cookie y `GET` con sesión | `401`, `401` y `405`, los tres envueltos |
| 4.7 | Flujo completo contra el proveedor real (`PocketId` 2.18.0) | **Completado**: el `POST /api/oidc/token` del proveedor contesta `200`, el callback de Kesto responde `302` con `Set-Cookie: kesto_session=…` —`HttpOnly`, `SameSite=Strict`, `Path=/`, `Max-Age=28800`, **sin** `Secure`—, y `GET /api/v1/auth/session` contesta `200` con el `sub` y el `email` de la cuenta |
| 4.8 | Cookie manipulada | `401` envuelto: también con la firma hecha con otra clave y con una cookie caducada |
| 4.9 | `RUST_LOG=trace` en todo el recorrido | **0** apariciones del `client_secret`, **0** del token de acceso, **0** del ID token y **0** de la cookie |
| 4.10 | Configuración incompleta | `KESTO_SECRET` ausente en modo `Oidc` → no arranca y el mensaje la nombra; las `OIDC_*` a medias → error de arranque nombrando la que falta |

Todo lo anterior se reproduce con el arnés guardado fuera del repositorio —un proveedor de descubrimiento local que sirve `/.well-known/openid-configuration`—, salvo la 4.7, que necesita un proveedor de verdad: se ejecutó contra un `PocketId` 2.18.0 en contenedor, en local, obteniendo la sesión del proveedor con su token de un solo uso para no depender de una passkey de hardware.

## Deuda pendiente y límites aceptados

- `session_cookie` devuelve `Result` porque `Cookie::max_age` quiere un `time::Duration` que no se puede nombrar sin añadir `cookie`/`time` como dependencia directa; la conversión va por inferencia con `try_into`.
- El mapa de logins pendientes se purga por TTL pero **no tiene tope duro**.
- Escenarios de robustez que siguen **sin candado**: el fallo del JWKS, el fallo del `token_endpoint` por causas distintas del código de estado, un documento de descubrimiento incompleto y el límite exacto del TTL de los `state` ya consumidos. (La confusión de algoritmo y la ausencia de `kid` **sí** quedaron candadeadas.)
- El `iat` con margen y el `exp` sin él es una **asimetría deliberada**, no un olvido: ver su apartado.
- `nbf` **no** es deuda, y está medido: `validate_nbf` viene en `false` por defecto y `nbf` no se lee.

Los cabos sueltos del SRS que **no** toca esta feature, y a qué change esperan. Son los mismos que listó `FEATURE-001`, y siguen abiertos:

| Lo que el SRS deja abierto | Qué change lo cierra |
|---|---|
| Los secretos son `TEXT` plano en el §3 | `db-schema` |
| `db_name` es inaplicable con los comandos del §4.A (`--all-databases` / `pg_dumpall`) | `db-schema` |
| Las contraseñas viajan por la línea de comandos y son visibles en el listado de procesos del contenedor | `db-schema` |
| `job_executions` no tiene índice por repositorio | `db-schema` |
| El `compose` del §8 monta el socket `:ro`, aunque el motor necesita `exec` | el change del motor |
| No hay `Dockerfile` | el change que lo escriba |

## Lo que NO trae esta feature

Refresh tokens; revocación de sesiones —el JWT vive hasta su `exp`, no hay lista de revocados—; roles y autorización por recurso —hay autenticación, no autorización: cualquier sesión válida vale para todo—; un `/api/v1/auth/me` con datos del proveedor (el `session` contesta lo que hay en el token verificado); logout del lado del proveedor —el cierre de sesión es **sólo local**: se borra la cookie, la sesión del proveedor sigue viva—; y cualquier forma de autenticación básica o por API key.

Y el panel: **`ui/` sigue sin existir**. Tras el login, la vuelta a `/` contesta `404` envuelto, y eso es lo esperado hoy: no hay panel que servir. La puerta está puesta antes de que haya puerta que cruzar.

Este documento es el hermano de `plans/PLAN-002-COMPLETED.md` y continúa a `docs/FEATURE-001-bootstrap-core.md`.
