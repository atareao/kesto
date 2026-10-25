## 0. Preparación

- [ ] 0.1 Leer el contrato completo de la capability nueva `api-resources` (11 requisitos, 33 escenarios) y los tres deltas (`api-shell` 2 requisitos / 7 escenarios, `db` 1 / 3, `secrets` 2 / 5) junto con el §5 de `docs/DEFINITION.md`, y **corregir el recuento de endpoints**: son **diez**, no nueve → esperado: los diez listados y los tres diferidos anotados con su change (`POST /api/v1/jobs/:id/run` y `GET /api/v1/events` → el change del **motor**; `GET /api/v1/snapshots?repository_id=<id>` → el change de **Rustic**), más la nota de que `AGENTS.md`, `plans/PLAN-003-COMPLETED.md` y `docs/FEATURE-003-db-schema.md` dicen «nueve» y hay que corregirlos en la fase 5.
- [ ] 0.2 Comprobar con `cargo tree --offline` que este change **no** necesita ninguna dependencia nueva → esperado: `axum` 0.8.9 con `json` y `query` por defecto (este `Cargo.toml` no declara `default-features = false` para `axum`), `serde` con `derive` ya presente y `tower` (`ServiceExt::oneshot`) con `http-body-util` ya en `[dev-dependencies]` y en uso por `src/test_support.rs`, con `Cargo.toml` y `Cargo.lock` **sin tocar**; si hiciera falta una dependencia, **parar y reportarlo**.
- [ ] 0.3 Decidir y medir el `Deserialize` **manual** de `SecretString` y la **ausencia** deliberada de `Serialize` → esperado: implementación a mano vía `SecretString::new` (para que el envoltorio no se pueda saltar) y **sin** encender la feature `serde` de `zeroize` 1.9.1, que unificaría **quince** paquetes del árbol (`crypto-bigint`, `curve25519-dalek`, `der`, `ed25519-dalek`, `elliptic-curve`, `generic-array`, `jsonwebtoken`, `kesto`, `num-bigint-dig`, `rsa`, `rustls`, `rustls-pki-types`, `sec1` y, por encima, `p256`, `p384`, `ecdsa`, `primeorder`), más la comprobación de que **no** se implementa `Serialize` para `SecretString` (es lo que hace que una credencial en una respuesta **no compile**).
- [ ] 0.4 Decidir la forma del **estado de base** dentro de `AppState` —pool, `Clave` y las cuatro capas (`Repositorios`, `Targets`, `Tareas`, `Ejecuciones`, cada una con `nuevo(pool: SqlitePool, clave: Arc<Clave>)`), construido una vez y **no** opcional— y **medir** los sitios a ajustar con `grep` sobre `src/` → esperado: **16** constructores de `AppState` (**11** en pruebas: `src/api/health.rs` 6 y `src/test_support.rs` 5; **5** fuera: `src/state.rs` 4 y `src/main.rs` 1), **7** ayudantes del arnés que pasan a ser asíncronos (`estado_desarrollo`, `estado_oidc`, `estado_oidc_con_emisor`, `router_desarrollo`, `router_oidc`, `router_oidc_con_emisor`, `auth_oidc`) con **61** puntos de llamada en total (`router_oidc_con_emisor` 26, `router_oidc` 16, `estado_desarrollo` 5, `auth_oidc` 5, `estado_oidc_con_emisor` 5, `estado_oidc` 3, `router_desarrollo` 1) y escrito por qué **no** un `Option`.
- [ ] 0.5 Comprobar que no queda ninguna vía para construir un `AppState` sin base —lo impone el typecheck— y que `AppState::new(config) -> Result<Self, ConfigError>` sigue **sin ser asíncrona** → esperado: escrito por qué la base llega **ya abierta** desde fuera (quien abre el pool y aplica las migraciones es `src/main.rs`, no el constructor del estado), y por qué `AppState` gana un argumento en vez de un `await`.
- [ ] 0.6 Fijar el **formato del cable** —identificadores como texto con guiones (`uuid` está declarado con `features = ["v4"]`, **sin `serde`**), fechas como cadena RFC3339 formateada por nosotros con `OffsetDateTime::format(&Rfc3339)`, nombres de campo los del §3 en `snake_case`— y **medir el literal** que produce `Rfc3339` para UTC → esperado: `2026-10-25T12:34:56Z`, con `Z` en lugar de `+00:00` y **sin** fracción de segundo (la fracción solo se escribe si los nanosegundos no son cero), que es lo que la prueba fija.
- [ ] 0.7 Localizar el `fixture` `base_con_clave()` en `src/test_support.rs` y comprobar que devuelve lo que hace falta (directorio temporal, pool ya migrado y `Clave` en un `Arc`) → esperado: `(DirectorioDePrueba, SqlitePool, Arc<Clave>)` anotado como la forma exacta del estado de base de prueba.

