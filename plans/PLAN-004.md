# PLAN-004 — `api-endpoints`

> **Estado:** en curso (change proposal aprobado el 2026-10-25; implementación TDD en marcha).
> **Rama:** `feature/api-endpoints`, desde `development`.
> **Change de OpenSpec:** `openspec/changes/api-endpoints/` — **16 requisitos y 48 escenarios** (`api-resources` 11/33, capability **nueva**; `api-shell` 2/7, `db` 1/3 y `secrets` 2/5, que crecen), `openspec validate api-endpoints --strict` válido. `tasks.md`: **99 tareas** (7 de preparación, 48 de RED, 15 de GREEN, 10 de REFACTOR, 10 de operador y 9 de cierre).
> **Especificación de origen:** `docs/DEFINITION.md` §5 (endpoints y el evento SSE) y §6 (lo que el panel necesita). El motor del §4, Docker y Rustic **no** entran.
> **Cambio anterior:** `db-schema` (`PLAN-003-COMPLETED.md`, `docs/FEATURE-003-db-schema.md`, 156 tests, fusionado en `development` como `203638c`).

## Objetivo

Que el orquestador deje de ser una biblioteca. Hoy la capa de acceso del §3 está escrita, probada y auditada —con sus secretos cifrados— y **nadie la alcanza desde el proceso**: `actualizar` y `borrar` de las cuatro tablas solo los ejercitan las pruebas, y `/api/v1/*` tiene la salud y el flujo de entrada y nada más. Al terminar, `/api/v1` sirve los recursos del §3 con sus códigos, sus DTO, su validación y su paginación, y el pool llega a los manejadores por el estado compartido.

Es además el change que estrena el **primer consumidor** de la capa de acceso, y cierra **tres deudas declaradas** en `docs/FEATURE-003-db-schema.md`: el pool sin traspasar al `AppState`, la violación de clave foránea que colapsa en `500` y el `SecretoVacio` que también colapsa en `500`.

Y corrige **un error de recuento**: el §5 declara **diez** endpoints, no nueve. Lo dicen mal `AGENTS.md`, `plans/PLAN-003-COMPLETED.md` y `docs/FEATURE-003-db-schema.md`.

## Alcance

**Dentro:**

1. El **estado de base** dentro del `AppState`: el pool, la `Clave` de cifrado y las cuatro capas de acceso, construido una vez y **no opcional** —un Kesto sin base no es un estado válido—, con la base **ya abierta** para que `AppState::new` siga sin ser asíncrona.
2. La **superficie de los tres recursos**: `GET` y `POST` de `/targets`, `/repositories` y `/jobs` —los seis endpoints del §5— más `GET /:id`, `PUT /:id` y `DELETE /:id` en cada uno, declarados por escrito **como añadido al SRS**, con el precedente de los cuatro índices de `db-schema`. Quince rutas.
3. Los **tipos del cable** propios de la API, nunca las entidades: identificadores como texto con guiones, fechas como cadena RFC3339 formateada por nosotros, y los nombres de los campos los del §3.
4. La **validación** que hoy no existe en ningún sitio: coherencia del target con su tipo, referencias que existen antes de escribir, campos desconocidos rechazados y secretos vacíos rechazados nombrando el campo.
5. El **listado de ejecuciones** con filtros, paginación acotada (`limit` 50 por defecto, **200 de tope**) y orden estable `started_at DESC, id`.
6. La **traducción de los errores de base que causó la petición**: `SecretoVacio` → `400` nombrando el campo, clave foránea violada → `409`, y **todo lo demás sigue en `500`** con el mensaje genérico.
7. El **rechazo del cuerpo y de los parámetros** traducido a `400` con la envoltura y con mensaje **propio**, sin el texto del intérprete ni en el cuerpo ni en el registro.

