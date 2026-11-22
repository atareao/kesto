# PLAN-043 — Endurecimiento de la superficie de la API

- **Issues**: #163 (SEC-07), #165 (SEC-09) y #166 (SEC-10), hijos de la auditoría de seguridad #156 — bloque D de la campaña.
- **Rama**: `feature/endurecimiento-superficie-api` (desde `development`).
- **Change de OpenSpec**: `openspec/changes/endurecimiento-superficie-api/` (aprobado por el usuario antes de escribir código).
- **Documento de la feature**: `docs/FEATURE-043-endurecimiento-superficie-api.md` (al cerrar).
- **Bump esperado**: minor (`0.19.0`). Los commits van `feat:`/`fix:` **sin** `!` ni `BREAKING CHANGE`, aunque el cambio rompa compatibilidad en un punto: el pipeline deduce `major` de esa marca y este proyecto va por `0.x`. La incompatibilidad se describe en el PR, en el CHANGELOG y en `docs/ENTORNO.md`.

## Objetivo

Los tres hallazgos viven en la superficie de red y en el borde HTTP —la cookie de sesión, la ruta pública del login y el destino S3 de un repositorio— y ninguno toca el motor ni la base.

1. **SEC-09 (#165)** — la cookie de sesión solo lleva `Secure` si el esquema de `OIDC_REDIRECT_URL` es `https`, y el valor por defecto es `http`. Un despliegue OIDC por HTTP emite la cookie **sin** `Secure`: la credencial de sesión viaja en claro durante ocho horas.
2. **SEC-10 (#166)** — `GET /api/v1/auth/login` es pública y no tiene límite de tasa; cada petición inserta en el mapa de logins, que no tiene tope y solo se poda por caducidad. Un caudal alto y no autenticado lo hace crecer sin límite: denegación de servicio de memoria.
3. **SEC-07 (#163)** — el `endpoint` de un repositorio llega a `opendal` sin validación de esquema ni de host. Un `http://` firma SigV4 y manda la `secret_access_key` en claro, y la diferencia entre `201`/`400` y `502` convierte el alta en un oráculo de alcanzabilidad contra hosts arbitrarios, incluida la dirección de metadatos de la nube.

## Alcance aprobado por el usuario

- **#165**: `Secure` **siempre** en modo OIDC y abortar sin escape. `session_cookie` pierde el parámetro `redirect_url`; desaparece `requires_secure` y `DEFAULT_OIDC_REDIRECT_URL`. En modo OIDC, `OIDC_REDIRECT_URL` es **obligatoria y `https`** (ausente, o con otro esquema, es error de arranque que nombra la variable). Sin las tres `OIDC_*` se ignora en silencio. **No** hay válvula de escape, ni siquiera la que existe para el modo sin autenticación.
- **#166**: tope duro del mapa de logins (desalojo del más antiguo por `issued_at`, nunca rechazo del login nuevo) **más** límite de tasa por IP real del socket (`ConnectInfo`, sin confiar en cabeceras), con respuesta `429` y la envoltura de error de la casa.
- **#163 (alcance)**: `https` obligatorio, **pero se permite la red privada** (RFC1918 y ULA), porque el caso de uso principal es MinIO/SeaweedFS autoalojado. Se rechazan siempre link-local (`169.254/16`, `fe80::/10`), metadatos IPv6 (`fd00:ec2::254`), no especificadas (`0.0.0.0`, `::`) y credenciales embebidas en la URL; se rechazan salvo permiso loopback, `localhost`/`*.localhost` y el esquema `http`.
- **#163 (http/e2e)**: una sola variable explícita de permiso, `KESTO_ALLOW_INSECURE_ENDPOINT` (`1` concede, `0`/ausente niega, otro valor —vacío incluido— error de arranque), que rebaja a la vez el esquema y el loopback; la batería la declara porque su almacén real es SeaweedFS en `http://127.0.0.1`.
- **#163 (oráculo)**: validar el `endpoint` y dejar el alta como está. **No** se cierra el oráculo `201`/`400` vs `502`.

## Fuera de alcance

- No se cierra el **oráculo de alcanzabilidad** del alta: quien puede usarlo ya tiene sesión de operador y puede sondear la conectividad por la ruta de salud y por el listado de instantáneas. Uniformar la respuesta quitaría el aviso inmediato sin quitar la capacidad.
- No se resuelve el **nombre del `endpoint`**: un host que resuelva por DNS a una dirección de enlace local no se detecta. Cerrarlo exige resolver y **fijar** la dirección usada en la conexión, y `opendal` no ofrece ese punto de enganche → **issue de seguimiento** al cerrar.
- No se añade **proxy de confianza** ni lectura de `X-Forwarded-For`.
- No se pone **TLS en el arnés** ni se sirve SeaweedFS por `https`.
- No se limita la tasa de `/api/v1/auth/callback`: no es un sumidero de memoria —consume una entrada acotada— y un `state` desconocido se rechaza en el acto.
- No se toca el `ON_ERROR_STOP` de la restauración (**#175**), ni la KDF (**#169**), ni la separación de privilegio del motor (**#173**).
- **Ninguna dependencia nueva**: la crate `url` ya está en el árbol por `reqwest` y pasa a declararse directa.

## Forma del delta de OpenSpec

`openspec validate --strict` **no** admite que un bloque `MODIFIED` pierda un escenario, y aquí hay dos que dejan de ser válidos en lugar de cambiar de cuerpo: «`Secure` depende del esquema de retorno» y «La URL de retorno tiene un valor por defecto de desarrollo». Renombrarlos tampoco vale —la comprobación los cuenta por nombre—, así que los dos requisitos afectados van como **`REMOVED` + re-añadidos** con un nombre que declara el cambio, igual que hizo el bloque A con el requisito de la credencial débil. Los otros tres requisitos que cambian (compose y verificación de la imagen en `deploy`, alta en `api-resources`) van como `MODIFIED`, porque conservan todos sus escenarios y solo añaden uno.

## Dependencia de orden (bloqueante)

- El PR **#179** (`chore/archivar-endurecimiento-restauracion`, archivado del bloque C) ha de estar fusionado antes de archivar este change: `endurecimiento-restauracion` también añade un requisito a la capability `verificacion-e2e`, y el archivado se aplica sobre la spec base.
- El PR **#178** (`feature/arreglo-sonda-mariadb`, #171) ha de estar fusionado antes de este PR: reserva `plans/PLAN-042.md` y toca `tests/e2e/`. Su `E2E (docker)` está en rojo sin diagnosticar.

## Ficheros implicados

| Fichero | Qué cambia |
|---|---|
| `src/auth/session.rs` | `session_cookie` pierde el parámetro `redirect_url` y emite `.secure(true)` incondicional; desaparece `requires_secure`. |
| `src/auth/limite.rs` (nuevo) | Módulo **puro**: limitador de ventana fija por IP, con el reloj y la IP inyectados y el mapa de ventanas acotado. |
| `src/config.rs` | `resolve_auth` exige `OIDC_REDIRECT_URL` presente y `https` en modo `Oidc`; se elimina `DEFAULT_OIDC_REDIRECT_URL`; constante y lectura de `KESTO_ALLOW_INSECURE_ENDPOINT`. |
| `src/state.rs` | Constante `MAX_LOGINS` con purga por caducidad y desalojo de la más antigua; el limitador en `AppState`. |
| `src/error.rs` | Variante `TooManyRequests` y su brazo en `into_response` → `429` con la envoltura. |
| `src/api/mod.rs` | Capa `from_fn_with_state` sobre `GET /api/v1/auth/login` que lee la IP de `ConnectInfo` y cae a la centinela. |
| `src/api/auth.rs` | La llamada a `session_cookie` pierde el argumento del retorno. |
| `src/api/repositories.rs` | `alta` y `actualizar` validan el `endpoint` con el módulo puro y el permiso de la configuración, con `400` que nombra el campo. |
| `src/rustic/endpoint.rs` (nuevo) | Módulo **puro** `validar(valor, permitir_inseguro)` con su tipo de error. |
| `src/main.rs` | `into_make_service_with_connect_info::<SocketAddr>()` al servir. |
| `src/test_support.rs` | El `endpoint` de las filas de prueba pasa a ser una URL `https://…` cuyo camino es la ruta real; `abridor_local`/`comprobador_local` la traducen; las lecturas OIDC de prueba declaran `OIDC_REDIRECT_URL` por `https`. |
| `Cargo.toml` | `url` como dependencia directa, con la versión ya fijada en `Cargo.lock`. |
| `tests/arranque.rs` | Caso de arranque con retorno por `http`. |
| `tests/e2e/arnes.rs` · `tests/e2e/escenarios.rs` | El arranque declara `KESTO_ALLOW_INSECURE_ENDPOINT=1`; escenario del almacén real con el permiso declarado. |
| `compose.yaml` · `.github/workflows/release.yml` | `OIDC_REDIRECT_URL` con la forma `${OIDC_REDIRECT_URL:?}` y por `https` en el smoke test. |
| `docs/DEFINITION.md` · `docs/ENTORNO.md` · `docs/ARQUITECTURA.md` · `.env.example` | Las dos variables, la cookie `Secure` y la validación del `endpoint`. |

## Riesgos y ripples

- **Arnés de pruebas (crítico)**: los tests del alta usan hoy una **ruta de directorio** como `endpoint` (`destino_local_de_prueba`), porque `abridor_local`/`comprobador_local` hacen `Path::new(&acceso.endpoint)`. Con la validación en el borde esas filas pasan a llevar una URL `https://…` cuyo camino es la ruta real, y el abridor la traduce. El cambio queda contenido en `src/test_support.rs`.
- **Ripple de #165**: `src/test_support.rs` (`OIDC_REDIRECT_URL` http), `estado_oidc`/`router_oidc`, los tests de config, `compose.yaml` (hoy no declara la variable) y el smoke test de `release.yml`.
- **Nunca dos cargas pesadas en paralelo**: serializar `e2e`/podman.
- El paquete es **solo binario**: `cargo test --locked`, nunca `--lib`. `just e2e` = 33 escenarios.

## Tareas

### 0. Preparación

- [ ] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [ ] 0.2 Rama desde `development`: `just gf-feature endurecimiento-superficie-api` (verificación: `git branch --show-current`).
- [ ] 0.3 Change aprobado por el usuario (verificación: aprobación explícita y `just check-spec` encuentra el change activo).

### 1. La cookie siempre con `Secure` (SEC-09, #165)

#### RED

- [ ] 1.1 Rust: test —cookie emitida en modo `Oidc` con retorno `https`— exige el atributo `Secure` (verificación: verde hoy; se conserva como contrapeso).
- [ ] 1.2 Rust: test —configuración en modo `Oidc` con `OIDC_REDIRECT_URL` por `http`— exige error de arranque que **nombre la variable** (verificación: rojo hoy, que arranca).
- [ ] 1.3 Rust: test —configuración en modo `Oidc` **sin** `OIDC_REDIRECT_URL`— exige error que nombre la variable, sin caer a ningún defecto (verificación: rojo hoy, que toma el defecto `http`).
- [ ] 1.4 Rust: test —configuración sin las tres `OIDC_*` y con `OIDC_REDIRECT_URL` declarado por `http`— exige modo `Disabled` y ningún error (verificación: verde hoy; se conserva como contrapeso).
- [ ] 1.5 Rust: test —`session_cookie` sin parámetro de retorno— exige `Secure` en el renderizado de la cookie (verificación: rojo hoy, que exige el parámetro).
- [ ] 1.6 Rust: test de integración (`tests/arranque.rs`) —modo `Oidc` con retorno por `http`— exige salida de error y código distinto de cero (verificación: rojo hoy).

#### GREEN

- [ ] 1.7 `src/auth/session.rs`: `session_cookie` pierde el parámetro `redirect_url`, emite `.secure(true)` incondicional y desaparece `requires_secure` (verificación: 1.1, 1.5 en verde).
- [ ] 1.8 `src/config.rs`: `resolve_auth` exige `OIDC_REDIRECT_URL` presente y con esquema `https` en modo `Oidc`, con variante de error nueva que nombra la variable; se elimina `DEFAULT_OIDC_REDIRECT_URL` y su uso (verificación: 1.2–1.4, 1.6 en verde).
- [ ] 1.9 `src/api/auth.rs`: la llamada a `session_cookie` pierde el argumento del retorno (verificación: `cargo test --locked --bin kesto 'api::auth::'` verde).

#### REFACTOR

- [ ] 1.10 Rustdoc del porqué del `Secure` incondicional y del coste de compatibilidad del retorno obligatorio; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

### 2. El mapa de logins tiene tope (SEC-10, #166)

#### RED

- [ ] 2.1 Rust: test —insertar `MAX_LOGINS` entradas y una más— exige que el mapa no pase del tope (verificación: rojo hoy, que crece sin límite).
- [ ] 2.2 Rust: test —con el mapa lleno, insertar una nueva— exige que la desalojada sea la de menor `issued_at` (verificación: rojo hoy).
- [ ] 2.3 Rust: test —con entradas caducadas y el mapa lleno— exige que la purga por caducidad ocurra **antes** del desalojo y que no se desaloje ninguna vigente si queda hueco (verificación: rojo hoy).
- [ ] 2.4 Rust: test —consumir un `state` vigente con el mapa lleno— exige que siga funcionando (verificación: verde hoy; se conserva como contrapeso).

#### GREEN

- [ ] 2.5 `src/state.rs`: constante `MAX_LOGINS`, purga por caducidad seguida del desalojo de la más antigua en `registrar_login`, y el comentario `AVISO` de `:124-128` actualizado a lo que ahora sí se implementa (verificación: 2.1–2.4 en verde).

#### REFACTOR

- [ ] 2.6 Rustdoc del porqué del desalojo frente al rechazo; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

### 3. El login tiene límite de tasa por IP (SEC-10, #166)

#### RED

- [ ] 3.1 Rust: test del módulo puro —`MAX_PETICIONES_DE_LOGIN` peticiones de la misma IP dentro de la ventana se permiten y la siguiente no— (verificación: rojo hoy, que no existe el módulo).
- [ ] 3.2 Rust: test del módulo puro —pasada la ventana, la misma IP recupera su cupo— con reloj inyectado y sin dormir (verificación: rojo hoy).
- [ ] 3.3 Rust: test del módulo puro —una IP distinta no consume el cupo de la primera— (verificación: rojo hoy).
- [ ] 3.4 Rust: test del módulo puro —el mapa de ventanas se poda y no crece sin límite— (verificación: rojo hoy).
- [ ] 3.5 Rust: test de integración —`GET /api/v1/auth/login` por encima del cupo contesta `429` con la envoltura y **no** inserta en el mapa de logins— (verificación: rojo hoy, que contesta `302`).
- [ ] 3.6 Rust: test —una petición dentro del cupo sigue recibiendo su `302`— (verificación: verde hoy; se conserva como contrapeso).
- [ ] 3.7 Rust: test —la respuesta `429` no publica ninguna credencial ni el valor de ninguna cabecera de la petición— (verificación: rojo hoy).

#### GREEN

- [ ] 3.8 `src/auth/limite.rs`: módulo nuevo con el limitador de ventana fija, con el reloj y la IP inyectados (verificación: 3.1–3.4 en verde).
- [ ] 3.9 `src/error.rs`: variante `TooManyRequests` y su brazo en `into_response` → `429` con la envoltura (verificación: 3.5, 3.7 en verde).
- [ ] 3.10 `src/state.rs`: el limitador en `AppState`, con su cupo y su tope (verificación: el estado se construye en las pruebas).
- [ ] 3.11 `src/api/mod.rs`: capa `from_fn_with_state` sobre `GET /api/v1/auth/login` que lee la IP de las extensiones (`ConnectInfo`) y cae a la centinela cuando no la hay (verificación: 3.5, 3.6 en verde).
- [ ] 3.12 `src/main.rs`: `into_make_service_with_connect_info::<SocketAddr>()` al servir (verificación: `tests/arranque.rs` en verde).

#### REFACTOR

- [ ] 3.13 Rustdoc del porqué de la ventana fija, de la IP del socket y de la centinela; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

### 4. El módulo puro del `endpoint` (SEC-07, #163)

#### RED

- [ ] 4.1 Rust: test —`https` con host público o de rango privado— se acepta (verificación: rojo hoy, que no existe el módulo).
- [ ] 4.2 Rust: test —`http`— se rechaza sin permiso y se acepta con permiso (verificación: rojo hoy).
- [ ] 4.3 Rust: test —loopback (`127.0.0.1`, `[::1]`) y nombre `localhost`/`algo.localhost`— se rechazan sin permiso y se aceptan con permiso (verificación: rojo hoy).
- [ ] 4.4 Rust: test —enlace local `169.254.169.254`, `fe80::1`, metadatos `fd00:ec2::254`, no especificada `0.0.0.0` y `[::]`— se rechazan **también** con el permiso concedido (verificación: rojo hoy).
- [ ] 4.5 Rust: test —esquema desconocido, valor que no es una URL absoluta y `endpoint` con credenciales embebidas— se rechazan (verificación: rojo hoy).
- [ ] 4.6 Rust: test —el mensaje del error nombra el campo `endpoint` y **no** repite la URL recibida— (verificación: rojo hoy).

#### GREEN

- [ ] 4.7 `src/rustic/endpoint.rs`: módulo nuevo **puro** con `validar(valor, permitir_inseguro)` y su tipo de error (verificación: 4.1–4.6 en verde).
- [ ] 4.8 `Cargo.toml`: `url` como dependencia directa, con la versión ya fijada en `Cargo.lock` (verificación: `cargo check --locked` y el `Cargo.lock` sin cambios de versión).

#### REFACTOR

- [ ] 4.9 Rustdoc del módulo —qué cierra y qué no— y de la limitación de no resolver el nombre; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

### 5. El borde valida el `endpoint` (SEC-07, #163)

#### RED

- [ ] 5.1 Rust: test de alta con `endpoint` por `http` sin permiso exige `400` nombrando `endpoint` y sin fila escrita (verificación: rojo hoy, que contesta `201` o `502`).
- [ ] 5.2 Rust: test de alta con `endpoint` a loopback sin permiso exige `400` (verificación: rojo hoy).
- [ ] 5.3 Rust: test de alta con `endpoint` por `https` contra un destino inalcanzable exige el `502` de siempre (verificación: verde hoy; se conserva como contrapeso del oráculo que **no** se cierra).
- [ ] 5.4 Rust: test de `PUT` con `endpoint` por `http` sin permiso exige `400` y que la fila no cambie (verificación: rojo hoy).
- [ ] 5.5 Rust: test —con el permiso declarado, el alta contra un `endpoint` por `http` a loopback sigue adelante— (verificación: verde hoy tras 5.7).
- [ ] 5.6 Rust: test de configuración —`KESTO_ALLOW_INSECURE_ENDPOINT` con un valor distinto de `1` y `0`— exige error que nombre la variable (verificación: rojo hoy).

#### GREEN

- [ ] 5.7 `src/api/repositories.rs`: `alta` y `actualizar` validan el `endpoint` con `endpoint::validar` y el permiso de la configuración, con `400` que nombra el campo (verificación: 5.1–5.5 en verde).
- [ ] 5.8 `src/config.rs`: constante `ENV_KESTO_ALLOW_INSECURE_ENDPOINT`, campo en `Config` con el defecto negado y la lectura con el mismo patrón que `KESTO_ALLOW_INSECURE` (verificación: 5.6 en verde).

#### REFACTOR

- [ ] 5.9 Un solo sitio decide qué es un `endpoint` aceptable —el módulo puro— usado por el alta y la edición; rustdoc del porqué del borde y del oráculo que se deja; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

### 6. Arnés, e2e y documentación

- [ ] 6.1 `src/test_support.rs`: el `endpoint` de las filas de prueba pasa a ser una URL `https://…` cuyo camino es la ruta real, y `abridor_local`/`comprobador_local` la traducen; las lecturas OIDC de prueba declaran `OIDC_REDIRECT_URL` por `https` (verificación: `just check` en verde).
- [ ] 6.2 `tests/e2e/arnes.rs`: el arranque declara `KESTO_ALLOW_INSECURE_ENDPOINT=1` (verificación: `just e2e` en verde).
- [ ] 6.3 `tests/e2e/escenarios.rs`: escenario nuevo —el almacén real sigue verificándose con el permiso declarado— (verificación: pasa y fallaría sin la variable en el arnés).
- [ ] 6.4 `compose.yaml`: `OIDC_REDIRECT_URL` con la forma `${OIDC_REDIRECT_URL:?}` y el comentario de las variables obligatorias actualizado (verificación: `podman compose config` se niega sin la variable).
- [ ] 6.5 `.github/workflows/release.yml`: el smoke test declara `OIDC_REDIRECT_URL` por `https` (verificación: el contenedor arranca y `/api/v1/health` contesta `200` con `auth_mode: "oidc"`).
- [ ] 6.6 `docs/DEFINITION.md` §8: `OIDC_REDIRECT_URL` obligatoria con OIDC y la variable `KESTO_ALLOW_INSECURE_ENDPOINT` (verificación: la tabla y el `compose` de referencia concuerdan).
- [ ] 6.7 `docs/ENTORNO.md` y `.env.example`: las dos variables, con su defecto, su obligación y la nota de actualización (verificación: la tabla las lista y el aviso de compatibilidad está).
- [ ] 6.8 `docs/ARQUITECTURA.md`: la cookie `Secure`, el retorno obligatorio y la validación del `endpoint` (verificación: el documento describe las tres reglas).
- [ ] 6.9 `docs/FEATURE-043-endurecimiento-superficie-api.md`: documento de la feature entregada (verificación: existe y cubre los cinco apartados).

### 7. Cierre

- [ ] 7.1 `just check-all` (backend y panel) en verde (verificación: termina sin error).
- [ ] 7.2 Revisión de `@rust-reviewer` sin bloqueantes (verificación: las observaciones aplicadas o contestadas).
- [ ] 7.3 PR a `development` con `Closes #163`, `Closes #165` y `Closes #166` (verificación: el PR abre con los tres cierres y el CI en verde).
- [ ] 7.4 Fusionado el PR: `just spec-archive endurecimiento-superficie-api` (verificación: el delta entra en `openspec/specs/`).
- [ ] 7.5 `plans/PLAN-043.md` → `plans/PLAN-043-COMPLETED.md` (verificación: `git mv` hecho).
- [ ] 7.6 Issue de seguimiento para la resolución del nombre del `endpoint` (verificación: el issue existe y nombra el hallazgo).
