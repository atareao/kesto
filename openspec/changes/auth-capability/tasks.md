# Tasks: auth-capability

## 0. Prerrequisito: las dependencias

No hay ciclo RED posible sin compilador, y el repositorio compila **sin red**: cada dependencia nueva tiene que estar ya en la caché local de Cargo. Este paso no es RED ni GREEN, es la condición para que existan.

- [ ] 0.1 Resolver, contra la caché local de `~/.cargo/registry`, las versiones exactas de `jsonwebtoken` —con `default-features = false` y las features `rust_crypto` y `use_pem`, que en la 11.x cacheada existen las dos: `rust_crypto` sustituye al backend criptográfico en C que trae por defecto, y `use_pem` hace falta para leer las claves del JWKS—, `reqwest` sin features por defecto y con `rustls-tls` + `json`, `axum-extra` con `cookie`, `rand` y `urlencoding`, y escribirlas en `Cargo.toml`. **No fijar versiones de memoria ni con `=`**: las que estén cacheadas, y el rango *caret* deja que Cargo resuelva.
- [ ] 0.2 `Cargo.toml`: en `[dev-dependencies]`, `wiremock` para el proveedor falso. `tower` con `util` e `http-body-util` ya están y no se duplican.
- [ ] 0.3 Dejar constancia escrita, en la sección de deudas de `docs/FEATURE-002-auth-capability.md`, de por qué **no** se usan `openidconnect` ni `tower-sessions`: no están en la caché local y una dependencia que obligue a red rompe la premisa de que el repositorio compila sin conexión.
- [ ] 0.4 `cargo check` limpio con las dependencias puestas, y `cargo tree` revisado para no duplicar ninguna versión mayor de una dependencia **directa** —si aparece duplicación transitiva de crates procedurales, se acepta y se deja constancia, como se hizo en `bootstrap-core`—.
- [ ] 0.5 Dejar escrito el mecanismo con el que se prueban las piezas de red, que **ya no es una decisión abierta**: **`wiremock`** —ya en la caché local, `0.6.5`, junto con `rsa` `0.9.10` y `sha2`— sirviendo el proveedor falso local de `authorization_endpoint`, `token_endpoint` y `jwks_uri`, con un **par RSA de verdad** y el ID token firmado **RS256 de verdad**, publicando la clave pública como JWK en el `jwks_uri`. Los dobles de las funciones de red quedan **descartados**, y el porqué SHALL quedar escrito donde se declare el proveedor falso: dejarían sin probar precisamente el requisito central de este change —que el ID token se verifica—, que es uno de los dos defectos de shuul que aquí se corrigen. **Ninguna prueba SHALL salir a internet.**

## 1. RED (pruebas que fallan primero)

Cada tarea es una prueba por escenario de `specs/auth/spec.md` y `specs/api-shell/spec.md` que sea asertable sin red. Los escenarios que hablan con el proveedor se prueban contra el proveedor falso local de la tarea 0.5 —`wiremock` con un par RSA real y el ID token firmado `RS256`—: **ninguna prueba sale a internet**.

