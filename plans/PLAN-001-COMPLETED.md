# PLAN-001 — bootstrap-core

> Implementación del change proposal `bootstrap-core`, archivado en `openspec/changes/archive/2026-10-24-bootstrap-core/`.
> Aprobado el 2026-10-24 y cerrado el 2026-10-24. Rama: `feature/bootstrap-core`, PR #6.
> Especificación que manda: `openspec/specs/` (**15 requisitos, 32 escenarios**). El change se archivó al cerrar, en `openspec/changes/archive/2026-10-24-bootstrap-core/`.

## Objetivo

Un proceso que arranca, se configura desde el entorno, registra lo que hace sin publicar credenciales y contesta por HTTP con una envoltura JSON única. **Nada más**: ni base de datos, ni autenticación, ni SSE, ni motor.

El valor de este plan no está en las líneas de código —son pocas— sino en que fija **el contrato de configuración** y **la forma del JSON**, que son las dos cosas caras de cambiar después.

## Criterios de aceptación

Al cerrar, todo esto tiene que ser cierto y comprobable:

| # | Criterio | Cómo se comprueba |
|---|---|---|
| A1 | Los nombres del §8 del SRS se leen tal cual, sin renombrar | Test de `Config` + lectura del `compose` de docs |
| A2 | Sin `KESTO_MASTER_KEY` el proceso no arranca y nombra la variable | Test + `just dev` a mano |
| A3 | `OIDC_*` a medias **muere**; no degrada a modo desarrollo | Test + verificación manual 4.5 |
| A4 | Ninguna credencial aparece en el log a ningún nivel de `RUST_LOG` | Test con `RUST_LOG=trace` |
| A5 | Una credencial de menos de 32 caracteres avisa y arranca igual | Test |
| A6 | `/api/v1/health` contesta `200` sin sesión y sin describir el despliegue | Test + `curl` |
| A7 | Una ruta desconocida da `404` con cuerpo JSON, no vacío | Test + `curl` |
| A8 | Un `5xx` no filtra la causa en el cuerpo | Test con una credencial dentro del mensaje del error |
| A9 | Ningún test declara `unsafe` ni muta el entorno del proceso | `grep -r unsafe src/` vacío |
| A10 | `just check` y `just doc` limpios | CI |

## Fases

### Fase 0 — Dependencias (prerrequisito, no es RED)

Hoy `Cargo.toml` no declara una sola dependencia y `cargo test` corre cero pruebas: sin compilador no hay ciclo RED posible. Versiones **ya cacheadas en esta máquina**, no las que suenen bien: axum 0.8.9, tokio 1.50, `tracing-subscriber` 0.3.23, `zeroize` 1.9.1.

- `axum` 0.8 con `macros`, `json`, `tokio` · `tokio` con `full` · `tracing` · `tracing-subscriber` con `env-filter` · `serde` con `derive` · `serde_json` · `thiserror` · `zeroize`
- `[dev-dependencies]`: `tower` con `util`

Nota posterior (auditoría del 2026-10-24): **`tower-http` se retiró.** Estaba declarada con la feature `trace` y no la usaba nadie. El spec no pide trazar peticiones, así que entra cuando entre su consumidor, no antes. Cuando llegue, el span NO SHALL incluir la cadena de consulta: el callback de OIDC lleva `?code=` y `?state=`, y un `TraceLayer` por defecto registra la URI completa — sería una fuga de credencial por el log, no un detalle de formato.

- `openspec` entra en el `exclude` del crate

### Fase 1 — RED

Tests primero, y que fallen. **22 pasos (1.1–1.22 de `tasks.md`)**, agrupados:

| Bloque | Fichero | Qué fija |
|---|---|---|
| Secretos | `src/secret.rs` | `Debug` y `Display` no revelan el valor; el valor sale por el método dedicado |
| Configuración | `src/config.rs` | Nombres del §8, valores por defecto, fallo en alto, los tres desenlaces de OIDC, dependencia de `KESTO_SECRET`, lectura inyectada, volcado redactado, aviso de credencial débil |
| Errores | `src/error.rs` | `400` con motivo, `500` genérico y sin la causa |
| Salud y router | `src/api/` | `200` sin credenciales, sin describir el despliegue, `404` con envoltura, apagado con señal inyectada |

**Puerta:** ejecutar `just check` y confirmar que lo nuevo **falla** y que la suite compila lo justo para fallar por aserción. Si algo pasa en RED, el test no prueba nada.

### Fase 2 — GREEN

Implementación mínima para que los 32 escenarios pasen:

1. `SecretString` sobre `zeroize` — redacción en `Debug`/`Display`, acceso explícito, borrado al caer
2. `AuthMode` — `Oidc(OidcSettings)` / `Disabled`, y el error de arranque para la configuración a medias
3. `Config::from_lookup` — lectura inyectada, nombres del §8, defectos, validaciones que fallan en alto
4. Volcado de configuración a `info`, con la redacción a cargo del tipo (no de un cuidado manual en cada sitio)
5. Aviso de credencial débil · aviso de modo desarrollo
6. `AppError` con `IntoResponse` — códigos por variante, genérico en `5xx`, detalle al log
7. `ApiResponse` (`{status, message, data}`) y `Data`
8. `AppState` con el `Arc<Config>`
9. La salud, con la ruta declarada sin barra final
10. Router de `/api/v1` y *fallback* de ruta desconocida con la envoltura de error
11. `main.rs` — tracing desde `RUST_LOG`, configuración, volcado, router, escucha, apagado con `SIGTERM`/`SIGINT` sobre señal inyectable
12. Arnés de tests con `tower::ServiceExt::oneshot`, sin abrir socket

**Puerta:** `just check` en verde.

### Fase 3 — REFACTOR