## 1. RED (pruebas que fallan primero)

Todas las pruebas de esta fase van contra el **router de verdad**, con una base temporal migrada; las que afirman una propiedad del **fichero** o de una **columna** comprueban además con **SQL directo**.

- [x] 1.1 Escribir la prueba de que el estado compartido por el router lleva el pool y las cuatro capas (`Repositorios`, `Targets`, `Tareas`, `Ejecuciones`) → cubre `api-resources`, requisito 1 (estado con la capa de acceso); esperado: roja por «no implementado», porque ni el estado ni las rutas existen todavía.
- [x] 1.2 Escribir la prueba de que dos peticiones seguidas al **mismo** router ven lo mismo —una escribe y la otra lee— → cubre requisito 1, escenario de estado compartido entre peticiones; esperado: roja antes de la implementación y verde sin volver a construir el estado.
- [x] 1.3 Escribir la prueba de que la escritura hecha por la API fue a **la** base del proceso: la fila se relee con **SQL directo** sobre el **mismo** fichero → cubre requisito 1 y el requisito del delta `db`; esperado: la fila aparece en el fichero con el que se abrió el estado.
- [ ] 1.4 Escribir las pruebas de las cinco rutas de `/targets` (`GET` listado, `POST` alta, `GET /:id`, `PUT /:id`, `DELETE /:id`) → cubre requisito 2, escenario de la superficie de `/targets`.
- [ ] 1.5 Escribir las pruebas de las cinco rutas de `/repositories` con la misma forma → cubre requisito 2, escenario de la superficie de `/repositories`.
- [ ] 1.6 Escribir las pruebas de las cinco rutas de `/jobs` con la misma forma → cubre requisito 2, escenario de la superficie de `/jobs`.
- [ ] 1.7 Escribir la prueba de que el listado contesta `200` con un array en `data` → cubre requisito 4, escenario del listado con la envoltura; esperado: `200` y `data` es un array.
- [ ] 1.8 Escribir la prueba de que una tabla **vacía** contesta `200` con array vacío —**no** `404` y **no** `data` nulo— → cubre el escenario de listado vacío; esperado: `200` con `data: []`.
- [ ] 1.9 Escribir la prueba de que el alta contesta `201` y de que el `id` devuelto sirve para el `GET /:id` siguiente → cubre requisito 4, escenario de alta y lectura encadenadas.
- [ ] 1.10 Escribir la prueba de que `GET /:id` con un identificador inexistente contesta `404` con la envoltura → cubre requisito 4, escenario de recurso inexistente.
- [ ] 1.11 Escribir la prueba de que `DELETE` contesta `200` con `data` nulo —**no** `204`— y de que la lectura posterior contesta `404` → cubre requisito 4, escenario de borrado.
- [ ] 1.12 Escribir la prueba de que `PUT /:id` sobre un identificador inexistente contesta `404` → cubre requisito 4, escenario de actualización de recurso inexistente.
- [ ] 1.13 Escribir la prueba de que el alta devuelve los campos con los nombres del §3 (`access_key_id`, `keep_daily`, `cron_expression`…) y en `snake_case` → cubre requisito 3, escenario de nombres del cable.
- [ ] 1.14 Escribir la prueba de que el identificador llega como **cadena con guiones** y de que `Uuid::parse_str` lo acepta → cubre requisito 3, escenario de representación del identificador; esperado: la cadena es la forma canónica con guiones.
- [ ] 1.15 Escribir la prueba de que una fecha publicada se relee con `OffsetDateTime::parse(…, &Rfc3339)`, da el **mismo instante**, es **cadena** —no una secuencia de números— y su literal es `…T…:…Z` **sin** fracción → cubre requisito 10, escenario de las fechas del cable; esperado: el literal exacto `2026-10-25T12:34:56Z`.
- [ ] 1.16 Escribir la prueba de que el alta y su lectura **no** traen la credencial y **sí** el booleano de si está configurada → cubre requisito 8, escenario del secreto nunca devuelto.
- [ ] 1.17 Escribir la prueba de que el `Debug` del tipo del cuerpo no publica la credencial y **sí** dice que está redactada → cubre requisito 8 y el delta `secrets`; esperado: aparece `REDACTED = "<redactado>"` y no el valor.
- [ ] 1.18 Escribir la prueba de que el valor solo sale por el método explícito (`expose`) → cubre requisito 8 y el delta `secrets`, escenario del acceso explícito.
- [ ] 1.19 Escribir la prueba de que **ningún** campo de secreto aparece en el cuerpo de **ninguna** respuesta —alta, lectura, actualización y borrado de los tres recursos— → cubre requisitos 8 + `secrets`, escenario de barrido de respuestas.
- [ ] 1.20 Escribir la prueba de que la credencial que entró **por la API** no aparece en claro en los bytes del `.db` ni en `-wal`/`-shm`, con la búsqueda **validada** antes con un valor que no es secreto → cubre el delta `secrets`, escenario de cifrado en reposo; esperado: la búsqueda encuentra el control y **no** encuentra la credencial.
- [ ] 1.21 Escribir la prueba con `RUST_LOG=trace` de que el alta por la API no deja la credencial en el registro capturado y **sí** deja la traza de que se atendió → cubre el delta `secrets`, escenario del registro.
- [ ] 1.22 Escribir la prueba con `RUST_LOG=trace` de que un alta que **no** valida no pone el cuerpo en el registro → cubre el delta `secrets`, escenario del rechazo sin volcado; esperado: el registro no contiene el cuerpo enviado.
- [x] 1.23 Escribir la prueba de que un cuerpo que no es JSON contesta `400` con `{status,message}` y **no** texto plano → cubre el delta `api-shell`, escenario del cuerpo ilegible.
- [x] 1.24 Escribir la prueba de que un campo con el **tipo equivocado** que lleva dentro una credencial distinguible contesta `400` y la credencial no aparece **ni** en el cuerpo **ni** en el registro capturado → cubre el delta `api-shell`, escenario del tipo equivocado.
- [ ] 1.25 Escribir la prueba de que `?limit=abc` contesta `400` con la envoltura → cubre el delta `api-shell`, escenario del parámetro inválido.
- [ ] 1.26 Escribir la prueba de que un target `directory` con `container_name` contesta `400` nombrando el campo y **sin** dejar fila (comprobado con SQL directo) → cubre requisito 6, escenario de coherencia del target.
- [ ] 1.27 Escribir la prueba de que un target `mariadb` sin `db_password` contesta `400` nombrando el campo → cubre requisito 6, escenario del secreto obligatorio.
- [ ] 1.28 Escribir la prueba de que un `db_name` ausente queda `NULL` **de verdad** (`IS NULL`, no `''`) → cubre requisito 6 y el delta `db`, escenario del `db_name` nulo; esperado: la columna es nula, verificada con SQL directo.
- [ ] 1.29 Escribir la prueba de que una tarea con `target_id` inexistente contesta `400` nombrando el campo y **sin** fila → cubre requisito 7, escenario de la referencia inexistente.
- [ ] 1.30 Escribir la prueba de que un campo de más en el cuerpo contesta `400` → cubre requisito 7, escenario del campo desconocido.
- [ ] 1.31 Escribir la prueba de que el `400` nombra el **campo** y **no** filtra el **valor** → cubre requisitos 6 y 7, escenario del mensaje sin eco del valor.
- [ ] 1.32 Escribir la prueba de que un `PUT` **sin** `passphrase` conserva la almacenada **intacta** —comparada con la que había— y cambia el resto → cubre requisito 8, escenario de la edición parcial.
- [ ] 1.33 Escribir la prueba de que un `PUT` con `passphrase` **vacía** contesta `400` nombrando el campo y **sin** cambiar la almacenada → cubre requisito 8, escenario del secreto vacío.
- [x] 1.34 Escribir la prueba de que un secreto vacío llegado a la capa de acceso se traduce a `400` con el campo nombrado, **no** a `500` → cubre el delta `api-shell` (`DbError::SecretoVacio` → `AppError`); esperado: `400`, no el `500` genérico de `AppError::Database`.
- [x] 1.35 Escribir la prueba de que una **clave foránea violada** se traduce a `409` con `status` y `message`, **sin** el texto de la sentencia, **sin** el nombre de la restricción, **sin** atribuir el fallo a un campo concreto, y con el motivo en el registro → cubre el delta `api-shell`, escenario del conflicto de integridad.
- [x] 1.36 Escribir la prueba de que una consulta que falla por otra causa sigue en `500` con el mensaje genérico (`INTERNAL_MESSAGE`) → cubre el delta `api-shell`, escenario del fallo interno.
- [x] 1.37 Escribir la prueba de que el `4xx` no filtra ni una credencial ni la ruta del fichero de la base → cubre el delta `api-shell`, escenario del error que no revela.
- [ ] 1.38 Escribir la prueba de que el listado de `/executions` sin filtros devuelve las ejecuciones **con sus métricas** → cubre requisito 9, escenario del listado de ejecuciones.
- [ ] 1.39 Escribir la prueba de que los filtros `job_id` y `status` acotan el listado → cubre requisito 9, escenario de los filtros.
- [ ] 1.40 Escribir la prueba de que `limit` acota y `offset` avanza → cubre requisito 9, escenario de la paginación.
- [ ] 1.41 Escribir la prueba de que un `limit` fuera de rango o no numérico contesta `400` y **no** se recorta en silencio → cubre requisito 9, escenario del límite inválido.
- [ ] 1.42 Escribir la prueba de que dos llamadas seguidas dan el **mismo orden** con filas que empatan en `started_at` → cubre requisito 9, escenario del orden estable.
- [ ] 1.43 Escribir la prueba de que en modo `Oidc` y **sin** cookie cada ruta nueva contesta `401` con la envoltura → cubre requisito 11, escenario de la partición protegida.
- [ ] 1.44 Escribir la prueba de que con la autenticación **apagada** las rutas nuevas contestan sin sesión, comprobando que **están montadas** —hoy el router de ese modo solo devuelve `public` y la mitad de negocio no se construye— → cubre requisito 11 y el delta `api-shell`, escenario del modo de desarrollo; esperado: respuesta de negocio, no `404`.
- [ ] 1.45 Escribir la prueba de que un método no declarado sobre una ruta nueva y **sin** sesión contesta `401`, **no** `405` → cubre requisito 11, escenario del orden de la puerta; esperado: la sesión se exige antes que el método.
- [ ] 1.46 Escribir la prueba de que un identificador de ruta que **no** es un UUID contesta `400` con el motivo —se pide `GET /api/v1/targets/no-es-un-uuid` y lo mismo sobre `/repositories` y `/jobs`—, **no** `404` y **no** `500` → cubre requisito 5, escenario del identificador mal formado; esperado: los tres recursos contestan `400` con `status` y `message`.
- [ ] 1.47 Escribir la prueba de que las fechas de una fila de ejecución se leen por el listado y sus dos campos son **cadenas** RFC3339 que se releen con `OffsetDateTime::parse(…, &Rfc3339)` dando el **mismo instante**, con el literal acabado en `Z` y **sin** fracción de segundo → cubre requisito 9, escenario de las fechas de una fila de ejecución; esperado: las dos son cadenas y ninguna es una secuencia de números.
- [x] 1.48 Medir el rojo y **pegar el número exacto**: las pruebas nuevas en rojo por «no implementado» y las **156** actuales (155 del binario + 1 de integración) en verde → esperado: el número pegado en el change; si al ajustar el arnés hay que tocar una aserción existente, **no** es RED: **parar y reportarlo**.