**Fuera:** `POST /api/v1/jobs/:id/run` y `GET /api/v1/events` (el **motor**: sin nada que ejecute una copia, una ejecución `pending` es prometer algo que no ocurre; y sin productor, el flujo SSE abriría mudo y sus pruebas fabricarían un publicador que en producción no existe); `GET /api/v1/snapshots` (**Rustic**: hay que listar snapshots en S3 y `src/rustic/` está vacío); motor, planificador, Docker, Rustic, `ui/` y `Dockerfile`; agregados o `/stats`; filtro por repositorio en `/executions`; `PATCH`; ordenación configurable, `ETag`, caché y versionado de la API.

## Módulos

El change se reparte en tres módulos entregables, cada uno con su ciclo RED → GREEN → auditoría independiente → correcciones:

| # | Módulo | Contenido | Ficheros |
|---|---|---|---|
| 1 | Estado, tipos del cable y errores | El `Deserialize` manual de `SecretString`, el estado de base en el `AppState`, el arnés ajustado y la traducción de los errores de base y del rechazo del cuerpo | `src/secret.rs`, `src/state.rs`, `src/main.rs`, `src/test_support.rs`, `src/error.rs` |
| 2 | Los tres recursos | Los DTO de petición y respuesta, las quince rutas, la validación y la conservación del secreto en la edición | `src/api/mod.rs`, `src/api/{targets,repositories,jobs}.rs` |
| 3 | El listado de ejecuciones | Los filtros, la paginación acotada y el orden estable | `src/api/executions.rs` |

El **orden de entrega** es 1 → 2 → 3. El 1 va primero porque **nada se puede probar sin él**: los manejadores reciben las capas por el estado, así que sin el estado no hay una sola ruta que ejecutar, y la traducción de los errores es lo que permite contestar `400` y `409` desde cualquiera de los tres recursos. El 3 va último aunque sea independiente del 2: los dos declaran rutas en `src/api/mod.rs`, y escribirlos a la vez sería dos escritores sobre el mismo fichero.

## Decisiones tomadas

### Alcance: los siete entregables hoy, más tres verbos declarados como añadido

`GET /:id`, `PUT /:id` y `DELETE /:id` **no** están en el §5 y se declaran como añadido, igual que `db-schema` declaró sus cuatro índices en vez de colarlos. El motivo es que la capa de acceso **ya implementa** `leer`, `actualizar` y `borrar` y hoy solo las pruebas las ejercitan: sin verbos de edición, un repositorio con una errata en la clave de acceso no se puede corregir, y el §6 pide «crear/editar tarea». El §5 tampoco tiene `GET /:id`, y sin él la edición se hace a ciegas.

### El SSE se difiere al motor

Sin un productor de eventos el flujo abriría y se quedaría mudo. Va con el motor, que trae a la vez el transporte y su primer evento real. Lo que **sí** queda escrito ahora es el esquema del evento del §5 —`event: execution_progress` con su payload literal—, porque `AGENTS.md` manda no inventar otro.

### Los códigos del recurso

`POST` contesta **`201`** con el recurso creado, y su `id` lo genera la capa de acceso: quien llama no lo informa. `GET`, `GET /:id`, `PUT` y `DELETE` contestan **`200`**; `GET /:id`, `PUT` y `DELETE` sobre una fila que no existe, **`404`**. Y **`DELETE` contesta `200` con `data` nulo, no `204`**: el contrato de `api-shell` exige que toda respuesta de la API sea JSON con `status` y `message`, y un `204` no tiene cuerpo —sería la única respuesta capaz de romper la envoltura que la API se impone—.

### `409` para la clave foránea, y no `400`

La petición está bien formada; lo que ocurre es que contradice una relación que existe. Un `400` diría que está mal escrita, y no lo está. Y el `409` es **una red de seguridad, no el camino normal**: el manejador valida antes de escribir y da un mensaje preciso, así que solo se llega a él por una carrera —otra petición borró la fila referenciada entre la comprobación y el `INSERT`—.

### Los secretos no vuelven, y el que no se envía se conserva

