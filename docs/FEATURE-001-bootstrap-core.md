# FEATURE-001 — bootstrap-core

> Implementación del change proposal `openspec/changes/bootstrap-core/`.
> Cerrada el 2026-10-24. Rama: `feature/bootstrap-core`, PR #6 hacia `development`.
> Estado: implementado, auditado y pendiente de archivar la spec.

## Qué quedó construido

Un proceso que arranca, se configura desde el entorno, registra lo que hace sin publicar credenciales y contesta por HTTP con una envoltura JSON única. **Nada más**: ni base de datos, ni autenticación, ni SSE, ni motor.

La especificación son **15 requisitos** y **32 escenarios**, cubiertos por **25 tests**.

| Fichero | Qué dejó |
|---|---|
| `src/config.rs` | `Config`, `AuthMode`, `OidcSettings`, `ConfigError`, el trait `Lookup` y el volcado |
| `src/secret.rs` | `SecretString` sobre `zeroize` |
| `src/error.rs` | `AppError` y su `IntoResponse` |
| `src/api/response.rs` | `ApiResponse`, `ErrorResponse` y `Data` |
| `src/api/health.rs` | la salud |
| `src/api/mod.rs` | el router de `/api/v1` y sus dos *fallbacks* |
| `src/state.rs` | `AppState` |
| `src/main.rs` | arranque, tracing, escucha y apagado |
| `src/test_support.rs` | el arnés común de los tests, bajo `#[cfg(test)]`: no viaja en el binario |

## Contratos

Lo caro de cambiar después. Los nombres, las firmas y el fichero donde viven.

### Configuración

| Tipo | Definición | Fichero |
|---|---|---|
| `Config` | `{ bind_address: SocketAddr, database_url: String, auth: AuthMode, secret: Option<SecretString>, master_key: SecretString, log_filter: String }` | `src/config.rs` |
| `AuthMode` | `Oidc(OidcSettings)` · `Disabled` | `src/config.rs` |
| `OidcSettings` | `{ issuer: String, client_id: String, client_secret: SecretString, redirect_url: String }` | `src/config.rs` |
| `ConfigError` | `MissingVariable { name }` (variable obligatoria ausente), `InvalidValue { name }` (valor presente con formato inválido), `IncompleteOidc { name }` (OIDC a medias) | `src/config.rs` |
| `Lookup` | `fn get(&self, name: &str) -> Option<String>`, implementado para todo `Fn(&str) -> Option<String>` | `src/config.rs` |

Los mensajes de `ConfigError` nombran la variable y **nunca** su valor, ni el de las demás.

| Firma | Qué hace |
|---|---|
| `Config::from_lookup(&impl Lookup) -> Result<Config, ConfigError>` | la carga. No consulta el entorno del proceso: la lectura entra por parámetro |
| `Config::from_env() -> Result<Config, ConfigError>` | el único punto de contacto con el entorno; delega en `from_lookup` |
| `Config::describe() -> serde_json::Value` | el volcado de arranque. Las credenciales ya salen redactadas por el tipo, no por un cuidado de este método |
| `Config::warnings() -> Vec<String>` | función pura —devuelve los avisos, no los registra—, para que el aviso sea comprobable sin capturar logs |

| Constante | Valor | Nota |
|---|---|---|
| `DEFAULT_LOG_FILTER` | `"info"` | `pub`: el arranque la usa en vez de repetir un literal propio. Solo se aplica cuando `RUST_LOG` **está ausente**: presente pero en blanco es error de arranque |
| `DEFAULT_BIND_ADDRESS` | `"0.0.0.0:8080"` | el del §8 |
| `DEFAULT_DATABASE_URL` | `"sqlite:///app/data/kesto.db"` | el del §8 |
| `DEFAULT_OIDC_REDIRECT_URL` | `"http://localhost:8080/api/v1/auth/callback"` | callback local, para desarrollar con proveedor |
| `MIN_CREDENTIAL_LENGTH` | `32` | mínimo que se le exige a una credencial antes de avisar |

### Secretos

