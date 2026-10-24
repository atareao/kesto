# Tasks: bootstrap-core

## 0. Prerrequisito: las dependencias

No hay ciclo RED posible sin compilador: hoy `Cargo.toml` no declara una sola dependencia y `cargo test` corre cero pruebas. Este paso no es RED ni GREEN, es la condición para que existan.

- [ ] 0.1 `Cargo.toml`: `axum` 0.8 con las features `macros`, `json` y `tokio`; `tokio` con `full`; `tracing`; `tracing-subscriber` con `env-filter`; `serde` con `derive`; `serde_json`; `thiserror`; `tower-http` con `trace`; `zeroize`. Y en `[dev-dependencies]`, `tower` con `util`.
- [ ] 0.2 `Cargo.toml`: añadir `openspec` al `exclude` del crate.
- [ ] 0.3 `cargo check` limpio y `cargo tree` sin duplicar ninguna versión mayor.

## 1. RED (pruebas que fallan primero)

- [ ] 1.1 `src/secret.rs`: test que fija que `Debug` de un `SecretString` no contiene el valor y sí una marca de redacción.
- [ ] 1.2 `src/secret.rs`: test que fija que `Display` tampoco lo contiene.
- [ ] 1.3 `src/secret.rs`: test que fija que el valor se obtiene por el método dedicado.
- [ ] 1.4 `src/config.rs`: test que fija que `BIND_ADDRESS` y `DATABASE_URL` se leen del entorno con el nombre que declara el §8.
- [ ] 1.5 `src/config.rs`: test que fija los valores por defecto con un entorno vacío, y que son los del §8.
- [ ] 1.6 `src/config.rs`: test que fija que sin `KESTO_MASTER_KEY` falla en alto y el mensaje nombra la variable.
- [ ] 1.7 `src/config.rs`: test que fija que un `BIND_ADDRESS` sin puerto falla en alto nombrando la variable.
- [ ] 1.8 `src/config.rs`: test que fija que las tres variables `OIDC_*` presentes dan modo `Oidc` y el emisor correcto.
- [ ] 1.9 `src/config.rs`: test que fija que ninguna variable `OIDC_*` da modo `Disabled`.
- [ ] 1.10 `src/config.rs`: test que fija que una configuración OIDC a medias **falla en alto** nombrando la variable que falta, y que no degrada a `Disabled`.
- [ ] 1.11 `src/config.rs`: test que fija que `KESTO_SECRET` es obligatoria con el modo `Oidc` y prescindible con `Disabled`.
- [ ] 1.12 `src/config.rs`: test que fija que los valores salen de la lectura inyectada, sin tocar el entorno del proceso.
- [ ] 1.13 `src/config.rs`: test que fija que el volcado de configuración no contiene ninguna de las tres credenciales y sí la dirección de escucha y el modo de autenticación.
- [ ] 1.14 `src/config.rs`: test que fija que una credencial de 12 caracteres produce un aviso y una de 64 no.
- [ ] 1.15 `src/error.rs`: test que fija que un error de entrada da `400` con el motivo en el cuerpo.
- [ ] 1.16 `src/error.rs`: test que fija que un error interno da `500` con mensaje genérico y **sin** el texto de la causa, aunque ese texto contenga una credencial.
- [ ] 1.17 `src/api/health.rs`: test que fija que `GET /api/v1/health` contesta `200` sin credenciales y trae la versión.
- [ ] 1.18 `src/api/health.rs`: test que fija que la salud declara el modo de autenticación efectivo.
- [ ] 1.19 `src/api/health.rs`: test que fija que la salud **no** contiene la dirección de escucha ni la cadena de base de datos.
- [ ] 1.20 `src/api/mod.rs`: test que fija que `GET /api/v1/no-existe` da `404` con cuerpos JSON `{status, message}`, no vacío.
- [ ] 1.21 `src/api/mod.rs`: test que fija el apagado ordenado con una señal de cierre inyectada: al resolverse, la función de arranque devuelve.
- [ ] 1.22 Ejecutar `just check`: las pruebas nuevas fallan por no existir el código, y la suite compila lo justo para fallar por aserción.