La respuesta de un repositorio **no** trae `secret_access_key` ni `passphrase`, ni la de un target `db_password`: trae un **booleano** por credencial diciendo si está configurada. El panel no necesita el valor para pintar el formulario, y devolverlo convertiría cada `GET` en una fuga. En un `PUT`, un secreto **ausente** conserva el almacenado —el manejador lee la fila y lo reutiliza— y uno **vacío** se rechaza con `400` nombrando el campo: sin lo primero, un formulario que no rellena la contraseña la sustituiría por nada; sin lo segundo, ese mismo formulario recibiría el error de la base.

### El `Deserialize` de `SecretString` es manual, y `Serialize` no se implementa

`SecretString` guarda su valor en un `Zeroizing<String>`, y el `Deserialize` de `Zeroizing` vive tras la feature `serde` de `zeroize` —dependencia **opcional**, hoy apagada—. Encenderla resolvería el `derive`, pero unificaría esa feature para **todo** el grafo que usa `zeroize` —`crypto-bigint`, `curve25519-dalek`, `der`, `ed25519-dalek`, `elliptic-curve`, `generic-array`, `jsonwebtoken`, `num-bigint-dig`, `rsa`, `rustls`, `rustls-pki-types`, `sec1`, `p256`, `p384`, `ecdsa`, `primeorder`— por una necesidad local. La implementación manual pasa por `SecretString::new` y deja la invariante donde se audita. Y **`Serialize` no se implementa**: es lo que hace que una respuesta que llevara una credencial **no compile**, así que la fuga deja de depender de que nadie se equivoque.

### El formato del cable

Los identificadores viajan como **texto** con guiones (`uuid` está declarado sin `serde`, así que un `Uuid` no se puede serializar y la trampa del BLOB del §3 no puede reaparecer en el cable), las fechas como **cadena RFC3339** formateada por nosotros, y los nombres de los campos son **los del §3** en `snake_case`. No se enciende la feature `serde-human-readable` de `time`: formatear por nuestra cuenta deja el formato del cable como parte del contrato en vez de como consecuencia de una feature, y hay una prueba que fija el literal.

### Sin dependencias nuevas

Medido: `axum` 0.8.9 trae `json` y `query` en sus features por defecto y este `Cargo.toml` no las desactiva; `serde` con `derive` ya está; y `tower` y `http-body-util` ya están en `[dev-dependencies]`. `Cargo.toml` y `Cargo.lock` quedan **sin tocar**, y la tarea 0.2 lo verifica con `cargo tree --offline`: si hiciera falta una dependencia, se para y se reporta antes de seguir.

## Trampas encontradas y candadeadas

- **`SecretString` no implementa `Deserialize`**, y su `Zeroizing` interno tampoco sin encender una feature de `zeroize` que se unificaría en dieciséis paquetes del árbol. La implementación es **manual** y pasa por `SecretString::new`.
- **El `Serialize` de `time` no produce una cadena** en este crate: `serde-human-readable` es una feature **aparte** en `time` 0.3.55 y no está activada. Se formatea con `OffsetDateTime::format(&Rfc3339)`, y el literal medido para UTC es `2026-10-25T12:34:56Z` —emite `Z` y **omite la fracción** cuando los nanosegundos son cero—, así que la prueba fija ese literal en vez de suponerlo.
- **El texto del rechazo de `serde` puede ecoar el valor del campo**: `{"keep_daily": "AKIA…"}` produce un mensaje con la credencial dentro. Por eso el rechazo se traduce a un mensaje **propio** y el texto del intérprete no entra ni en el cuerpo **ni** en el registro.
- **Con la autenticación apagada el router no monta la mitad de negocio**: hoy devuelve solo la mitad pública, así que las quince rutas nuevas contestarían `404` y `just dev` no serviría para nada. Se montan públicas y sin capa en ese modo, que es lo que `api-shell` ya dice del modo de desarrollo.
- **El orden de las dos líneas del router es semántico**: `method_not_allowed_fallback` va **antes** de colgar el *gate*, o un `405` sale sin haber exigido sesión y revela qué métodos existen.
- **De una violación de clave foránea no se sabe cuál de las dos referencias falló**: `backup_jobs` tiene dos y SQLite contesta `FOREIGN KEY constraint failed` sin nombrar ninguna. El mensaje del `409` no adivina.
- **La guarda del directorio de prueba borra al caer y el pool la sobrevive**: si el ayudante del arnés construye la base y no devuelve con el estado la guarda que la mantiene viva, el fichero desaparece con la conexión abierta. La guarda tiene que viajar con el estado hasta el final de la prueba.
- **`PUT` es reemplazo completo**: lo que no venga en el cuerpo vuelve a su valor por defecto, con la salvedad de los secretos. No hay `PATCH`.
- **El `DELETE` de un repositorio arrastra sus tareas y con ellas sus ejecuciones** por el `ON DELETE CASCADE` del §3: el histórico desaparece con el repositorio. Queda escrito para que nadie lo descubra vaciando la tabla.