| Elemento | Contrato |
|---|---|
| `SecretString` | Envuelve un `zeroize::Zeroizing<String>`. Se construye con `new(String)` |
| `Debug` y `Display` | Redactan a `REDACTED`, la constante `"<redactado>"` |
| Acceso al valor | Solo por `expose() -> &str`. Que sea un método con nombre es lo que hace visible cada sitio que lo usa |
| Borrado | El contenido se borra al caer, por `zeroize` |
| `Clone` | Sí, para poder guardar el mismo secreto en varios sitios sin duplicar el contrato de redacción |

### Errores y envoltura

| Variante de `AppError` | Código | Cuerpo |
|---|---|---|
| `InvalidInput` | `400` | con el motivo: el cliente necesita saber qué traía mal |
| `NotFound` | `404` | con el motivo |
| `MethodNotAllowed` | `405` | con el motivo: se equivocó de método, no de recurso |
| `Other` | `500` | mensaje genérico; la causa **solo** al log |
| `Config` | `500` | mensaje genérico; por construcción solo puede dispararse al atender una petición |

`AppError` implementa `IntoResponse`.

| Tipo | Forma | Nota |
|---|---|---|
| `ApiResponse` | `{status: u16, message: String, data: Data}` | `ApiResponse::new` toma un `StatusCode`, no un `u16`: el cuerpo no puede contradecir la cabecera |
| `ErrorResponse` | `{status: u16, message: String}` | la forma de todo fallo; no lleva `data` |
| `Data` | `None` · `Some(serde_json::Value)` | `None` se serializa como `null` |

### Arranque y apagado

| Elemento | Contrato | Fichero |
|---|---|---|
| `AppState` | `{ config: Arc<Config> }` | `src/state.rs` |
| `serve(state, shutdown: impl Future<Output = ()>) -> io::Result<()>` | Ata el listener **dentro** y sirve con `with_graceful_shutdown`: la dirección no entra como parámetro suelto, sale de la configuración | `src/main.rs` |
| `shutdown_signal()` | `SIGTERM` y `SIGINT`. Si una no se puede registrar, se deja de esperar por ella en vez de apagar el servidor | `src/main.rs` |
| `main() -> std::process::ExitCode` | Devuelve el código en vez de matar el proceso: así los `SecretString` se borran al caer y un `RUST_LOG=off` no puede dejar morir el proceso en silencio | `src/main.rs` |

## Endpoints

| Método y ruta | Código | Cuerpo | Notas |
|---|---|---|---|
| `GET /api/v1/health` | `200` | `{status: 200, message: "servicio en marcha", data: {version, auth_mode}}` | Sin sesión. No publica ni la dirección de escucha, ni la cadena de base de datos, ni el emisor OIDC. El conjunto de claves de `data` está fijado por un test en exactamente `["auth_mode", "version"]` |
| Cualquier ruta desconocida | `404` | `{status, message}` | *Fallback* propio: la misma envoltura, no el cuerpo vacío de Axum |
| Cualquier método no declarado sobre una ruta existente | `405` | `{status, message}` | `POST /api/v1/health` es el caso que fija el test |
| `GET /api/v1/health/` | `404` | `{status, message}` | La ruta se declara **sin** barra final, así que la barra es otra ruta |

`status` en el cuerpo duplica siempre el código de la cabecera.

`/api/v1/health` es **una adición al SRS**, que no lo declara en su §5. Se añade porque el §8 despliega con `restart: always` y el contenedor necesita un health check que no exija sesión —uno que exigiera sesión no serviría para lo que sirve un health check—. Y precisamente por ser un endpoint sin autenticar, limita su cuerpo a la versión y al modo de autenticación: no tiene por qué describir el despliegue a quien lo consulte.

## Decisiones tomadas durante la implementación

Las que no están en la especificación y se decidieron al escribir el código:

1. **Los módulos se declaran `pub` en `main.rs`, aunque sea un binario.** En un binario, lo que no es alcanzable desde la raíz es código muerto; declararlos públicos es lo que impide que el contrato se lea como muerto y deja que el aviso delate, en cambio, los ayudantes privados sin uso. Sin eso, tres variantes que hoy solo construyen los tests —`AppError::InvalidInput`, `AppError::Other` y `Data::None`— aparecerían como muertas, que es justo lo que son desde el punto de vista de un binario. Verificado que esta declaración no esconde ningún otro código muerto.
2. **El listener no se puede inyectar.** `axum::serve::Listener` está sellado, así que no hay forma de meter un listener falso. El test de apagado prueba el mecanismo con una señal **inyectada** —al resolverse, la función de arranque devuelve— sobre `BIND_ADDRESS=127.0.0.1:0`, y la señal real se comprueba a mano.
3. **El escenario del defecto de `BIND_ADDRESS`/`DATABASE_URL` se corrigió en RED.** Decía «un entorno vacío», que es imposible: `KESTO_MASTER_KEY` es obligatoria, así que un entorno vacío falla antes de llegar a los defectos. La spec se corrigió en la fase RED, antes de dar el escenario por bueno.
4. **`DATABASE_URL` es `String` y no `SecretString` por ahora.** Hoy apunta a un SQLite local que no lleva credenciales. El paso a `SecretString` llega con el change que introduce el DSN por red.
5. **`tower-http` se retiró.** Estaba declarada con la feature `trace` y no la usaba nadie. Entra cuando entre su consumidor, no antes.

## Cobertura de la especificación

| Capability | Requisitos | Escenarios |
|---|---|---|
| `config` | 7 | 13 |
| `secrets` | 3 | 7 |
| `api-shell` | 5 | 12 |
| **Total** | **15** | **32** |

**Los 32 escenarios tienen código y test.** Dos grupos se comprueban **además** a mano: las señales reales del sistema (`SIGTERM`/`SIGINT`) y las comprobaciones con el binario en marcha —el volcado en el log, el `curl` a la salud, el `curl` a una ruta inexistente, el arranque sin `KESTO_MASTER_KEY`, el OIDC a medias y el `kill -TERM`—.

El escenario «Las pruebas no mutan el entorno del proceso» **no es asertable en Rust**: comprobar que una variable no cambió exige leer el entorno, y el entorno de un test no se puede aislar del de la suite. Se comprueba con un `grep` de `unsafe` —vacío— y por construcción, porque ningún test llama a `std::env::set_var`.

## Auditoría y qué se hizo con cada hallazgo

`rust-reviewer` auditó el código contra la especificación, posterior a la primera pasada de REFACTOR. Veredicto: **aprobado con reservas**.

| Hallazgo | Desenlace |
|---|---|
| **Bloqueante.** Un método no declarado sobre una ruta existente contestaba `405` **con el cuerpo vacío** —lo que hacía el `MethodRouter` por defecto—, incumpliendo el requisito de la envoltura única | Corregido, con escenario propio en `specs/api-shell` y test propio (`POST /api/v1/health`). Comprobado que el test no es vacuo |
| La salud no cubría el emisor OIDC: su fixture no tenía variables de autenticación | Corregido: el test usa una configuración **con** OIDC y comprueba que no aparecen ni el emisor ni el `client_id`. De paso, el conjunto de claves del cuerpo quedó fijado en vez de confiar en que nadie añada campos |
| El escenario de `RUST_LOG=trace` no tenía test | Corregido. El vector real es el `Debug` de `Config`, no solo el volcado de nivel `info` que era lo único probado |
| La envoltura no quedaba fijada por ningún test, y `ApiResponse::new` podía emitir un cuerpo que contradecía la cabecera | Corregido: el constructor toma un `StatusCode`, y los tests fijan la envoltura entera —el `message` del éxito y el mensaje genérico **sin** la clave `data` en el error— |
| Dos fuentes de verdad para el filtro de registro: un `"info"` literal en `main.rs` hacía que cambiar la constante no cambiara nada | Corregido: el filtro sale de `Config::log_filter`, con `DEFAULT_LOG_FILTER` como única fuente |
| `main` podía morir en silencio y se saltaba el `Drop` de los `SecretString` | Corregido: `ExitCode` en vez de matar el proceso, y el fallo de escucha sale por `stderr` |
| Varias aserciones pasaban por el motivo equivocado o cubrían solo la mitad del escenario | Corregidas: el modo de autenticación del volcado se asierta por su valor; el test de credencial débil comprueba también que la salud contesta; el valor con formato inválido comprueba que el mensaje no lleva el valor; un clon innecesario fuera |
| Una `RUST_LOG` presente pero vacía dejaba el proceso arrancando **sin una sola línea de registro y en silencio**, que es la degradación silenciosa que el requisito de los valores inutilizables prohíbe | Corregido: presente y en blanco es error de arranque nombrando la variable, como ya hacía `BIND_ADDRESS=` vacía. El caso ausente sigue cayendo al valor por defecto, `info`, y hay un test que lo fija. Y el valor se usa recortado de espacios: sin el recorte, `EnvFilter` lee `" info "` como el nombre de un destino y el proceso arranca ciego con un valor que la validación no puede rechazar por no estar en blanco. Lo encontró la propia implementación al probarla contra el binario. |