- [ ] 1.1 `src/api/mod.rs`: test que fija que en modo `Disabled` el flujo no está montado y `/api/v1/auth/session` contesta `404` con la envoltura, no `401`.
- [ ] 1.2 `src/api/mod.rs`: test que fija que en modo `Oidc` esa misma ruta sin cookie contesta `401` con la envoltura `{status, message}` y el cuerpo no vacío.
- [ ] 1.3 `src/api/mod.rs`: test que fija que en modo `Oidc` `GET /api/v1/health` sigue contestando `200` sin cookie.
- [ ] 1.4 `src/api/mod.rs`: test que fija que la superficie pública es exactamente `/api/v1/health`, `/api/v1/auth/login` y `/api/v1/auth/callback`, y que `/api/v1/events` no está en ella.
- [ ] 1.5 `src/auth/mod.rs`: test que fija que un descubrimiento fallido no impide arrancar: el router se construye y el aviso declara el fallo.
- [ ] 1.6 `src/auth/mod.rs`: test que fija que un login con el descubrimiento sin cachear lo reintenta y cachea el resultado, de modo que el segundo login no vuelve a pedirlo.
- [ ] 1.7 `src/auth/oidc.rs`: test que fija que `GET /api/v1/auth/login` contesta `302` y su `Location` trae `response_type=code`, `client_id`, `redirect_uri`, `scope`, `state` y `nonce`.
- [ ] 1.8 `src/auth/oidc.rs`: test que fija que dos logins seguidos usan `state` y `nonce` distintos, de 32 caracteres cada uno.
- [ ] 1.9 `src/auth/oidc.rs`: test que fija que el login no emite `Set-Cookie` de sesión.
- [ ] 1.10 `src/api/auth.rs`: test que fija que un login con el proveedor caído contesta **`502`** con la envoltura, que su mensaje no publica el `client_secret` y que el mensaje es **genérico** —el mismo de un `5xx`: la causa va al log y no al cuerpo—.
- [ ] 1.11 `src/auth/oidc.rs`: test que fija que un `state` desconocido se rechaza sin emitir cookie.
- [ ] 1.12 `src/auth/oidc.rs`: test que fija que un `state` emitido hace más de 5 minutos se rechaza sin emitir cookie. **Es el escenario que corrige el defecto de shuul**, que guarda la marca de tiempo y no la comprueba. **Sin reloj inyectable**: `AppState` NO se deforma para los tests, así que la entrada del mapa se construye con su marca de tiempo **ya retrasada** —determinista—, que prueba la misma lógica sin dormir y sin tocar el estado de producción.
- [ ] 1.13 `src/auth/oidc.rs`: test que fija que un `state` ya consumido se rechaza sin emitir cookie.
- [ ] 1.14 `src/auth/oidc.rs`: test que fija que un callback con `error=access_denied` no llega a llamar al `token_endpoint`.
- [ ] 1.15 `src/auth/oidc.rs`: test que fija que un ID token con `aud` distinto del `client_id` no se acepta y no produce cookie.
- [ ] 1.16 `src/auth/oidc.rs`: test que fija que un ID token con `exp` en el pasado no se acepta y no produce cookie.
- [ ] 1.17 `src/auth/oidc.rs`: test que fija que un ID token cuyo `nonce` no es el emitido no se acepta y no produce cookie.
- [ ] 1.18 `src/auth/oidc.rs`: test que fija que un ID token firmado con una clave que el `jwks_uri` no publica no se acepta.
- [ ] 1.19 `src/auth/oidc.rs`: test que fija que un ID token con `iss` distinto del emisor configurado no se acepta.
- [ ] 1.20 `src/auth/oidc.rs`: test que fija que un ID token válido convierte la sesión en una cookie cuyo `sub` y `email` son los del token.
- [ ] 1.21 `src/auth/oidc.rs`: test que fija que el flujo se completa con un documento de descubrimiento **sin** `userinfo_endpoint`, y que no se pide ningún `userinfo`.
- [ ] 1.22 `src/auth/session.rs`: test que fija los atributos de la cookie: `HttpOnly`, `SameSite=Strict`, `Path=/` y `Max-Age`.
- [ ] 1.23 `src/auth/session.rs`: test que fija que una URL de retorno `http` no añade `Secure` y una `https` sí.
- [ ] 1.24 `src/auth/session.rs`: test que fija que los claims del JWT de sesión son exactamente `sub`, `email`, `iat` y `exp`, y que no hay ningún claim de rol.
- [ ] 1.25 `src/auth/session.rs`: test que fija que el `Max-Age` de la cookie, el `exp` del JWT y la constante de vida de la sesión coinciden, y que la constante son 8 horas.
- [ ] 1.26 `src/api/auth.rs`: test que fija que el callback correcto contesta `302` —la vuelta al panel—.
- [ ] 1.27 `src/auth/middleware.rs`: test que fija que una petición protegida sin cookie contesta `401` con la envoltura.
- [ ] 1.28 `src/auth/session.rs`: test que fija que una cookie firmada con una clave distinta de `KESTO_SECRET` no se acepta.
- [ ] 1.29 `src/auth/session.rs`: test que fija que una cookie con `exp` en el pasado no se acepta.
- [ ] 1.30 `src/auth/session.rs`: test que fija que una cookie manipulada sin volver a firmarla no se acepta.
- [ ] 1.31 `src/auth/middleware.rs`: test que fija que una cookie válida deja pasar y el manejador ve la identidad de la sesión.
- [ ] 1.32 `src/api/mod.rs`: test que fija que el `401` no es `3xx`, no trae `Location` y su cuerpo no está vacío.
- [ ] 1.33 `src/api/auth.rs`: test que fija que `POST /api/v1/auth/logout` con sesión contesta `200` con la envoltura y una `Set-Cookie` que borra la cookie.
- [ ] 1.34 `src/api/auth.rs`: test que fija que `POST /api/v1/auth/logout` sin cookie contesta `401` con la envoltura.
- [ ] 1.35 `src/api/auth.rs`: test que fija que `GET /api/v1/auth/logout` **con una cookie de sesión válida** contesta `405` con la envoltura.
- [ ] 1.36 `src/api/auth.rs`: test que fija que `GET /api/v1/auth/session` con sesión contesta `200` con el `sub` y el `email`.
- [ ] 1.37 `src/api/auth.rs`: test que fija que `GET /api/v1/auth/session` sin cookie contesta `401` con la envoltura.
- [ ] 1.38 `src/auth/mod.rs`: test que fija que con `RUST_LOG=trace` el flujo completo —login, callback y una petición con la sesión— no deja en el log ni el `client_secret`, ni el código de autorización, ni el ID token, ni el token de acceso, usando valores distinguibles que se buscan literalmente en la salida capturada.
- [ ] 1.39 `src/auth/session.rs`: test que fija que el JWT de la cookie no lleva el ID token ni el token de acceso del proveedor.
- [ ] 1.40 `src/api/auth.rs`: test que fija que la `Location` de la vuelta al panel no contiene el código de autorización.
- [ ] 1.41 `src/api/mod.rs`: test que fija que las rutas públicas no pasan por el *gate* —la salud no contesta `401`— y que la superficie de negocio sí.
- [ ] 1.42 `src/api/mod.rs`: test que fija que un **método no declarado** sobre una ruta protegida —`POST /api/v1/auth/session`, que solo declara `GET`—, **sin** cookie, contesta `401` con la envoltura y **no** `405`: el *gate* precede al enrutado por método.
- [ ] 1.43 `src/api/mod.rs`: test que fija que ese mismo método no declarado —`POST /api/v1/auth/session`—, **con** una cookie de sesión válida, contesta `405` con la envoltura.
- [ ] 1.44 `src/api/mod.rs`: test que fija que una **ruta no declarada** bajo `/api/v1` contesta `404` con la envoltura **sin** exigir sesión: el `404` no pasa por el *gate*, y eso es correcto.
- [ ] 1.45 `src/api/auth.rs`: test que fija que `GET /api/v1/auth/logout` **sin** cookie contesta `401` con la envoltura y **no** `405`.
- [ ] 1.46 Ejecutar `just check`: las pruebas nuevas fallan por no existir el código, y la suite de `bootstrap-core` sigue verde.