- `cargo fmt --all` y `cargo clippy --all-targets -- -D warnings`
- Confirmar que **ningún** test declara `unsafe` ni muta el entorno (A9)
- Confirmar que la redacción de credenciales la lleva el tipo, no un cuidado disperso por las llamadas de log
- Confirmar el layout del §7: existe `src/api/` y **no** existen los otros cinco módulos todavía
- `just doc` sin enlaces rotos

### Fase 4 — Verificación manual (operador)

- Con un `.env` de desarrollo: el log muestra el `bind` y el modo, y **ninguna** credencial
- `curl -i localhost:8080/api/v1/health` → `200` con versión y modo
- `curl -i localhost:8080/api/v1/no-existe` → `404` con JSON
- Arrancar sin `KESTO_MASTER_KEY` → muere nombrando la variable
- `OIDC_ISSUER_URL` + `OIDC_CLIENT_ID` sin secret → muere nombrando `OIDC_CLIENT_SECRET`
- `kill -TERM` → cierre ordenado, salida con éxito

### Fase 5 — Auditoría y cierre

- `rust-reviewer` audita el código contra la especificación
- Verificación de que los 32 escenarios están implementados y cubiertos
- `just spec-validate bootstrap-core` y `just spec-archive bootstrap-core`
- `AGENTS.md`: tabla de variables de entorno y modo de desarrollo
- `README.md`: la sección de estado deja de decir que `src/main.rs` es un `Hello, world!`
- `docs/FEATURE-001-bootstrap-core.md`
- Renombrar este plan a `PLAN-001-COMPLETED.md`

## Decisiones que no se reabren

Tomadas y justificadas en el change proposal; si la implementación las contradice, es un error de la implementación, no del plan:

1. **`KESTO_MASTER_KEY` es obligatoria ya**, aunque hasta `db-schema` no haya nada que cifrar. Un despliegue que arranca sin clave maestra es el que guardará credenciales S3 en claro mañana.
2. **Los nombres del §8 no se renombran.** Un `compose` copiado de `docs/DEFINITION.md` funciona sin editar una línea.
3. **OIDC a medias es un error de arranque, no un modo desarrollo.** Degradar a `Disabled` dejaría el panel abierto sin que nadie se entere.
4. **La lectura del entorno se inyecta.** En `edition = "2024"`, `std::env::set_var` es `unsafe`: un test que mutase el entorno sería código inseguro y no reproducible en paralelo.
5. **No se crean los seis directorios vacíos del §7.** Solo `api/`; cada uno aparece con el change que le da contenido.
6. **No se usa el crate `secrecy`** — no está en la caché local de Cargo. Se envuelve `zeroize`.
7. **`/api/v1/health` se añade al SRS**, que no lo declara, porque el §8 despliega con `restart: always`.

## Límites conocidos que se aceptan

- **`DATABASE_URL` se guarda como `String` y el volcado la imprime.** Hoy es una ruta SQLite local sin credenciales, así que no hay nada que redactar. El vector no es solo el volcado: el mismo DSN aparece en el `Debug` derivado de `Config` y —lo que importa más— dentro del texto de cualquier error de biblioteca que `AppError::Other` manda al log por mandato de la especificación. Con un `String` plano, `zeroize` no protege nada de eso. **Cuándo cambia**: en el change `db-schema`, que es el que introduce `sqlx`; ahí `DATABASE_URL` pasa a `SecretString`, el DSN se excluye de la causa registrada y se rechaza el valor vacío. Queda como criterio de aceptación de ese change, no como promesa sin fecha.

- **`zeroize` limpia las copias de Kesto, no el bloque de entorno del proceso.** Devolver un código de salida en vez de matar el proceso deja que los `SecretString` se borren al caer, que es su contrato. Lo que no se puede evitar es que `KESTO_MASTER_KEY` siga siendo legible en `/proc/<pid>/environ` mientras el proceso viva: esto protege de un volcado de memoria o de un log, no de quien puede leer el entorno del proceso.

- **El log publica a propósito lo que la salud tiene prohibido publicar.** El volcado de arranque dice la dirección de escucha, la cadena de base de datos, el emisor y la URL de retorno; la salud no publica ni el puerto, ni el DSN, ni el emisor. Es deliberado —el log es para quien administra el contenedor, la salud es para cualquiera que la consulte— y conviene no «arreglarlo» unificándolos.

- **No hay traza de ninguna petición.** `tower-http` se retiró porque el spec no la pide y una dependencia sin consumidor es superficie regalada. Cuando llegue, con el primer consumidor que la necesite, el span NO SHALL incluir la cadena de consulta: el callback de OIDC lleva `?code=` y `?state=`, y un `TraceLayer` por defecto registra la URI completa. Es una fuga de credencial por el log, no un detalle de formato.

- **Un `OIDC_REDIRECT_URL` suelto se ignora en silencio.** Los tres desenlaces se definen sobre las tres variables de la autenticación: con ninguna de ellas presentes el modo es `Disabled` y la URL de retorno no se usa. Quien configure solo el retorno creerá que encendió algo. Queda dicho en `AGENTS.md`, que es donde lo va a leer quien despliega.

- **El apagado ordenado no se prueba en automático.** Se prueba el mecanismo (que la función de arranque devuelve al resolverse la señal); la señal real se comprueba a mano.

- **La salud es vida, no disponibilidad.** No hay base de datos a la que preguntar; la comprobación llega con `db-schema`.

## Fuera de este plan

`auth-capability` y `db-schema` son los otros dos changes. Este plan no los adelanta ni deja ganchos para ellos más allá del contrato de configuración: el router **no** se parte en público y protegido, porque quien necesita esa estructura es el middleware de autenticación y llega con él.