**Observaciones diferidas**, con su constancia donde toca:

- La validación del vacío en `DATABASE_URL` y su paso a `SecretString` se difieren al change `db-schema`, **con constancia escrita en el plan**.
- El comportamiento de un `OIDC_REDIRECT_URL` suelto se deja como está y se documenta.

## Deuda pendiente y límites aceptados

- **`DATABASE_URL` viaja en claro.** Llega con `db-schema`, que es el que introduce el DSN. Y el vector que más importa no es el volcado de arranque: es el log de error, donde viaja el texto de las bibliotecas y el DSN aparece dentro de él.
- **`zeroize` no alcanza al bloque de entorno del proceso.** Es inherente a que el entorno es del sistema operativo, no del proceso: protege de un volcado de memoria o de un log, no de quien puede leer el entorno. `KESTO_MASTER_KEY` es legible en `/proc/<pid>/environ` mientras el proceso viva.
- **La salud es vida, no disponibilidad.** Es inherente a que no hay nada a lo que preguntar todavía: la comprobación de disponibilidad llega con `db-schema`.
- **El apagado ordenado se prueba por su mecanismo, no con señales reales.** Es inherente a que un test que levante un proceso y le mande `SIGTERM` es un test de integración frágil y lento; la señal se comprueba a mano.
- **No hay traza de peticiones.** Llega con su primer consumidor. Cuando llegue, su span **no** debe incluir la cadena de consulta: el callback de OIDC llevará `?code=` y `?state=`, y un `TraceLayer` por defecto registra la URI completa. Es una obligación de `auth-capability`, no un detalle de formato.
- **Un `OIDC_REDIRECT_URL` suelto se ignora en silencio.** Queda dicho donde lo lee quien despliega.

Cabos del SRS que siguen abiertos y pasan a ser responsabilidad de los changes siguientes:

| Cabo suelto | A quién le toca |
|---|---|
| No hay autenticación en ninguna capa: el §5 no define login y el §8 publica el `8080` | `auth-capability` |
| Los secretos son `TEXT` plano en el §3 | `db-schema` |
| `db_name` es inaplicable con los comandos del §4.A (`--all-databases` / `pg_dumpall`) | `db-schema` |
| Las contraseñas viajan por la línea de comandos y son visibles en el listado de procesos del contenedor | `db-schema` |
| `job_executions` no tiene índice por repositorio | `db-schema` |
| El `compose` del §8 monta el socket `:ro`, aunque el motor necesita `exec` | el change del motor |
| No hay `Dockerfile` | el change que lo escriba |

## Lo que NO trae esta feature

Sin autenticación, sin base de datos, sin SSE, sin motor, sin planificador, sin panel. Los changes `auth-capability` y `db-schema` están sin empezar.

Este documento es el hermano de `plans/PLAN-001.md`, que a partir de ahora se llama `PLAN-001-COMPLETED.md`.