## 2. GREEN (implementación mínima)

- [x] 2.1 Implementar el `Deserialize` **manual** de `SecretString` en `src/secret.rs` vía `SecretString::new`, y **no** implementar `Serialize` → esperado: el `PUT`/`POST` acepta una credencial y una respuesta nunca puede llevarla (no compila).
- [x] 2.2 Añadir el estado de base a `src/state.rs`: pool, `Clave` y las cuatro capas (`Repositorios`, `Targets`, `Tareas`, `Ejecuciones`) construidas una vez y **no** opcionales → esperado: un solo sitio donde se construyen, sin `Option`.
- [x] 2.3 Dar a `AppState::new` el argumento del estado de base manteniéndolo **síncrono** → esperado: `Result<Self, ConfigError>` sin `async`.
- [x] 2.4 Ajustar `src/main.rs` para construir el estado **después** de abrir el pool y aplicar las migraciones, con el fallo de migración como error de arranque → esperado: una sola base abierta por proceso y el fallo nombrando la causa.
- [x] 2.5 Llevar el `AppState` con base a `src/test_support.rs`, hacer **asíncronos** los **7** ayudantes y añadir el `.await` a sus **61** llamantes → esperado: el arnés compila sin cambiar ninguna aserción.
- [x] 2.6 Ajustar los **16** constructores de `AppState` (`src/api/health.rs` 6, `src/test_support.rs` 5, `src/state.rs` 4, `src/main.rs` 1) **sin debilitar ninguna aserción** → esperado: las 156 anteriores siguen verdes; si una se pone roja, se arregla el **código**, no la prueba.
- [ ] 2.7 Escribir en `src/api/` los tipos de petición y respuesta de los tres recursos —identificadores `String` con guiones, fechas `String` RFC3339, nombres del §3— → esperado: `Serialize` en las respuestas y `Deserialize` con rechazo de campos desconocidos en las peticiones.
- [ ] 2.8 Implementar el módulo de `/executions` con sus filtros (`job_id`, `status`) y su paginación (`limit`, `offset`) → esperado: consulta parametrizada, orden estable y sin recorte silencioso.
- [ ] 2.9 Declarar las rutas nuevas en la mitad de negocio, sin excepciones a la lista blanca → esperado: la lista blanca del router **no** crece y el orden de los dos fallbacks no se invierte.
- [ ] 2.10 Montar las quince rutas nuevas **públicas y sin capa** cuando la autenticación está apagada → esperado: en modo `Disabled` se alcanzan todas, y en modo `Oidc` quedan detrás de la puerta.
- [ ] 2.11 Traducir el rechazo del cuerpo y de los parámetros a `400` con **mensaje propio**, sin el texto del intérprete en el cuerpo **ni** en el registro → esperado: `{status,message}` siempre, nunca texto plano.
- [x] 2.12 Traducir en `src/error.rs` `SecretoVacio` → `400` con el campo nombrado y la violación de clave foránea → `409` con `status` y `message` → esperado: el resto de consultas sigue en `500` con `INTERNAL_MESSAGE`, y el motivo del `409` queda en el registro.
- [ ] 2.13 Implementar la validación: coherencia del target (`directory` sin `container_name`, `mariadb` sin `db_password`), comprobación de referencias antes de escribir, y rechazo de campos desconocidos → esperado: `400` nombrando el campo y **sin** dejar fila.
- [ ] 2.14 Implementar la conservación del secreto ausente en el `PUT` y el rechazo del vacío → esperado: el `PUT` sin `passphrase` deja la almacenada intacta y el `PUT` con `passphrase` vacía contesta `400`.
- [ ] 2.15 Pasar las cuatro puertas —`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked` y `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`— → esperado: **156 + las nuevas**, todas verdes, y las cuatro limpias.

