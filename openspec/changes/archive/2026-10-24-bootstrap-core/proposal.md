# Proposal: bootstrap-core

## Why

El repositorio no tiene código: `src/main.rs` es un `Hello, world!`, `Cargo.toml` no declara una sola dependencia y `cargo test` corre **cero** pruebas (verificado antes de abrir este change). Todo lo que describe `docs/DEFINITION.md` —el motor de ejecución, `rustic_core`, `bollard`, el planificador, el panel— necesita antes un proceso que arranque, lea su configuración, sostenga el estado compartido, registre lo que hace y conteste por HTTP. Nada de eso existe todavía.

Y hay tres decisiones tomadas al cerrar la base que fijan la **forma de la configuración**, y que llegarían tarde si se dejaran para cuando haya endpoints que las usen:

1. **Los secretos se cifran en reposo** con una clave maestra que entra por entorno.
2. **La autenticación es OIDC (`PocketId`)** con entrada directa cuando OIDC no está definido, y la sesión viaja en una cookie `HttpOnly`. Es la solución de `shuul`, adaptada.
3. **La `EventSource` del navegador no puede enviar cabeceras**, así que el token no puede ir en `Authorization`: de ahí la cookie. Es la razón exacta de que Kesto no pueda copiar el `localStorage` de shuul —que hace *polling* y nunca tuvo que enfrentarse a esa limitación—.

`docs/DEFINITION.md` no dice una palabra sobre configuración, autenticación ni manejo de secretos: su §5 lista los endpoints y su §8 un `compose` de ejemplo, y ninguna de las dos cosas define un contrato. Escribir ese contrato después obligaría a retrofitear cada módulo que lea configuración —empezando por `main.rs`— y dejaría los secretos circulando por el log sin que nada lo impidiera.

Este change aterriza **el esqueleto y el contrato de configuración, y nada más**.

### 1. Los nombres que el SRS ya fijó se respetan; los que faltan se fijan aquí

`docs/DEFINITION.md` §8 declara tres variables de entorno en su `compose` de referencia —`RUST_LOG`, `DATABASE_URL` y `BIND_ADDRESS`— y este change las lee **con esos nombres exactos**. No se renombran a un prefijo `KESTO_`: el SRS es la fuente de verdad y un `compose` copiado de la documentación tiene que funcionar tal cual.

Las variables que el SRS no declara son nuevas y se fijan aquí, con prefijo `KESTO_` para que no colisionen con las de otro servicio del mismo host: `KESTO_SECRET` (firma de la sesión), `KESTO_MASTER_KEY` (cifrado en reposo), `OIDC_ISSUER_URL` / `OIDC_CLIENT_ID` / `OIDC_CLIENT_SECRET` / `OIDC_REDIRECT_URL` (el proveedor).

`OIDC_*` conserva los nombres que ya usa shuul, para que los dos servicios de la casa se configuren igual en el mismo `compose`.

### 2. Los dos secretos existen desde el primer commit, no cuando hagan falta

`KESTO_MASTER_KEY` es obligatoria ya, aunque hasta el change `db-schema` no haya nada que cifrar. Es deliberado: un despliegue que arranca hoy sin clave maestra es un despliegue que guardará credenciales S3 en claro mañana, porque nadie va a volver a tocar su `compose` y la ruta sin clave es la que ya está funcionando.

Y los tres valores que son credenciales —la clave de firma, la maestra y el `OIDC_CLIENT_SECRET`— no son `String` sino `SecretString`, un tipo que redacta en `Debug` y `Display` y borra su contenido al caer. La diferencia importa: **el volcado de configuración del arranque es una funcionalidad deseable** —es lo que permite ver de un vistazo con qué está corriendo el servicio— y con un `String` ese volcado publicaría las tres credenciales. Con un `SecretString` el volcado sigue siendo útil y no puede filtrarlas, ni hoy ni cuando alguien añada un campo nuevo.

### 3. La envoltura de respuesta se fija antes de que ningún endpoint la invente

El SRS no define la forma del JSON que devuelven sus endpoints. El panel la va a consumir en cada llamada, así que fijarla antes de que existan endpoints evita que cada uno improvise la suya. Se adopta la de shuul —`{status, message, data}`, y `{status, message}` para los errores— porque es la que el frontend de la casa ya sabe leer.

Con ella llega una decisión sobre errores internos: **un 5xx no cuenta en el cuerpo por qué falló**. Kesto va a manipular credenciales S3 y cadenas de conexión a bases de datos, y el mensaje de un `sqlx::Error` es justo donde acaban. El detalle va al log; al cliente le llega un mensaje genérico.

