# Tasks — endurecimiento-superficie-api

> Issues **#163 (SEC-07)**, **#165 (SEC-09)** y **#166 (SEC-10)**, hijos de la auditoría **#156** (bloque D).
> Change de **producto**: configuración, cookie de sesión, límite de tasa del login, borde HTTP de repositorios y arnés.
> Decisiones y alternativas descartadas, en `design.md`. TDD: primero los tests que hoy fallan (rojo),
> luego el mínimo (verde), luego el refactor. Sin dependencias nuevas (la crate `url` ya está en el árbol).
> Plan: `plans/PLAN-043.md`. Documento de la feature: `docs/FEATURE-043-endurecimiento-superficie-api.md`.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Rama desde `development`: `just gf-feature endurecimiento-superficie-api` (verificación: `git branch --show-current`).
- [x] 0.3 Change aprobado por el usuario (verificación: aprobación explícita y `just check-spec` encuentra el change activo).

## 1. La cookie siempre con `Secure` (SEC-09, #165)

### RED

- [x] 1.1 Rust: test —cookie emitida en modo `Oidc` con retorno `https`— exige el atributo `Secure` (verificación: verde hoy; se conserva como contrapeso).
- [x] 1.2 Rust: test —configuración en modo `Oidc` con `OIDC_REDIRECT_URL` por `http`— exige error de arranque que **nombre la variable** (verificación: rojo hoy, que arranca).
- [x] 1.3 Rust: test —configuración en modo `Oidc` **sin** `OIDC_REDIRECT_URL`— exige error que nombre la variable, sin caer a ningún defecto (verificación: rojo hoy, que toma el defecto `http`).
- [x] 1.4 Rust: test —configuración sin las tres `OIDC_*` y con `OIDC_REDIRECT_URL` declarado por `http`— exige modo `Disabled` y ningún error (verificación: verde hoy; se conserva como contrapeso).
- [x] 1.5 Rust: test —`session_cookie` sin parámetro de retorno— exige `Secure` en el renderizado de la cookie (verificación: rojo hoy, que exige el parámetro).
- [x] 1.6 Rust: test de integración (`tests/arranque.rs`) —modo `Oidc` con retorno por `http`— exige salida de error y código distinto de cero (verificación: rojo hoy).

### GREEN

- [x] 1.7 `src/auth/session.rs`: `session_cookie` pierde el parámetro `redirect_url`, emite `.secure(true)` incondicional y desaparece `requires_secure` (verificación: 1.1, 1.5 en verde).
- [x] 1.8 `src/config.rs`: `resolve_auth` exige `OIDC_REDIRECT_URL` presente y con esquema `https` en modo `Oidc`, con variante de error nueva que nombra la variable; se elimina `DEFAULT_OIDC_REDIRECT_URL` y su uso (verificación: 1.2–1.4, 1.6 en verde).
- [x] 1.9 `src/api/auth.rs`: la llamada a `session_cookie` pierde el argumento del retorno (verificación: `cargo test --locked --bin kesto 'api::auth::'` verde).

### REFACTOR