## Auditado y corregido

### Módulo 1 — el estado, los tipos del cable y la traducción de los errores

RED medido: **157 passed; 5 failed** de 162 en el binario —los dos rojos de `src/api/repositories.rs` y los tres de `src/error.rs`— y **1 passed** en la de integración. GREEN: **162 passed; 0 failed** más la de integración. Tras el incremento correctivo: **164 passed; 0 failed** más la de integración.

La auditoría independiente encontró **un defecto de seguridad bloqueante** y lo midió **en vivo** contra el binario: el extractor `axum::Json` rechaza **antes** de entrar al manejador y su rechazo por defecto sale en `text/plain`, con `422` y con el valor del campo **ecoado dentro**. El director lo reprodujo sobre su propio binario y en un puerto limpio, con el cuerpo entero igual a la credencial —`Failed to deserialize the JSON body into the target type: invalid type: string "AKIAIOSFODNN7EXAMPLE", expected struct …`— y con la credencial como nombre de un campo de más. El registro del servidor **no** la llevaba: la fuga era al cliente.

Corrección: un extractor propio, `src/api/extract.rs`, que envuelve a `axum::Json` para quedarse con su camino real de extracción —tipo de contenido, lectura del cuerpo y deserialización— y traduce el rechazo a la envoltura: `400` con `{status, message}` y un mensaje **nuestro** por clase de fallo, más un `tracing::warn!` que deja constancia de **que** una petición no se pudo interpretar, **sin** su texto. Lo usan los dos manejadores con cuerpo. Cerrado con las pruebas 1.23 y 1.24 y verificado otra vez en el cable real: `400` con `application/json`, cero apariciones de la credencial en el cuerpo **y** en el registro, y el alta válida intacta en `201`.

Los demás hallazgos de esa auditoría, en corto:

- **Una laguna de cobertura** que explicaba el bloqueante —ninguna prueba miraba el **tipo de contenido** de un rechazo, así que un `400` en texto plano pasaba inadvertido—: cerrada por 1.23 y 1.24.
- **Una justificación falsa** en el doc-comment del booleano por credencial —decía que el formulario, el cuerpo y la columna se llaman igual, y el cuerpo informa `secret_access_key`, no `has_secret_access_key`—: reescrita, y la tensión entre el requisito que manda llamar a los campos como las columnas del §3 y el booleano del requisito 8 queda **declarada como añadido al SRS** en el `proposal` y en la spec, en vez de quedar como una contradicción.
- **La precisión del doc-comment de `contenido`**: afirmaba que la causa de `serde_json` no lleva el valor, y eso no es una garantía del crate sino de estos tipos de respuesta; ahora lo dice así.
- **Un hallazgo rechazado, con su motivo**: los `400` de validación del manejador no dejan rastro propio en el registro, y no se añade. Lo que el contrato manda registrar es **que** una petición no se pudo interpretar —el cuerpo ilegible, que es un defecto de integración de quien llama y cuyo texto no se le puede devolver porque puede ecoar el valor—, y no cada validación que falle: un formulario rellenado mal es el desenlace normal de un `400`, ya le dice al cliente qué campo estaba mal, y no hay acción que tomar por él. Registrar cada uno cambiaría el ruido del cliente por ruido del operador.