## 3. REFACTOR

- [ ] 3.1 Dejar `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios tras el refactor → evidencia: la salida pegada en el change.
- [ ] 3.2 Comprobar con `grep` sobre `src/` que **ninguna** consulta se construye concatenando y que **ningún** cuerpo de petición se registra → evidencia: el `grep` pegado y los `bind`/`query` parametrizados.
- [ ] 3.3 Comprobar que **ninguna** credencial llega al registro —ni el cuerpo en crudo, ni un `serde_json::Value`, ni el texto del rechazo del intérprete— → evidencia: la captura de registro con `RUST_LOG=trace` pegada.
- [ ] 3.4 Revisar el `Debug` de los tipos del cable, **tipo por tipo**, comprobando que no publica credenciales → evidencia: la lista de tipos revisados.
- [ ] 3.5 Comprobar que no hay `unwrap`/`expect`/`panic!` en rutas de producción → evidencia: el `grep` sobre `src/` sin hallazgos fuera de pruebas.
- [ ] 3.6 Comprobar que ninguna prueba declara `unsafe` ni muta el entorno del proceso, y que las capturas de registro usan el buffer **por hilo** (`src/test_support.rs`, nunca `set_default`/`with_default`) → evidencia: el `grep` y las pruebas que capturan registro.
- [ ] 3.7 Comprobar que la lista blanca del router **no** creció con excepciones y que el orden de los dos fallbacks **no** se invirtió → evidencia: el `git diff` de `src/api/`.
- [ ] 3.8 Comprobar que no hay dependencias nuevas y que `Cargo.toml` y `Cargo.lock` siguen **sin tocar** → evidencia: `git diff --stat` con ambos ficheros fuera del diff.
- [ ] 3.9 Comprobar que `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` pasa sin enlaces rotos → evidencia: la salida pegada.
- [ ] 3.10 Comprobar que el layout sigue el §7 (`src/api/` con sus módulos, sin directorios nuevos en la raíz) → evidencia: `git status --short` sin directorios nuevos.

## 4. Verificación manual (operador)

Todas contra el binario de verdad, con `KESTO_MASTER_KEY`, un `DATABASE_URL` temporal, `BIND_ADDRESS` en un puerto libre y `RUST_LOG=info`.

- [ ] 4.1 Arrancar el binario y dar de alta con `curl` un repositorio, un target de base de datos, uno de directorio y una tarea → esperado: `201` en los cuatro, con la envoltura.
- [ ] 4.2 Leer los cuatro con `curl` y comprobar los códigos y que **ningún** cuerpo trae una credencial → esperado: `200` y ningún valor secreto en la salida.
- [ ] 4.3 Comprobar con `sqlite3` que la columna del secreto guarda el sobre `v1:…` con sus **tres** partes → esperado: `v1:<nonce>:<cifrado>` con el nonce distinto por valor.
- [ ] 4.4 Comprobar con `sqlite3` que el `db_name` del target de directorio es `NULL` y el del target de base es el informado → esperado: `IS NULL` para el primero.
- [ ] 4.5 Actualizar un repositorio **sin** mandar la `passphrase` y comprobar con `sqlite3` que la almacenada **no** cambió → esperado: el sobre idéntico al anterior.
- [ ] 4.6 Comprobar que las credenciales buscadas **literalmente** en los bytes del fichero no aparecen, **validando la búsqueda** antes con un valor que no es secreto → esperado: el control se encuentra y la credencial no.
- [ ] 4.7 Pedir un `limit` fuera de rango y ver el `400` con la envoltura → esperado: `{status,message}` y ni una traza del intérprete.
- [ ] 4.8 Borrar el repositorio y comprobar que sus tareas y sus ejecuciones **desaparecen** con él (`ON DELETE CASCADE`) y que el target **sigue** → esperado: `sqlite3` confirma el borrado en cascada y la supervivencia del target.
- [ ] 4.9 En modo `Oidc`, comprobar el `401` sin cookie en una ruta nueva y el `200` con la salud → esperado: la partición intacta y la salud pública.
- [ ] 4.10 Comprobar que arrancar **no** abre una segunda base y que el fichero sigue siendo el mismo → esperado: un único `kesto.db` con su `-wal`, sin ficheros nuevos.

## 5. Cierre

- [ ] 5.1 Ejecutar `just check` → esperado: en verde (formato, clippy y tests).
- [ ] 5.2 Ejecutar `just spec-validate api-endpoints` → esperado: válido en modo estricto.
- [ ] 5.3 Corregir el recuento a **diez** endpoints en `AGENTS.md`, `plans/PLAN-003-COMPLETED.md` y `docs/FEATURE-003-db-schema.md` → esperado: los tres documentos dicen diez y nombran los tres diferidos con su change.
- [ ] 5.4 Escribir `docs/FEATURE-004-api-endpoints.md` con lo que **realmente** quedó construido (contratos, endpoints, decisiones y deuda pendiente) → esperado: el documento emparejado con su plan.
- [ ] 5.5 Rellenar el `Purpose` de `openspec/specs/api-resources/spec.md` → esperado: la capability nueva **no** se queda en `TBD`.
- [ ] 5.6 Actualizar `AGENTS.md` con el estado real y los módulos que siguen vacíos → esperado: `api` deja de estar a medias y se listan los que faltan (`docker`, `engine`, `rustic`, `scheduler`).
- [ ] 5.7 Ejecutar `just spec-archive api-endpoints` → esperado: el delta fusionado en `openspec/specs/` y el change en `changes/archive/`.
- [ ] 5.8 Renombrar el plan con `just plan-done 4` → esperado: `plans/PLAN-004-COMPLETED.md`.
- [ ] 5.9 Abrir el PR de `feature/api-endpoints` a `development` con `just gf-pr` → esperado: PR abierto contra `development`, y **ninguna** release a `main`.