## 2. GREEN (implementación mínima)

- [ ] 2.1 `Cargo.toml` / `Cargo.lock`: las dependencias de la tarea 0.1 y el `wiremock` de la 0.2.
- [ ] 2.2 `src/error.rs`: **dos** variantes —`Unauthorized`, que traduce a `401` con la envoltura `{status, message}`, y `BadGateway`, que traduce a `502` con la misma envoltura y con el **mensaje genérico** de la variante de fallo interno, con la causa al log y nunca al cuerpo—, ambas sin `data` y sin redirección, cada una con su test por la vía de `IntoResponse`.
- [ ] 2.3 `src/state.rs`: el estado de autenticación —el descubrimiento cacheado tras un candado de lectura/escritura y el mapa de `state`/`nonce` en memoria con su marca de tiempo—, sólo presente cuando el modo es `Oidc`.
- [ ] 2.4 `src/auth/mod.rs`: el descubrimiento del proveedor, con su caché, su reintento y el aviso que no impide arrancar.
- [ ] 2.5 `src/auth/oidc.rs`: la construcción de la URL de autorización, la generación de `state` y `nonce`, el canje del código en el `token_endpoint` y la verificación del ID token contra el `jwks_uri`.
- [ ] 2.6 `src/auth/session.rs`: la emisión del JWT de sesión con `HS256` y `KESTO_SECRET`, su verificación, y los atributos de la cookie con el `Secure` condicional al esquema del retorno.
- [ ] 2.7 `src/auth/middleware.rs`: el *gate* de la superficie protegida, que extrae la cookie, verifica firma y vigencia y contesta `401` con la envoltura.
- [ ] 2.8 `src/api/auth.rs`: los cuatro endpoints del flujo, con `login` y `callback` públicos y `logout` y `session` protegidos.
- [ ] 2.9 `src/api/mod.rs`: la partición del router —lista blanca pública, mitad protegida con el *gate* por `route_layer`— y la no-montura del flujo en modo `Disabled`. El `method_not_allowed_fallback` global SHALL declararse **antes** de colgar la capa, para que la capa lo envuelva y el *gate* decida antes que el enrutado por método: **el orden de esas dos líneas es semántico, no cosmético**.
- [ ] 2.10 `src/main.rs`: el descubrimiento al arrancar, su aviso cuando falla y el router partido.
- [ ] 2.11 El proveedor falso local de la tarea 0.5: `wiremock` sirviendo `authorization_endpoint`, `token_endpoint` y `jwks_uri`, con un **par RSA real** y el ID token firmado **RS256**, sin salir a internet. No hay dobles de las piezas de red.
- [ ] 2.12 `just check` en verde.

## 3. REFACTOR