- [x] 1.10 Rustdoc del porqué del `Secure` incondicional y del coste de compatibilidad del retorno obligatorio; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 2. El mapa de logins tiene tope (SEC-10, #166)

### RED

- [x] 2.1 Rust: test —insertar `MAX_LOGINS` entradas y una más— exige que el mapa no pase del tope (verificación: rojo hoy, que crece sin límite).
- [x] 2.2 Rust: test —con el mapa lleno, insertar una nueva— exige que la desalojada sea la de menor `issued_at` (verificación: rojo hoy).
- [x] 2.3 Rust: test —con entradas caducadas y el mapa lleno— exige que la purga por caducidad ocurra **antes** del desalojo y que no se desaloje ninguna vigente si queda hueco (verificación: rojo hoy).
- [x] 2.4 Rust: test —consumir un `state` vigente con el mapa lleno— exige que siga funcionando (verificación: verde hoy; se conserva como contrapeso).

### GREEN

- [x] 2.5 `src/state.rs`: constante `MAX_LOGINS`, purga por caducidad seguida del desalojo de la más antigua en `registrar_login`, y el comentario `AVISO` de `:124-128` actualizado a lo que ahora sí se implementa (verificación: 2.1–2.4 en verde).

### REFACTOR

- [x] 2.6 Rustdoc del porqué del desalojo frente al rechazo; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 3. El login tiene límite de tasa por IP (SEC-10, #166)

### RED

- [x] 3.1 Rust: test del módulo puro —`MAX_PETICIONES_DE_LOGIN` peticiones de la misma IP dentro de la ventana se permiten y la siguiente no— (verificación: rojo hoy, que no existe el módulo).
- [x] 3.2 Rust: test del módulo puro —pasada la ventana, la misma IP recupera su cupo— con reloj inyectado y sin dormir (verificación: rojo hoy).
- [x] 3.3 Rust: test del módulo puro —una IP distinta no consume el cupo de la primera— (verificación: rojo hoy).
- [x] 3.4 Rust: test del módulo puro —el mapa de ventanas se poda y no crece sin límite— (verificación: rojo hoy).
- [x] 3.5 Rust: test de integración —`GET /api/v1/auth/login` por encima del cupo contesta `429` con la envoltura y **no** inserta en el mapa de logins— (verificación: rojo hoy, que contesta `302`).
- [x] 3.6 Rust: test —una petición dentro del cupo sigue recibiendo su `302`— (verificación: verde hoy; se conserva como contrapeso).
- [x] 3.7 Rust: test —la respuesta `429` no publica ninguna credencial ni el valor de ninguna cabecera de la petición— (verificación: rojo hoy).

### GREEN

- [x] 3.8 `src/auth/limite.rs`: módulo nuevo con el limitador de ventana fija, con el reloj y la IP inyectados (verificación: 3.1–3.4 en verde).
- [x] 3.9 `src/error.rs`: variante `TooManyRequests` y su brazo en `into_response` → `429` con la envoltura (verificación: 3.5, 3.7 en verde).
- [x] 3.10 `src/state.rs`: el limitador en `AppState`, con su cupo y su tope (verificación: el estado se construye en las pruebas).
- [x] 3.11 `src/api/mod.rs`: capa `from_fn_with_state` sobre `GET /api/v1/auth/login` que lee la IP de las extensiones (`ConnectInfo`) y cae a la centinela cuando no la hay (verificación: 3.5, 3.6 en verde).
- [x] 3.12 `src/main.rs`: `into_make_service_with_connect_info::<SocketAddr>()` al servir (verificación: `tests/arranque.rs` en verde).

### REFACTOR

- [x] 3.13 Rustdoc del porqué de la ventana fija, de la IP del socket y de la centinela; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 4. El módulo puro del `endpoint` (SEC-07, #163)

### RED

- [x] 4.1 Rust: test —`https` con host público o de rango privado— se acepta (verificación: rojo hoy, que no existe el módulo).
- [x] 4.2 Rust: test —`http`— se rechaza sin permiso y se acepta con permiso (verificación: rojo hoy).
- [x] 4.3 Rust: test —loopback (`127.0.0.1`, `[::1]`) y nombre `localhost`/`algo.localhost`— se rechazan sin permiso y se aceptan con permiso (verificación: rojo hoy).
- [x] 4.4 Rust: test —enlace local `169.254.169.254`, `fe80::1`, metadatos `fd00:ec2::254`, no especificada `0.0.0.0` y `[::]`— se rechazan **también** con el permiso concedido (verificación: rojo hoy).
- [x] 4.5 Rust: test —esquema desconocido, valor que no es una URL absoluta y `endpoint` con credenciales embebidas— se rechazan (verificación: rojo hoy).
- [x] 4.6 Rust: test —el mensaje del error nombra el campo `endpoint` y **no** repite la URL recibida— (verificación: rojo hoy).

### GREEN

- [x] 4.7 `src/rustic/endpoint.rs`: módulo nuevo **puro** con `validar(valor, permitir_inseguro)` y su tipo de error (verificación: 4.1–4.6 en verde).
- [x] 4.8 `Cargo.toml`: `url` como dependencia directa, con la versión ya fijada en `Cargo.lock` (verificación: `cargo check --locked` y el `Cargo.lock` sin cambios de versión).

### REFACTOR

- [x] 4.9 Rustdoc del módulo —qué cierra y qué no— y de la limitación de no resolver el nombre; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 5. El borde valida el `endpoint` (SEC-07, #163)

### RED

- [x] 5.1 Rust: test de alta con `endpoint` por `http` sin permiso exige `400` nombrando `endpoint` y sin fila escrita (verificación: rojo hoy, que contesta `201` o `502`).
- [x] 5.2 Rust: test de alta con `endpoint` a loopback sin permiso exige `400` (verificación: rojo hoy).
- [x] 5.3 Rust: test de alta con `endpoint` por `https` contra un destino inalcanzable exige el `502` de siempre (verificación: verde hoy; se conserva como contrapeso del oráculo que **no** se cierra).
- [x] 5.4 Rust: test de `PUT` con `endpoint` por `http` sin permiso exige `400` y que la fila no cambie (verificación: rojo hoy).
- [x] 5.5 Rust: test —con el permiso declarado, el alta contra un `endpoint` por `http` a loopback sigue adelante— (verificación: verde hoy tras 5.7).
- [x] 5.6 Rust: test de configuración —`KESTO_ALLOW_INSECURE_ENDPOINT` con un valor distinto de `1` y `0`— exige error que nombre la variable (verificación: rojo hoy).

### GREEN

- [x] 5.7 `src/api/repositories.rs`: `alta` y `actualizar` validan el `endpoint` con `endpoint::validar` y el permiso de la configuración, con `400` que nombra el campo (verificación: 5.1–5.5 en verde).
- [x] 5.8 `src/config.rs`: constante `ENV_KESTO_ALLOW_INSECURE_ENDPOINT`, campo en `Config` con el defecto negado y la lectura con el mismo patrón que `KESTO_ALLOW_INSECURE` (verificación: 5.6 en verde).

### REFACTOR

- [x] 5.9 Un solo sitio decide qué es un `endpoint` aceptable —el módulo puro— usado por el alta y la edición; rustdoc del porqué del borde y del oráculo que se deja; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 6. Arnés, e2e y documentación

- [x] 6.1 `src/test_support.rs`: el `endpoint` de las filas de prueba pasa a ser una URL `https://…` cuyo camino es la ruta real, y `abridor_local`/`comprobador_local` la traducen; las lecturas OIDC de prueba declaran `OIDC_REDIRECT_URL` por `https` (verificación: `just check` en verde).
- [x] 6.2 `tests/e2e/arnes.rs`: el arranque declara `KESTO_ALLOW_INSECURE_ENDPOINT=1` (verificación: `just e2e` en verde).
- [x] 6.3 `tests/e2e/escenarios.rs`: escenario nuevo —el almacén real sigue verificándose con el permiso declarado— (verificación: pasa y fallaría sin la variable en el arnés).
- [x] 6.4 `compose.yaml`: `OIDC_REDIRECT_URL` con la forma `${OIDC_REDIRECT_URL:?}` y el comentario de las variables obligatorias actualizado (verificación: `podman compose config` se niega sin la variable).
- [x] 6.5 `.github/workflows/release.yml`: el smoke test declara `OIDC_REDIRECT_URL` por `https` (verificación: el contenedor arranca y `/api/v1/health` contesta `200` con `auth_mode: "oidc"`).
- [x] 6.6 `docs/DEFINITION.md` §8: `OIDC_REDIRECT_URL` obligatoria con OIDC y la variable `KESTO_ALLOW_INSECURE_ENDPOINT` (verificación: la tabla y el `compose` de referencia concuerdan).
- [x] 6.7 `docs/ENTORNO.md` y `.env.example`: las dos variables, con su defecto, su obligación y la nota de actualización (verificación: la tabla las lista y el aviso de compatibilidad está).
- [x] 6.8 `docs/ARQUITECTURA.md`: la cookie `Secure`, el retorno obligatorio y la validación del `endpoint` (verificación: el documento describe las tres reglas).
- [x] 6.9 `docs/FEATURE-043-endurecimiento-superficie-api.md`: documento de la feature entregada (verificación: existe y cubre los cinco apartados).

## 7. Cierre

- [x] 7.1 `just check-all` (backend y panel) en verde (verificación: termina sin error).
- [x] 7.2 Revisión de `@rust-reviewer` sin bloqueantes (verificación: las observaciones aplicadas o contestadas).
- [x] 7.3 PR a `development` con `Closes #163`, `Closes #165` y `Closes #166` (verificación: el PR abre con los tres cierres y el CI en verde).
- [x] 7.4 Fusionado el PR: `just spec-archive endurecimiento-superficie-api` (verificación: el delta entra en `openspec/specs/`).
- [x] 7.5 `plans/PLAN-043.md` → `plans/PLAN-043-COMPLETED.md` (verificación: `git mv` hecho).
- [x] 7.6 Issue de seguimiento para la resolución del nombre del `endpoint` (verificación: el issue existe y nombra el hallazgo).
