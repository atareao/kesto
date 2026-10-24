# Tasks: bootstrap-core

## 0. Prerrequisito: las dependencias

No hay ciclo RED posible sin compilador: hoy `Cargo.toml` no declara una sola dependencia y `cargo test` corre cero pruebas. Este paso no es RED ni GREEN, es la condición para que existan.

- [x] 0.1 `Cargo.toml`: `axum` 0.8 con las features `macros`, `json` y `tokio`; `tokio` con `full`; `tracing`; `tracing-subscriber` con `env-filter`; `serde` con `derive`; `serde_json`; `thiserror`; `zeroize`. Y en `[dev-dependencies]`, `tower` con `util`.
- [x] 0.2 `Cargo.toml`: añadir `openspec` al `exclude` del crate.
- [x] 0.3 `cargo check` limpio y `cargo tree` sin duplicar ninguna versión mayor.

`cargo check` limpio, verificado. Sobre el `cargo tree`: ninguna dependencia **directa** está duplicada, pero `syn` aparece en dos versiones mayores —2.0.119 tras `axum-macros` y `tracing-attributes`, 3.0.9 tras `serde_derive`, `thiserror-impl` y `tokio-macros`—. Es duplicación **transitiva de crates procedurales**: unificarla obligaría a bajar la versión de `serde`, `thiserror` o `tokio`, que es peor que el propio duplicado. Se acepta, y queda constado aquí en vez de tildar la tarea como si no existiera.

`tower-http` se retiró el 2026-10-24, con la auditoría: estaba declarada con la feature `trace` y no la usaba nadie. El spec no pide trazar peticiones, así que entra cuando entre su consumidor, no antes. Cuando llegue, el span NO SHALL incluir la cadena de consulta: el callback de OIDC lleva `?code=` y `?state=`, y un `TraceLayer` por defecto registra la URI completa — sería una fuga de credencial por el log, no un detalle de formato.

## 1. RED (pruebas que fallan primero)

- [x] 1.1 `src/secret.rs`: test que fija que `Debug` de un `SecretString` no contiene el valor y sí una marca de redacción.
- [x] 1.2 `src/secret.rs`: test que fija que `Display` tampoco lo contiene.
- [x] 1.3 `src/secret.rs`: test que fija que el valor se obtiene por el método dedicado.
- [x] 1.4 `src/config.rs`: test que fija que `BIND_ADDRESS` y `DATABASE_URL` se leen del entorno con el nombre que declara el §8.
- [x] 1.5 `src/config.rs`: test que fija los valores por defecto con un entorno vacío, y que son los del §8.
- [x] 1.6 `src/config.rs`: test que fija que sin `KESTO_MASTER_KEY` falla en alto y el mensaje nombra la variable.
- [x] 1.7 `src/config.rs`: test que fija que un `BIND_ADDRESS` sin puerto falla en alto nombrando la variable.
- [x] 1.8 `src/config.rs`: test que fija que las tres variables `OIDC_*` presentes dan modo `Oidc` y el emisor correcto.
- [x] 1.9 `src/config.rs`: test que fija que ninguna variable `OIDC_*` da modo `Disabled`.
- [x] 1.10 `src/config.rs`: test que fija que una configuración OIDC a medias **falla en alto** nombrando la variable que falta, y que no degrada a `Disabled`.
- [x] 1.11 `src/config.rs`: test que fija que `KESTO_SECRET` es obligatoria con el modo `Oidc` y prescindible con `Disabled`.
- [x] 1.12 `src/config.rs`: test que fija que los valores salen de la lectura inyectada, sin tocar el entorno del proceso.
- [x] 1.13 `src/config.rs`: test que fija que el volcado de configuración no contiene ninguna de las tres credenciales y sí la dirección de escucha y el modo de autenticación.
- [x] 1.14 `src/config.rs`: test que fija que una credencial de 12 caracteres produce un aviso y una de 64 no.
- [x] 1.15 `src/error.rs`: test que fija que un error de entrada da `400` con el motivo en el cuerpo.
- [x] 1.16 `src/error.rs`: test que fija que un error interno da `500` con mensaje genérico y **sin** el texto de la causa, aunque ese texto contenga una credencial.
- [x] 1.17 `src/api/health.rs`: test que fija que `GET /api/v1/health` contesta `200` sin credenciales y trae la versión.
- [x] 1.18 `src/api/health.rs`: test que fija que la salud declara el modo de autenticación efectivo.
- [x] 1.19 `src/api/health.rs`: test que fija que la salud **no** contiene la dirección de escucha ni la cadena de base de datos.
- [x] 1.20 `src/api/mod.rs`: test que fija que `GET /api/v1/no-existe` da `404` con cuerpos JSON `{status, message}`, no vacío.
- [x] 1.21 `src/api/mod.rs`: test que fija el apagado ordenado con una señal de cierre inyectada: al resolverse, la función de arranque devuelve.
- [x] 1.22 Ejecutar `just check`: las pruebas nuevas fallan por no existir el código, y la suite compila lo justo para fallar por aserción.