### 4. `/api/v1/health` se añade al SRS

El §5 no declara un endpoint de salud, pero el contenedor del §8 se despliega con `restart: always` y necesita uno. Se añade, **público** —un health check que exija sesión no sirve para lo que sirve un health check— y respondiendo solo la versión y el modo de autenticación efectivo: ni el `bind`, ni la ruta de la base, ni el emisor OIDC. Es un endpoint sin autenticar y no tiene por qué describir el despliegue a quien lo consulte.

En este change la salud es **vida**, no **disponibilidad**: no hay base de datos a la que preguntar. La comprobación de disponibilidad llega con el `db-schema`.

## What Changes

1. **`Cargo.toml` estrena dependencias.** Axum 0.8 con el servidor de Tokio, `tokio` con `full`, `tracing` + `tracing-subscriber` (con `env-filter`), `serde`/`serde_json`, `thiserror`, `tower-http` (con `trace`) y `zeroize` para `SecretString`. En dev, `tower` con `util` para el arnés de tests. Las versiones son las que ya están resueltas y cacheadas en esta máquina —axum 0.8.9, `tracing-subscriber` 0.3.23, `tower-http` 0.7.1, `zeroize` 1.9.1—, no las que suenen bien. `tokio` se pide como `"1.50.0"` y Cargo resuelve 1.54.1 por semántica *caret*: es la que está cacheada, y no se fija con `=` porque un pin exacto en el manifiesto es una atadura que no aporta nada aquí. Nota posterior (auditoría del 2026-10-24): `tower-http` se retiró —estaba declarada con la feature `trace` y no la usaba nadie—; entra con su primer consumidor, no antes.

2. **`Config` se lee del entorno de una sola vez, al arrancar, y falla en alto si no puede.** `Config::from_lookup` tiene la lectura del entorno **inyectada**, y no llama a `std::env::var` por dentro: en `edition = "2024"` `std::env::set_var` es `unsafe`, así que un test que mutase el entorno del proceso sería código inseguro y además no reproducible en paralelo. Con la lectura inyectada, los tests no tocan el entorno de nadie.

3. **La configuración de OIDC es opcional para permitir el desarrollo, y la ausencia parcial es un error.** `AuthMode` tiene dos estados —`Oidc` cuando las tres variables están, `Disabled` cuando no está ninguna— y un tercer caso que **no es modo alguno sino un error de arranque**: cuando están unas sí y otras no. Un proveedor a medias no es «modo desarrollo»: es un despliegue mal configurado que dejaría el panel abierto sin que nadie se entere. `Disabled` es el modo de desarrollo y se anuncia en voz alta.

4. **`SecretString` y el volcado de configuración del arranque.** El tipo redacta en `Debug` y `Display`, expone el valor solo por un método explícito y borra su contenido al caer. El arranque registra la configuración efectiva una sola vez, a nivel `info`, con las credenciales redactadas.

5. **El modelo de error y la envoltura JSON.** `AppError` con `IntoResponse`, `ApiResponse` (`{status, message, data}`) y un *fallback* para las rutas desconocidas de `/api/v1` que contesta con la misma envoltura en vez del cuerpo vacío que Axum devuelve por defecto.

6. **`AppState` y el router.** `AppState` con el `Arc<Config>`, un router `/api/v1` con la salud, y apagado ordenado con `SIGTERM`/`SIGINT` para que el `restart: always` del §8 no corte una respuesta a medias.

7. **El arnés de tests.** El router se construye sin abrir un socket y se ejercita con `tower::ServiceExt::oneshot`, que es lo que permite que las pruebas de los endpoints sean unitarias y rápidas. Se estrena con la salud y con los casos de error.

**Decisiones tomadas al abrir el change:**