## 2. GREEN (implementación mínima)

- [ ] 2.1 `src/secret.rs`: `SecretString` sobre `zeroize`, con `Debug`/`Display` redactados, método explícito de acceso y borrado al caer.
- [ ] 2.2 `src/config.rs`: `AuthMode` con `Oidc(OidcSettings)` y `Disabled`, y el error de arranque para la configuración a medias.
- [ ] 2.3 `src/config.rs`: `Config::from_lookup` con la lectura inyectada, los nombres del §8, los valores por defecto y las validaciones que fallan en alto.
- [ ] 2.4 `src/config.rs`: el volcado de configuración a `info`, con las credenciales redactadas por el tipo.
- [ ] 2.5 `src/config.rs`: el aviso de credencial débil, sin impedir el arranque.
- [ ] 2.6 `src/config.rs`: el aviso de modo desarrollo cuando la autenticación está apagada.
- [ ] 2.7 `src/error.rs`: `AppError` con `IntoResponse`, códigos por variante y mensaje genérico en los `5xx`, con el detalle al log.
- [ ] 2.8 `src/api/response.rs`: `ApiResponse` (`{status, message, data}`) y `Data`.
- [ ] 2.9 `src/state.rs`: `AppState` con el `Arc<Config>`.
- [ ] 2.10 `src/api/health.rs`: la salud, con la ruta declarada sin barra final.
- [ ] 2.11 `src/api/mod.rs`: el router de `/api/v1` y el *fallback* de ruta desconocida con la envoltura de error.
- [ ] 2.12 `src/main.rs`: init de tracing desde `RUST_LOG`, carga de configuración, volcado de arranque, router, escucha y apagado ordenado con `SIGTERM`/`SIGINT` sobre una señal inyectable.
- [ ] 2.13 El arnés de tests con `tower::ServiceExt::oneshot`, construyendo el router sin abrir socket.
- [ ] 2.14 `just check` en verde.

## 3. REFACTOR

- [ ] 3.1 `cargo fmt --all` y `cargo clippy --all-targets -- -D warnings` limpios.
- [ ] 3.2 Ningún test declara `unsafe` ni muta el entorno del proceso: la configuración se prueba por la lectura inyectada.
- [ ] 3.3 Ninguna credencial se interpola en ninguna llamada de log: comprobar que la redacción la lleva el tipo y no un cuidado manual en cada sitio.
- [ ] 3.4 El layout respeta el §7: `src/api/` es un directorio, y los otros cinco módulos del §7 **no** existen todavía.
- [ ] 3.5 `just doc` sin enlaces rotos.

## 4. Verificación manual (operador)

- [ ] 4.1 Arrancar con un `.env` de desarrollo y comprobar en el log el volcado de configuración: se ve el `bind`, el modo de autenticación y el aviso de desarrollo, y **no** se ve ninguna credencial.
- [ ] 4.2 `curl -i localhost:8080/api/v1/health` sin credenciales: `200` con la versión y el modo.
- [ ] 4.3 `curl -i localhost:8080/api/v1/no-existe`: `404` con cuerpo JSON, no vacío.
- [ ] 4.4 Arrancar sin `KESTO_MASTER_KEY`: el proceso muere con un mensaje que nombra la variable y nada más.
- [ ] 4.5 Poner `OIDC_ISSUER_URL` y `OIDC_CLIENT_ID` sin `OIDC_CLIENT_SECRET`: el proceso muere nombrando `OIDC_CLIENT_SECRET`, **no** arranca en modo desarrollo.
- [ ] 4.6 `kill -TERM` al proceso: cierra de forma ordenada y termina con éxito.

## 5. Cierre

- [ ] 5.1 `just spec-validate bootstrap-core` en modo estricto.
- [ ] 5.2 `AGENTS.md`: la tabla de variables de entorno y el modo de desarrollo.
- [ ] 5.3 `README.md`: la sección de estado deja de describir `src/main.rs` como un `Hello, world!`.
- [ ] 5.4 `docs/FEATURE-001-bootstrap-core.md` con lo que realmente quedó construido.
- [ ] 5.5 `just plan-done` y `just spec-archive bootstrap-core`.