- [ ] 3.1 `cargo fmt --all` y `cargo clippy --all-targets -- -D warnings` limpios.
- [ ] 3.2 Ninguna credencial se interpola en ninguna llamada de log: el `client_secret` sigue siendo un `SecretString` y la redacción la lleva el tipo, no un cuidado manual en cada sitio. Comprobar que el código de autorización, el ID token y el token de acceso tampoco se interpolan.
- [ ] 3.3 Comprobar que ninguna línea de log —ni de nivel `trace`— contiene la cadena de consulta del callback. Cuando llegue la traza de peticiones, su span **no** podrá incluirla; hoy basta con que nadie registre la URI.
- [ ] 3.4 La cookie lleva únicamente el JWT de sesión: ni el ID token, ni el token de acceso, ni claims de rol.
- [ ] 3.5 El mapa de `state`/`nonce` no crece sin límite: las entradas caducadas se purgan al consumir y al insertar.
- [ ] 3.6 Ningún test declara `unsafe` ni muta el entorno del proceso: la configuración, como en `bootstrap-core`, se prueba por la lectura inyectada.
- [ ] 3.7 Sin `unwrap()`, ni `expect()`, ni `panic!()` en código de producción; los errores son variantes tipadas.
- [ ] 3.8 El layout sigue el §7: `src/api/auth.rs` es HTTP y `src/auth/` es el flujo, la sesión y el *gate*. `src/auth/` **no** es un módulo del SRS y se declara en `main.rs` como los demás.
- [ ] 3.9 `just doc` sin enlaces rotos.

## 4. Verificación manual (operador)

- [ ] 4.1 Arrancar sin ninguna variable `OIDC_*` y comprobar en el log el aviso de modo desarrollo y que `/api/v1/auth/session` contesta `404` con la envoltura —no `401`—: sin proveedor no hay flujo montado.
- [ ] 4.2 Arrancar en modo `Oidc` con el emisor de un proveedor que no contesta: el proceso **arranca**, el aviso declara el descubrimiento fallido y la salud contesta `200`.
- [ ] 4.3 `curl -i localhost:8080/api/v1/health` sin cookie: `200` con la versión y el `auth_mode` `oidc`.
- [ ] 4.4 `curl -i localhost:8080/api/v1/auth/session` sin cookie: `401` con cuerpo JSON `{status, message}`, ni vacío ni redirigido.
- [ ] 4.5 `curl -i 'localhost:8080/api/v1/auth/login'` sin seguir el redirector: `302` con la cabecera `Location` y los seis parámetros. `curl -iL` hasta el proveedor: la URL del proveedor conserva `state` y `nonce`.
- [ ] 4.6 `curl -i -X POST localhost:8080/api/v1/auth/logout` sin cookie: `401` con la envoltura. Y `curl -i localhost:8080/api/v1/auth/logout` con `GET`: `405` con la envoltura.
- [ ] 4.7 Flujo completo en el navegador contra el proveedor real (`PocketId`): entrar, y en las herramientas de desarrollo comprobar que la cookie es `HttpOnly`, `SameSite=Strict`, `Path=/`, con `Max-Age` de 8 horas y **sin** `Secure`, porque en desarrollo el retorno es `http`. Después, `GET /api/v1/auth/session` contesta el `sub` y el `email`.
- [ ] 4.8 Con la sesión abierta, editar a mano la cookie en el navegador y comprobar que la siguiente petición protegida contesta `401` con la envoltura.
- [ ] 4.9 Arrancar con `RUST_LOG=trace`, completar el flujo y pasar el log por un `grep` de las credenciales del proveedor —el `client_secret`, el código, el ID token y el token de acceso—: ninguna aparece.
- [ ] 4.10 Comprobar que un `.env` sin `KESTO_SECRET` en modo `Oidc` no arranca y el mensaje nombra `KESTO_SECRET`, como ya fijaba `bootstrap-core`.

## 5. Cierre

- [ ] 5.1 `just spec-validate auth-capability` en modo estricto.
- [ ] 5.2 `AGENTS.md`: la tabla de variables gana la obligatoriedad de `KESTO_SECRET` en modo OIDC, y el modo de desarrollo deja de describirse como «panel sin autenticar» para decir qué queda público y qué no.
- [ ] 5.3 `README.md`: el estado deja de decir que no hay autenticación.
- [ ] 5.4 `docs/FEATURE-002-auth-capability.md` con lo que realmente quedó construido: contratos, endpoints, las versiones resueltas en la tarea 0.1, el mecanismo de la 0.5 y la deuda pendiente.
- [ ] 5.5 Redactar el `Purpose` de `openspec/specs/auth/spec.md` al archivar: es una capability **nueva** y no puede quedarse en `TBD`.
- [ ] 5.6 `just plan-done` y `just spec-archive auth-capability`.
