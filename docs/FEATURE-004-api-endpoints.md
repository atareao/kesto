# FEATURE-004 — api-endpoints

> Implementación del change proposal `api-endpoints`, en `openspec/changes/archive/2026-10-25-api-endpoints/` — **11 requisitos y 33 escenarios** en la capability nueva `api-resources`, más tres deltas: `api-shell` (2 requisitos / 7 escenarios), `db` (1 / 3) y `secrets` (2 / 5). `openspec validate api-endpoints --strict` válido.
> Cerrada el 2026-10-25. Rama: `feature/api-endpoints`, desde `development`. Plan: `plans/PLAN-004-COMPLETED.md`. Especificación de origen: `docs/DEFINITION.md` §5 (API REST), con el §3 (modelo de datos) y el §7 (layout).
> PR: [#9 — `feature/api-endpoints` → `development`](https://github.com/atareao/kesto/pull/9) · CI en verde (run [`40170430143`](https://github.com/atareao/kesto/actions/runs/40170430143)). **Sin release a `main`.**
> **Sin release a `main`**: se publica solo hasta `development`.
> Diff contra `development`: **28 ficheros, +7755 / −135**, en **15 commits**. `Cargo.lock`: **302** paquetes, **sin tocar** — el change no estrena ni una dependencia.
> Estado: **cerrado** — **214 tests** (213 en el binario y 1 de integración, `tests/arranque.rs`), **58 nuevos** sobre los 156 de `db-schema`. `tasks.md`: **99** tareas.
> El delta **ya** se ha archivado: el change vive en `openspec/changes/archive/2026-10-25-api-endpoints/`, y sus specs están fusionadas en `openspec/specs/` —`api-resources`, capability nueva con su `Purpose` redactado, más lo añadido a `api-shell`, `db` y `secrets`—.

Hasta este change, `docs/DEFINITION.md` §5 declaraba **diez** endpoints y **no existía ninguno**. La capa de acceso del §3 estaba escrita, probada y auditada, y **nadie la alcanzaba desde el proceso**: era una biblioteca.

Lo que trae este change es la superficie: el orquestador de copias tiene por dónde recibir datos y por dónde devolverlos, y la capa de acceso deja de ser código que solo ejecutan las pruebas para convertirse en código que ejecuta el binario.

## Qué es y qué no es

Es la **superficie HTTP del §5**: las rutas con las que se dan de alta, se consultan, se editan y se borran las fuentes de respaldo, los repositorios S3 y las tareas programadas, más el listado del histórico de ejecuciones. Y es **el primer consumidor de la capa de acceso**: lo que la saca de dentro de las pruebas y la pone en el binario.

### Fuera del alcance, con destinatario cada uno

- `POST /api/v1/jobs/:id/run` — necesita al **motor**. Crear una ejecución `pending` que nadie va a ejecutar es prometer al cliente algo que no ocurre.
- `GET /api/v1/events` — el SSE llega con el motor, que trae a la vez el transporte y su primer evento real. No hay hub, ni `broadcast` en el estado, ni manejador de `text/event-stream`.
- `GET /api/v1/snapshots?repository_id=<id>` — necesita a **Rustic**: hay que listar los snapshots del repositorio en S3 y `src/rustic/` está vacío.

No hay agregados ni `/stats`: «el historial **y métricas**» del §5 se lee como que **cada fila** lleva sus métricas —los dos contadores de bytes y sus dos fechas—, y quien sume totales es el panel. Un `/stats` sería una segunda fuente de verdad que mantener en sincronía con los listados.

No hay `PATCH` ni actualización parcial; no hay ordenación configurable, ni `ETag`, ni caché, ni versionado de la API; y no se toca la autenticación, ni la partición, ni la envoltura de `api-shell`.

### Lo que sigue sin existir

Los **cuatro módulos del §7 que siguen vacíos** tras este change: `docker`, `engine`, `rustic` y `scheduler`. Y `ui/` y `Dockerfile` siguen sin existir.

## Los tres módulos y el orden de entrega

Se entregó en tres módulos, y el orden importa porque cada uno es la base del siguiente.

### Módulo 1 — el estado y los tipos del cable

El estado de base dentro del `AppState`, los DTO, y la traducción de los errores de la petición a `4xx`. Trajo el único defecto **bloqueante de seguridad** del change (ver las auditorías).

### Módulo 2 — los tres recursos

`/repositories`, `/targets` y `/jobs`, con su validación, su instrumentación y la partición probada sobre las quince rutas.

### Módulo 3 — el listado de ejecuciones

Los filtros, la ventana acotada y el orden estable, más el extractor propio de los **parámetros de consulta**.

### La medición

Medido, módulo a módulo, en el binario:

| Tanda | Resultado |
|---|---|
| Módulo 1 | **157/5 → 162/0 → 164/0** |
| Módulo 2, `/repositories` | **164/7 → 171/0** |
| Módulo 2, `/targets` | **171/9 → 180/0** |
| Módulo 2, `/jobs` | **182/6 → 188/0** |
| Módulo 2, cobertura transversal | hasta **203/0** |
| Módulo 3 | **203/9 → 213/0** |

Más **1** prueba de integración, siempre verde.

## El estado de base dentro del AppState

El pool, la `Clave` de cifrado y las cuatro capas (`Repositorios`, `Targets`, `Tareas`, `Ejecuciones`) entran en un **estado de base** que el `AppState` lleva como campo **no opcional**: un Kesto sin base no es un estado válido y **el tipo lo dice**.

### Por qué no un `Option`

Se descartó el `Option` porque dejaría escrito lo contrario —que un arranque sin base es un estado admitido—.

### Por qué no abrir la base en el constructor

Se descartó abrir la base en el constructor porque obligaría a que fuera asíncrono: la base llega **ya abierta** desde `main`, que es quien abre el pool y aplica las migraciones.

Así que `AppState::new` sigue sin ser asíncrona y solo gana un argumento.

### Lo que costó y lo que no se tocó

Costó ajustar **los 16 sitios** que construyen un `AppState` y **los siete ayudantes** del arnés, que pasan a ser asíncronos, con sus **61** puntos de llamada.

Y **ninguna aserción se debilitó**: las 156 pruebas que ya existían siguen afirmando exactamente lo mismo.

## Los tipos del cable son propios de la API

Los DTO viven en `src/api/` y la separación la imponen **tres hechos medidos**, no una preferencia:

- **`SecretString` no implementa `Serialize`**, así que una respuesta que llevara una credencial **no compila**: la red la cierra el compilador, no la disciplina. Sí gana un `Deserialize` **manual** —nunca derivado, porque el de su `Zeroizing` interno vive tras una feature **opcional** de `zeroize` que se unificaría para todo el grafo— y **no** gana `Serialize`.
- **`uuid` está declarado sin `serde`**, así que los identificadores viajan como **texto en su forma canónica con guiones**. La trampa del BLOB del §3 no puede reaparecer en el cable, y es una consecuencia de las dependencias, no una convención.
- **`time` no produce RFC3339 con su `Serialize` por defecto** —la feature `serde-human-readable` no está activada—, así que las fechas se formatean **por nosotros** con `OffsetDateTime::format(&Rfc3339)`: acaban en `Z`, sin fracción de segundo, y hay una prueba que fija el literal.

### Dos añadidos declarados

**Los secretos no viajan de vuelta jamás**: cada credencial sale como un booleano `has_*` que dice si está configurada, que es el **único** campo del cable que no refleja una columna del §3.

**`PUT` es reemplazo completo** de los campos mutables, con `id` y `created_at` fuera. Un secreto **ausente** conserva el valor almacenado; uno **vacío** se rechaza con `400` nombrando el campo.

## La superficie: dieciséis rutas

Siete de los diez endpoints del §5, y nueve más declaradas **por escrito como añadido al SRS** —con el precedente de los cuatro índices que `db-schema` añadió al §3—, porque sin `GET /:id` el formulario de edición se rellena a ciegas:

| Recurso | Rutas | Origen |
|---|---|---|
| `/repositories` | `GET`, `POST`, `GET /:id`, `PUT /:id`, `DELETE /:id` | las dos primeras del §5; las tres últimas, añadido |
| `/targets` | las cinco | ídem |
| `/jobs` | las cinco | ídem |
| `/executions` | `GET` | del §5, y **solo esa**: es de **solo lectura** |

### Sobre `/executions`

Una ejecución **no se crea desde el panel**, la crea una copia que corre. Las tres escrituras de la capa de acceso existen desde `db-schema` y las usará el **motor**; hoy siguen sin ruta.

### Los códigos

| Código | Cuándo |
|---|---|
| **`201`** | al crear |
| **`200`** | al leer, actualizar y borrar |
| **`404`** | cuando la fila no existe |
| **`400`** | para la petición mal formada |
| **`409`** | para la clave foránea violada |
| **`500`** | con el mensaje genérico, para todo lo demás |

El `DELETE` contesta `200` con la envoltura y `data` nulo, **no** `204`, porque el contrato dice que **toda** respuesta es JSON y un `204` no tiene cuerpo.

### Sobre el `409`

Es la **red de seguridad** de una carrera —otra petición borró el target entre la comprobación y el `INSERT`—, no el camino normal. El camino normal **valida antes de escribir** y contesta un `400` preciso que nombra el campo. Y el `409` no copia el texto de `sqlx`: deja la causa en el registro.

## La validación que no existía en ningún sitio

Tres reglas, y ninguna la puede imponer el esquema.

### La coherencia del target con su tipo

Un `directory` exige `host_path` y no admite `container_name`, `db_user`, `db_password` ni `db_name`; un `mariadb` o un `postgresql` exige `container_name`, `db_user` y `db_password`, y `db_name` **es opcional** —`NULL` es dump global, la semántica que fijó `db-schema`—. El `CHECK` del §3 solo limita el valor de `target_type`, así que una fila incoherente se guardaba sin que nada la rechazara.

### Las referencias existen antes de escribir

Sin esto, un `POST /jobs` con un `target_id` inventado llegaba al `INSERT` y salía por el `409`.

### Los campos desconocidos se rechazan

`deny_unknown_fields`: sin eso, una errata como `keep_dailly` se convertiría en un campo que nadie escribe y nadie lee, y la retención se quedaría en su defecto sin que la respuesta dijera una palabra.

Lo mismo vale para los **parámetros de consulta**: un `?limits=5` que se ignorara en silencio devolvería el listado por defecto mientras quien llamó cree haber acotado.

## El listado de ejecuciones

`GET /api/v1/executions` es la tabla que **más crece** de las cuatro, una fila por intento de copia, y por eso la única con **ventana**.

### La ventana

Filtros `job_id` y `status`; `limit` con **50** por defecto y **200** de tope; `offset` desde cero.

**Un `limit` fuera de rango contesta `400` y no se recorta**: recortar habría dejado a quien pidió creyendo que recibió lo que pidió. Y `?limit=` —presente y vacía— es `400`, no el valor por defecto: es la misma regla que la del `BIND_ADDRESS` y el `RUST_LOG` del arranque, y aquí la hace cumplir el intérprete.

### El orden estable

El orden es **`started_at DESC, id`** y el `id` desempata porque las ejecuciones del **mismo segundo** empatan —un lote disparado por el mismo tick de cron arranca a la vez y `CURRENT_TIMESTAMP` no tiene más resolución—.

Sin desempate, dos peticiones con el mismo `offset` pueden repetir una fila y saltarse otra, y el panel paginaría de forma visiblemente rota.

### El filtro en la capa de acceso

**El filtro vive en la capa de acceso y su SQL es una sola cadena estática**: `WHERE (?1 IS NULL OR job_id = ?1) AND (?2 IS NULL OR status = ?2) ORDER BY started_at DESC, id LIMIT ?3 OFFSET ?4`, con los cuatro valores **enlazados**.

Componer el `WHERE` según los filtros presentes sería «construir la consulta concatenando», que es lo que la auditoría prohíbe. El filtro ausente se expresa **dentro** de la sentencia.

### `error_message`

Sale por el cable **tal cual** —el panel necesita saber por qué falló una copia— y quien la escribe es el motor, que **no debe volcar credenciales ahí**: es deuda declarada, con su motivo.

## La red sobre el registro: los dos extractores propios

Es el apartado que más ha valido del change, y se cuenta como se midió.

### El defecto, medido contra el binario

`axum::Json` rechaza **antes** de entrar al manejador, y su rechazo por defecto describe el fallo con el texto del intérprete, que **ecoa el valor del campo**: un cuerpo como `{"keep_daily": "AKIA…"}` produce `invalid type: string "AKIA…", expected i64`, sale en `text/plain` y con el código del intérprete.

Es decir: **la credencial acababa en el cuerpo de la respuesta**. Se midió contra el binario, con el cuerpo entero igual a la credencial.

### La corrección: `src/api/extract.rs`

Un `Json` **propio** que envuelve al de axum para quedarse con **todo** su camino real de extracción —el tipo de contenido, la lectura del cuerpo y la deserialización— y cambiar solo lo que pasa cuando falla: `400` con la envoltura, un mensaje **nuestro** por clase de fallo, y un `tracing::warn!` que deja constancia de **que** una petición no se pudo interpretar y **sin** su texto.

### El mismo tratamiento para los parámetros de consulta

El **mismo** tratamiento se aplica a los parámetros de consulta, con el `Query` propio: `QueryRejection` está declarado `#[non_exhaustive]` con **una sola** variante, así que la traducción lleva un brazo comodín que hoy no cubre nada y está para que una variante nueva de axum caiga por el mismo camino en vez de romper la compilación.

### La instrumentación

Un middleware `trazar` sobre el router registra **método, ruta y código** —nunca el cuerpo— y lo hace **en un solo sitio** y no en cada manejador, porque son dieciséis rutas y dieciséis llamadas a mano son dieciséis sitios donde olvidarse.

Va a `debug`, así que en un despliegue sin tocar `RUST_LOG` no queda traza: es deliberado —una línea por clic del panel es ruido de operación a `info`— y está declarado.

## La partición intacta

Las rutas nuevas cuelgan de la mitad de negocio **por construcción** y sin una sola excepción nueva a la lista blanca: la salud y, solo en modo `Oidc`, las dos rutas del flujo de entrada.

En modo de desarrollo no se monta la mitad protegida —de modo que `/api/v1/auth/*` **no** contesta `401`, contesta `404` con la envoltura— pero la superficie de recursos **sí** se monta, pública y sin capa: es lo que `api-shell` ya decía, y sin ello `just dev` contestaría `404` en cada recurso.

## La verificación

### Las cuatro puertas

`just check` (`cargo fmt --all -- --check` → `cargo clippy --all-targets -- -D warnings` → `cargo test --locked`) en verde con **213 passed; 0 failed** en el binario y **1 passed** en la de integración; `just doc` (`RUSTDOCFLAGS="-D warnings"`) limpio.

`just check-all` **no** se puede ejecutar y no es un fallo: el job del panel se salta solo mientras no exista `ui/`.

### Las diez comprobaciones de operador

Diez comprobaciones con `curl` **de verdad** contra dos instancias del binario —una en modo de desarrollo con `RUST_LOG=trace` y otra en modo `Oidc`— y con `sqlite3` sobre el fichero. Las que más dicen:

- Las cuatro altas, **`201`**; las ocho lecturas, **`200`**, con **cero** apariciones de las tres credenciales en 2 639 bytes de respuestas y el control encontrado (la búsqueda funciona).
- El sobre `v1:<nonce>:<cifrado>` con sus **tres** partes en las tres columnas y **nonce distinto por valor**.
- **Cero** apariciones de las credenciales en los bytes de `kesto.db` y su `-wal`, con los dos controles encontrados.
- El `limit` hostil: `?limit=201`, `?limit=0`, `?limit=-1`, `?limit=abc`, `?offset=-1`, `?limits=5` y `?limit=` → **`400`** los siete, con la envoltura, en `application/json` y sin una traza del intérprete.
- La cascada: el borrado de un repositorio se llevó **sus tareas y sus ejecuciones** y dejó **intactos los targets**; el borrado repetido contesta `404`.
- En modo `Oidc`, **`401`** con envoltura en las rutas de negocio sin cookie, **`200`** en la salud con `auth_mode: "oidc"`.
- **Cero** credenciales en **130 líneas** de registro `trace`, con las 36 del middleware presentes.

### Una expectativa escrita que la medición contradijo

La comprobación decía «el sobre idéntico al anterior» al actualizar sin mandar la `passphrase`, y es **falso** —la capa vuelve a cifrar y el nonce es aleatorio por valor, así que el sobre **cambia de bytes** en cada escritura (`v1:9d6zm/…` → `v1:7S6bZ+…`)—.

Lo que se conserva es la **estructura y el tamaño** —el cifrado de 80 y 76 caracteres antes y después— y el **valor**, que lo fija por descifrado la prueba `un_put_sin_passphrase_conserva_la_almacenada`. La expectativa se **corrigió** en el `tasks.md` en vez de marcarla en verde.

### Una observación del auditor, refutada con `EXPLAIN QUERY PLAN`

El patrón `(?1 IS NULL OR job_id = ?1)` **no** inhabilita ningún índice —SQLite elige `idx_job_executions_job_id` o `idx_job_executions_status_started_at` según el filtro—; lo único que se paga es la ordenación del término `id` (`USE TEMP B-TREE FOR LAST TERM OF ORDER BY`), que queda como deuda declarada.

## Las auditorías: qué devolvieron y qué se corrigió

Tres módulos auditados por revisores **independientes** y de **solo lectura**.

### Módulo 1 — un defecto de seguridad bloqueante, medido en vivo

Es el del apartado de los extractores: el rechazo por defecto de `axum::Json` publicaba la credencial en el cuerpo, en `text/plain` y con el código del intérprete. El director lo reprodujo sobre su propio binario, con la credencial como valor de un campo numérico y también como **nombre de un campo de más**. El registro del servidor **no** la llevaba: la fuga era al cliente.

Se cerró con el extractor propio, y con él llegó una **laguna de cobertura** que explicaba por qué nadie lo había visto: ninguna prueba miraba el **tipo de contenido** de un rechazo, así que un `400` en texto plano pasaba inadvertido.

Y otra cosa se corrigió de paso: una justificación **falsa** en un doc-comment, que afirmaba una garantía que no era del crate sino de estos tipos de respuesta.

### Módulo 2 — el registro estaba vacío

El delta `secrets` exige por escrito que el registro diga **qué** ocurrió —con la identidad del recurso— y nunca el contenido del cuerpo, con dos escenarios: que un alta deje la traza de que **se atendió**, y que un rechazo de validación deje la del **campo que no cumplía**. Medido contra el binario, no había **nada**: los manejadores no registraban.

Cerrarlo costó un middleware sobre el router —método, ruta y código— y **dos** trazas más, porque el `400` del secreto vacío en `/repositories` **no pasa por el manejador**: lo rechaza la capa de acceso, así que instrumentar solo la primera rama dejaba sin traza el rechazo más probable de un formulario. Esto lo descubrió el subagente al **medir**, no al leer: la tanda quedó en 197/1 y el fallo decía, con el registro en la mano, que la traza del campo no estaba.

Y **una decisión del módulo 1 revertida en el 2, con el motivo escrito**: el director había rechazado registrar los `400` de validación —«un formulario rellenado mal es el desenlace normal de un `400`»—, y era un error de lectura del contrato: el escenario **pide** la traza del campo. Lo revirtió una prueba que nació en rojo con el registro vacío, no una relectura. Lo que sigue rechazado es lo que de verdad pedía el argumento —registrar el **valor** o el **cuerpo**—, y eso no se hace en ningún punto.

### Módulo 3 — un fallo del propio director, destapado por la verificación

La prueba de la capa de acceso que él dictó salió **flaky**: medida diez veces seguidas, falló **siete**. La causa no estaba en el código —el SQL y el orden eran correctos— sino en la aserción: la prueba siembra dos filas con la **misma** `started_at` para ejercitar el desempate, el orden entre ellas lo decide el `id` —que es **aleatorio** en cada ejecución—, y afirmaba cuál de las dos iba primero, que es medir el azar del UUID y no el contrato.

Corregido afirmando el **conjunto** de las empatadas y dejando el desempate a las aserciones por identificador. Y se verificó como se verifica un fallo intermitente: **doce** pasadas seguidas de la prueba sola y la suite entera verde **dos** veces —una sola pasada verde no prueba nada cuando el fallo es de uno de cada tres—.

## Deuda pendiente y límites aceptados

- Los **tres** endpoints del §5 que se difieren: `POST /jobs/:id/run` y `GET /events` con el **motor**, `GET /snapshots` con **Rustic**.
- `error_message` es una **frontera de confianza**: lo escribe el motor y no debe volcar credenciales ahí —el §4.A manda la contraseña del volcado por la línea de comandos del `exec`, así que la salida de un volcado fallido puede traerla dentro—.
- `ORDER BY started_at DESC, id` cuesta una **ordenación parcial** (medido con `EXPLAIN QUERY PLAN`): el `id` del desempate no está en el índice, así que el plan dice `USE TEMP B-TREE FOR LAST TERM OF ORDER BY`. Un índice que incluya `id` la quitaría, y va con el change del motor, que es quien llena el histórico.
- **`offset` no tiene tope** (solo `>= 0`): un `offset` enorme es un no-op en SQLite, así que no es un defecto hoy.
- **El listado no devuelve el total de filas**: el panel no puede pintar «N de M» sin una llamada extra, y esa llamada no existe. El §5 no pide agregados.
- **No hay paginación en los listados de los tres recursos**: solo la lleva `/executions`, que es la tabla que crece. `GET /targets` devuelve todo.
- **No se filtra por repositorio en `/executions`**: el índice existe desde `db-schema`, pero el `JOIN` con `backup_jobs` y su filtro los añade el change que lo pida.
- **`PUT` es reemplazo completo, sin `PATCH`**: el panel siempre tiene el objeto entero porque lo ha leído antes.
- **La instrumentación solo se ve con `RUST_LOG=debug` o `trace`** y el valor por defecto es `info`: en un despliegue sin tocar la variable no queda rastro de las peticiones atendidas.
- **`Ejecuciones::listar` —el listado ascendente— y sus tres escrituras siguen sin ruta**: los usará el motor; si no acaba necesitándolos, se retiran entonces.
- **Los ayudantes internos de la capa de acceso siguen copiados en las cuatro capas** (heredado de `db-schema`).