## Verificación

*(Se rellena al cerrar. Son las puertas del repositorio —`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked` y `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`— más las diez comprobaciones de operador de la fase 4 del `tasks.md`, que esta vez van con `curl` **de verdad** contra el binario —ya hay endpoints— y con `sqlite3` sobre el fichero: los altas y las lecturas de los tres recursos, la columna del secreto guardando el sobre, el `NULL` del `db_name` de un directorio, la `passphrase` que **no** cambia cuando no se envía, las credenciales buscadas literalmente en los bytes del fichero, el `limit` fuera de rango, la cascada del borrado y el `401` sin cookie en modo `Oidc`.)*

## Deuda declarada

- **Los tres endpoints del §5 que no se entregan**: `POST /jobs/:id/run` y `GET /events` llegan con el **motor**, y `GET /snapshots` con **Rustic**. Cada uno con su motivo escrito en el `proposal` y en la spec.
- **No hay agregados ni `/stats`**: «el historial **y métricas**» del §5 se lee como que cada fila lleva sus métricas —los dos contadores de bytes y sus dos fechas—, y quien sume totales es el panel. Un `/stats` sería una segunda fuente de verdad que mantener en sincronía con los listados.
- **No se filtra por repositorio en `/executions`**: el índice existe desde `db-schema`, pero el `JOIN` con `backup_jobs` y su filtro los añade el change que lo pida.
- **El listado de los recursos no tiene paginación**: solo `/executions` la lleva, que es la tabla que crece. Con muchas filas, `GET /targets` devuelve todo, y acotarlo es una tarea de quien lo necesite.
- **`PUT` es reemplazo completo, sin `PATCH`**: el panel siempre tiene el objeto entero porque lo ha leído antes.
- **Los ayudantes internos de la capa de acceso siguen copiados en las cuatro capas** (heredado de `db-schema`).
- **Las más de treinta construcciones de `AppState`** que hereda el arnés se ajustan a la firma nueva, y los siete ayudantes pasan a ser asíncronos: es un cambio ancho, mecánico y verificado por las 156 pruebas que ya existen, que tienen que seguir verdes afirmando lo mismo.
- **Los tres manejadores que siguen sin implementar —`listado`, `actualizar` y `borrar` de `/repositories`— revientan si se alcanzan por la red**: son `todo!()` y llegan con el módulo 2. Medido: sin `CatchPanicLayer`, el `panic!` deja caer la tarea de la conexión, el cliente ve la conexión cerrada sin respuesta (`curl` contesta `000`) y el pánico sale en el registro del servidor. Queda declarado mientras dure la fase, para que nadie lo confunda con un fallo del servidor: ningún binario entregado debe tener un camino declarado y sin implementar.

## Cierre

- [ ] `tasks.md` completo (obligatorio: `openspec archive` se niega si queda una casilla sin marcar).
- [ ] `docs/FEATURE-004-api-endpoints.md` con lo que realmente quedó construido.
- [ ] La cuenta corregida a **diez** endpoints en `AGENTS.md`, `plans/PLAN-003-COMPLETED.md` y `docs/FEATURE-003-db-schema.md`.
- [ ] `AGENTS.md`: el estado —los módulos que siguen vacíos son cuatro— y la superficie HTTP nueva.
- [ ] Redactar el `Purpose` de `openspec/specs/api-resources/spec.md`: es capability **nueva** y no puede quedarse en `TBD`.
- [ ] `openspec archive api-endpoints`.
- [ ] Renombrar este fichero a `plans/PLAN-004-COMPLETED.md` con `just plan-done 4`.
- [ ] PR `feature/api-endpoints` → `development` con el CI en verde. **Sin release a `main`**: se publica solo hasta `development`.