- **No se crean los seis directorios vacíos del §7.** El SRS recomienda `src/{api,db,docker,engine,rustic,scheduler}`, y este change crea solo `api/` —lo único con contenido— más `config.rs`, `error.rs`, `state.rs` y `secret.rs`. Un `db/` vacío no documenta nada y sí obliga a mantener un `mod` que no compila nada. Cada directorio aparece con el change que le da contenido, con el nombre que el §7 ya fijó.
- **No se usa el crate `secrecy`.** Es la opción de referencia para este problema, pero no está en la caché local de Cargo y no se quiere añadir una dependencia que obligue a red. Se envuelve `zeroize` —que sí está— en un `SecretString` propio de treinta líneas, que es además lo que hace testeable la redacción.
- **`BIND_ADDRESS` y `DATABASE_URL` tienen valor por defecto, y es el del §8.** El §8 los declara explícitamente, pero un binario que se niega a arrancar porque no le han dicho en qué puerto escuchar es un binario incómodo de ejecutar a mano en desarrollo. Los valores por defecto son los del §8, así que un `compose` copiado de la documentación se comporta igual.
- **`DATABASE_URL` se guarda como `String` y el volcado la imprime.** Hoy el SRS la fija a una ruta SQLite (`sqlite:///app/data/kesto.db`), que no lleva credenciales. El día que Kesto apunte a un PostgreSQL por red, esa cadena lleva usuario y contraseña y tiene que pasar a `SecretString`: se anota como límite conocido en vez de resolverlo por si acaso.
- **El volcado de configuración va a `info`, no a `debug`.** Es la línea que dice qué servicio está corriendo: se quiere en un despliegue estándar, y por eso mismo es la que más importa que esté redactada.
- **`KESTO_SECRET` es obligatoria solo cuando la autenticación está encendida.** Sin OIDC no hay sesión que firmar, así que exigirla en desarrollo sería pedir una credencial que no se usa.
- **`RUST_LOG` se respeta si viene, y si no el filtro por defecto es `info`.** El §8 lo declara como `info,kesto=debug` en producción; el valor por defecto del binario no impone un nivel de nadie.
- **El apagado ordenado no se prueba en automático.** Verificar que `SIGTERM` cierra el servidor exige levantar un proceso y señalarle, que es un test de integración frágil y lento. Lo que sí se prueba es el mecanismo: que la función de arranque devuelve cuando la señal de cierre se resuelve. La señal, a mano.
- **`openspec` entra en el `exclude` del crate.** Se acaba de crear y no tiene por qué viajar en un paquete publicado.

## Non-goals

- **No hay base de datos.** Ni `sqlx`, ni `migrations/`, ni el esquema del §3, ni la comprobación de disponibilidad en la salud. Todo eso es `db-schema`.
- **No hay autenticación.** Ni middleware, ni endpoints `/api/v1/auth/*`, ni cookie de sesión, ni el cliente OIDC. La configuración de OIDC se lee y se valida aquí porque es una decisión de arranque; usarla para autenticar es `auth-capability`.
- **No se parte el router en público y protegido.** Es la estructura que el middleware necesita para colgarse con `route_layer` —como en shuul—, pero quien la necesita es la autenticación y llega con ella. Hoy partiría un router de un solo endpoint en dos sin que nada lo justifique.
- **No hay SSE.** Ni el stream de `/api/v1/events`, ni el formato de mensaje del §5.
- **No hay motor, ni `bollard`, ni `rustic_core`, ni planificador, ni panel, ni `Dockerfile`.** Cada uno tiene su change.
- **No se toca el pipeline, ni `release-prepare`, ni la versión.** Añadir dependencias no cambia ninguna guarda: la imagen sigue saltándose por falta de `Dockerfile` y `ui/package.json`, y el crate por el `publish = false`.

## Impact

- `Cargo.toml` — el bloque de dependencias y el de dev-dependencies; `openspec` entra en `exclude`
- `Cargo.lock` — regenerado
- `src/main.rs` — deja de ser un `Hello, world!`: arranque, tracing, configuración, router, apagado
- `src/config.rs` — **nuevo**: `Config`, `AuthMode` y la lectura del entorno
- `src/secret.rs` — **nuevo**: `SecretString`
- `src/error.rs` — **nuevo**: `AppError` y su `IntoResponse`
- `src/state.rs` — **nuevo**: `AppState`
- `src/api/mod.rs` — **nuevo**: el router de `/api/v1`
- `src/api/health.rs` — **nuevo**: la salud
- `src/api/response.rs` — **nuevo**: `ApiResponse` y `Data`
- `openspec/specs/config/spec.md`, `openspec/specs/secrets/spec.md` y `openspec/specs/api-shell/spec.md` — capabilities nuevas (`ADDED`); al archivar hay que redactar su `Purpose`, no dejarlo en `TBD`
- `AGENTS.md` — la tabla de variables de entorno y el modo de desarrollo
- `README.md` — la sección de estado deja de decir que `src/main.rs` es un `Hello, world!`
- tests: unitarios de configuración, secretos, envoltura de error y salud
- **Ninguna aserción existente se debilita**: hoy hay cero pruebas —`cargo test` pasa con 0 tests—, así que no hay nada que romper