## 2. GREEN (implementación mínima)

- [x] 2.1 `src/secret.rs`: `SecretString` sobre `zeroize`, con `Debug`/`Display` redactados, método explícito de acceso y borrado al caer.
- [x] 2.2 `src/config.rs`: `AuthMode` con `Oidc(OidcSettings)` y `Disabled`, y el error de arranque para la configuración a medias.
- [x] 2.3 `src/config.rs`: `Config::from_lookup` con la lectura inyectada, los nombres del §8, los valores por defecto y las validaciones que fallan en alto.
- [x] 2.4 `src/config.rs`: el volcado de configuración a `info`, con las credenciales redactadas por el tipo.
- [x] 2.5 `src/config.rs`: el aviso de credencial débil, sin impedir el arranque.
- [x] 2.6 `src/config.rs`: el aviso de modo desarrollo cuando la autenticación está apagada.
- [x] 2.7 `src/error.rs`: `AppError` con `IntoResponse`, códigos por variante y mensaje genérico en los `5xx`, con el detalle al log.
- [x] 2.8 `src/api/response.rs`: `ApiResponse` (`{status, message, data}`) y `Data`.
- [x] 2.9 `src/state.rs`: `AppState` con el `Arc<Config>`.
- [x] 2.10 `src/api/health.rs`: la salud, con la ruta declarada sin barra final.
- [x] 2.11 `src/api/mod.rs`: el router de `/api/v1` y el *fallback* de ruta desconocida con la envoltura de error.
- [x] 2.12 `src/main.rs`: init de tracing desde `RUST_LOG`, carga de configuración, volcado de arranque, router, escucha y apagado ordenado con `SIGTERM`/`SIGINT` sobre una señal inyectable.
- [x] 2.13 El arnés de tests con `tower::ServiceExt::oneshot`, construyendo el router sin abrir socket.
- [x] 2.14 `just check` en verde.

## 3. REFACTOR

- [x] 3.1 `cargo fmt --all` y `cargo clippy --all-targets -- -D warnings` limpios.
- [x] 3.2 Ningún test declara `unsafe` ni muta el entorno del proceso: la configuración se prueba por la lectura inyectada.
- [x] 3.3 Ninguna credencial se interpola en ninguna llamada de log: comprobar que la redacción la lleva el tipo y no un cuidado manual en cada sitio.
- [x] 3.4 El layout respeta el §7: `src/api/` es un directorio, y los otros cinco módulos del §7 **no** existen todavía.
- [x] 3.5 `just doc` sin enlaces rotos.

Estas correcciones salen de la auditoría de `rust-reviewer` del 2026-10-24, posterior a la primera pasada de REFACTOR. Van aquí, y no en las secciones 1 y 2, porque llegaron después: no estaban en el RED ni en el GREEN, las encontró la revisión.

- [x] 3.6 `src/api/mod.rs`: un método no declarado sobre una ruta existente contesta `405` con la envoltura JSON y no con el cuerpo vacío, que es lo que hacía el `MethodRouter` por defecto e incumplía el requisito de la envoltura única. Con test propio (`POST /api/v1/health`) y su escenario en `specs/api-shell`.
- [x] 3.7 `src/api/health.rs`: el test de «no describe el despliegue» usa una configuración **con** OIDC, comprueba que no aparecen ni el emisor ni el `client_id`, y fija el conjunto exacto de claves del cuerpo en vez de confiar en que nadie añada campos.
- [x] 3.8 `src/config.rs`: el test del volcado comprueba también el `Debug` de `Config`, que es el vector real de `RUST_LOG=debug`/`trace`. El escenario lo pedía desde el principio y hasta ahora lo único probado era el volcado de nivel `info`.
- [x] 3.9 `src/api/response.rs`: `ApiResponse::new` toma un `StatusCode` y no un `u16`, para que el cuerpo no pueda contradecir la cabecera. Y los tests fijan la envoltura entera: `message` en el éxito, y mensaje genérico **sin** la clave `data` en el error.
- [x] 3.10 `src/main.rs`: el filtro de registro sale de `Config::log_filter`, con `DEFAULT_LOG_FILTER` como única fuente de verdad. Antes había un `"info"` literal en `main.rs` que hacía que cambiar la constante no cambiara nada.
- [x] 3.11 `Cargo.toml`: se retira `tower-http`, declarada y sin usar.
- [x] 3.12 `src/main.rs`: `main` devuelve `ExitCode` en vez de matar el proceso, y el fallo de escucha también sale por `stderr`. Así los `SecretString` se borran al caer (que es su contrato) y el proceso no puede morir en silencio por un `RUST_LOG=off`.
- [x] 3.13 `src/config.rs`: el modo de autenticación del volcado se asierta por su valor. La aserción anterior pasaba por un motivo equivocado: la palabra `oidc` también aparece como clave del objeto anidado.
- [x] 3.14 `src/api/health.rs`: test de que una credencial débil no impide contestar la salud, que es la segunda mitad de ese escenario.
- [x] 3.15 `src/config.rs`: el test del valor con formato inválido comprueba que el mensaje no lleva el valor, que es lo que el requisito pide y solo se asertaba en el caso de la variable ausente.
- [x] 3.16 `src/error.rs`: fuera el clon innecesario del test que precisamente demuestra que no se copia de más, y el registro de `AppError::Config` deja de describirse como error de arranque cuando por construcción solo puede dispararse al atender una petición.

## 4. Verificación manual (operador)

- [x] 4.1 Arrancar con un `.env` de desarrollo y comprobar en el log el volcado de configuración: se ve el `bind`, el modo de autenticación y el aviso de desarrollo, y **no** se ve ninguna credencial.
- [x] 4.2 `curl -i localhost:8080/api/v1/health` sin credenciales: `200` con la versión y el modo.
- [x] 4.3 `curl -i localhost:8080/api/v1/no-existe`: `404` con cuerpo JSON, no vacío.
- [x] 4.4 Arrancar sin `KESTO_MASTER_KEY`: el proceso muere con un mensaje que nombra la variable y nada más.
- [x] 4.5 Poner `OIDC_ISSUER_URL` y `OIDC_CLIENT_ID` sin `OIDC_CLIENT_SECRET`: el proceso muere nombrando `OIDC_CLIENT_SECRET`, **no** arranca en modo desarrollo.
- [x] 4.6 `kill -TERM` al proceso: cierra de forma ordenada y termina con éxito.

## 5. Cierre

- [ ] 5.1 `just spec-validate bootstrap-core` en modo estricto.
- [ ] 5.2 `AGENTS.md`: la tabla de variables de entorno y el modo de desarrollo.
- [ ] 5.3 `README.md`: la sección de estado deja de describir `src/main.rs` como un `Hello, world!`.
- [ ] 5.4 `docs/FEATURE-001-bootstrap-core.md` con lo que realmente quedó construido.
- [ ] 5.5 `just plan-done` y `just spec-archive bootstrap-core`.
