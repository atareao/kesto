# FEATURE-003 — db-schema

> Implementación del change proposal `db-schema`, en `openspec/changes/archive/2026-10-25-db-schema/` — **11 requisitos y 50 escenarios** (`db` 7/34, capability **nueva**; `secrets` 4/16, que crece con el cifrado en reposo), `openspec validate db-schema --strict` válido.
> Cerrada el 2026-10-25. Rama: `feature/db-schema`, desde `development`. Plan: `plans/PLAN-003-COMPLETED.md`. Especificación de origen: `docs/DEFINITION.md` §3 (modelo de datos) y §7 (layout).
> PR: [#8 — `feature/db-schema` → `development`](https://github.com/atareao/kesto/pull/8) · CI en verde (run [`40162918376`](https://github.com/atareao/kesto/actions/runs/40162918376)). **Sin release a `main`.**
> Diff contra `development`: **22 ficheros, +7288 / −16**. `Cargo.lock`: **302** paquetes.
> Estado: **cerrado** — **156 tests** (155 en el binario y 1 de integración, `tests/arranque.rs`), **62 nuevos** sobre los 93 de `auth-capability`. `tasks.md`: **98** tareas.
> El delta **ya** se ha archivado: el change vive en `openspec/changes/archive/2026-10-25-db-schema/`, y sus specs están fusionadas en `openspec/specs/` —`db`, capability nueva, con su `Purpose` redactado tras el archivado, y `secrets`, que creció.

**El archivo está hecho.** Hasta este change `DATABASE_URL` era una cadena decorativa: el proceso arrancaba con un DSN que no llevaba a ninguna parte —no había `migrations/`, ni pool, ni entidades, ni cifrado—, y los tres secretos del §3 se habrían guardado **en claro** en el fichero que los custodia. Ahora `src/db/` existe, el esquema del §3 se aplica al arrancar, el pool abre con WAL y claves foráneas, cada tabla tiene su entidad y su capa de acceso, y las tres columnas de secreto guardan un sobre cifrado.

## Qué es y qué no es

Es la persistencia del §3 de `docs/DEFINITION.md`: el esquema, el pool, la capa de acceso y el cifrado en reposo de `secret_access_key`, `passphrase` y `db_password`.

Es además **la raíz del grafo**: de los cinco módulos del §7 que estaban vacíos antes de este change —`db`, `docker`, `engine`, `rustic` y `scheduler`—, `db` es el que los demás necesitan: el motor necesita targets y repositorios que leer, el planificador tareas que agendar y el contenedor ejecuciones que registrar. Los otros cuatro siguen vacíos.

Y cierra cuatro cabos sueltos del SRS anotados como deuda desde `FEATURE-001`: los secretos en `TEXT` plano, las claves foráneas sin índice, `job_executions` sin forma razonable de filtrarse por repositorio, y `db_name` inaplicable con los comandos del §4.A.

**Fuera de este change**, y cada uno con su destinatario:

- Los **nueve endpoints del §5** (change propio, con su contrato y sus escenarios).
- El **motor**, **Docker**, **Rustic** y el **planificador**.
- El **cifrado del repositorio Rustic**.
- La **rotación de `KESTO_MASTER_KEY`** y cualquier clave de datos envuelta por una maestra rotable.
- **SQLCipher**.
- **`VACUUM`** programado y la **retención** de `job_executions`.
- El **`Dockerfile`** y el volumen del `compose`.

## Los tres módulos y el orden de entrega 1 → 3 → 2

El change se reparte en tres módulos entregables, cada uno con su ciclo RED → GREEN → auditoría independiente → correcciones:

| # | Módulo | Contenido | Ficheros |
|---|---|---|---|
| 1 | Apertura y esquema | Pool, pragmas, migraciones embebidas, error de base | `migrations/0001_init.sql`, `migrations/0002_indexes.sql`, `src/db/mod.rs`, `src/db/migrate.rs`, `src/error.rs` |
| 2 | Entidades y acceso | Las cuatro entidades, su mapeo y su capa de acceso, con la cascada a tres niveles | `src/db/entities/{mod,repository,target,job,execution}.rs` |
| 3 | Cifrado en reposo | El sobre versionado, la derivación por HKDF y el descifrado que falla en alto | `src/db/crypto.rs` |

El **orden de entrega fue 1 → 3 → 2**, y no el de la numeración, porque la capa de acceso **descifra**: el escenario «la lectura por la capa de acceso devuelve el valor original» y los cuatro del «descifrado que falla en alto» pasan por ella, así que las entidades dependen del módulo de cifrado. Escribirlas antes habría obligado a escribirlas dos veces: una con el valor guardado en claro y otra cuando llegara el sobre. La numeración de la tabla es la del reparto de contenido, no la del calendario.

## El esquema

Las **cuatro tablas del §3** —`repositories`, `backup_targets`, `backup_jobs` y `job_executions`— con sus columnas, sus tipos y sus valores por defecto, y con las restricciones **en la base**, no sólo en Rust: un `INSERT` hecho con `sqlite3` a mano choca con el mismo `CHECK` que choca la aplicación.

- **Dos `CHECK` de enumerado:** `target_type IN ('mariadb','postgresql','directory')` y `status IN ('pending','running','success','failed')`. El enumerado es una garantía de la base, no una convención de la aplicación.
- **Tres `ON DELETE CASCADE`:** `backup_jobs` cuelga de `backup_targets` y de `repositories`; `job_executions` cuelga de `backup_jobs`. Borrar un target arrastra sus tareas y, con ellas, sus ejecuciones; borrar un repositorio arrastra sus tareas y sus ejecuciones.

**Cuatro índices añadidos al SRS**, declarados como añadido y no colados —el §3 no tiene ningún índice—:

| Índice | Por qué |
|---|---|
| `idx_backup_jobs_target_id` | columna hija de la FK a `backup_targets` |
| `idx_backup_jobs_repository_id` | columna hija de la FK a `repositories` |
| `idx_job_executions_job_id` | columna hija de la FK a `backup_jobs` |
| `idx_job_executions_status_started_at` | el listado del §6, por estado y fecha |

Los tres primeros existen porque **SQLite no indexa las claves foráneas**: sin índice en la columna hija, cada borrado en cascada recorre la tabla entera, y el §3 declara tres cascadas sin pagar ninguno de esos tres recorridos. El cuarto cubre el listado del panel: sin él, la consulta ordena recorriendo.

**No se desnormaliza `repository_id` en `job_executions`.** Filtrar las ejecuciones por repositorio va por el `JOIN` con `backup_jobs`, que ya se apoya en los índices anteriores; duplicar la columna crearía una segunda verdad que se puede desincronizar.

## Las migraciones embebidas

El esquema se aplica con `sqlx::migrate!()`, **embebido en el binario**, al arrancar y antes de aceptar peticiones. Un fallo de migración es **error de arranque**: un esquema a medias es peor que un proceso que no arranca, porque el motor escribiría contra columnas que no existen y el fallo aparecería como una copia perdida en vez de como un arranque fallido.

Son **idempotentes** por la tabla de control de `sqlx`: arrancar dos veces sobre la misma base no cambia el esquema ni vuelve a aplicar una migración ya aplicada.

**La trampa, medida.** El prefijo del fichero es un **entero** —`0001_init.sql`, `0002_indexes.sql`—, no la forma `V001__init.sql` del estilo de Flyway. `sqlx::migrate!()` parte el nombre por el primer `_` y hace `parts[0].parse::<i64>()`, así que `V001` no parsea y la macro **falla al compilar** con «expected integer version prefix». Se comprobó con un crate de prueba: la forma con `V` **no compila**, y un esquema que no compila no llega a arrancar. `migrations/` tiene que existir antes del primer `cargo check` que incluya la macro, porque el directorio se resuelve en tiempo de compilación.

## El pool

`MAX_CONEXIONES = 5`, `journal_mode=WAL` y `busy_timeout` de 5 s.

`foreign_keys` se enciende **en las opciones de conexión del pool**, no con un `PRAGMA` suelto. Es una opción **por conexión** y está **apagada por defecto** en SQLite, así que un `PRAGMA foreign_keys=ON` ejecutado una vez sobre una conexión del pool sólo la afectaría a ella: las demás seguirían sin claves foráneas, el `ON DELETE CASCADE` del §3 **no ocurriría sin que nada lo delatara** —el borrado simplemente no borraría—. Declararlo en las opciones es lo que hace que toda conexión del pool lo cumpla.

`journal_mode=WAL` es una propiedad del **fichero** y no de la conexión, así que se activa una vez y persiste; permite leer mientras se escribe, que es lo que hace el panel mientras el motor registra progreso. Deja ficheros `-wal` y `-shm` **junto a la base**, así que **el directorio tiene que ser escribible**: es un requisito del volumen del `compose` del §8, y se declara aquí porque es el pragma quien lo impone.

`busy_timeout` es finito porque SQLite serializa las escrituras y sin espera una escritura que coincide con otra **falla** en vez de esperar.

## El DSN

Se acepta la forma `sqlite://<ruta>` del SRS, que para una ruta absoluta lleva **tres** barras (`sqlite:///app/data/kesto.db`); la lectura del DSN **no** cuela la tercera barra en el nombre del fichero.

Si el fichero de la base no existe, **se crea**: una base embebida que exige un `touch` previo es una base que no arranca la primera vez.

Si **el directorio** del DSN no existe, el arranque **falla en alto nombrando la ruta absoluta** que falta y **no** lo crea. Crear el directorio escondería un volumen mal montado —el del `compose` del §8 es precisamente el directorio de la base— y el fallo se manifestaría más tarde y en otro sitio; nombrar la ruta convierte un «no arranca» en un «falta montar esto». El arranque tampoco delega el diagnóstico en el error crudo de la biblioteca, que se limita a decir que no pudo abrir el fichero.

**Consecuencia práctica**, escrita en la fase 4 del `tasks.md`: en una máquina sin `/app/data`, `just dev` con el `DATABASE_URL` por defecto del SRS (`sqlite:///app/data/kesto.db`) **no arranca**. Eso es correcto —ese directorio es el del volumen del `compose`—, así que el desarrollo exporta su propio `DATABASE_URL`.

## El modelo de error

El módulo tiene su propio error, `DbError`, con la causa conservada dentro y una variante por familia:

| Variante | Qué lleva |
|---|---|
| `DirectorioAusente` | la **ruta absoluta** del directorio que falta |
| `RutaNoEsDirectorio` | la **ruta** cuando lo que hay ahí es un fichero, no un directorio |
| `DsnIlegible` | la causa de `sqlx` al interpretar el `DATABASE_URL` |
| `Apertura` | la causa de `sqlx` al abrir el pool |
| `Migracion` | la causa de `sqlx::migrate` |
| `Consulta` | la **sentencia**, y **por tipo** (`&'static str`), con la causa de `sqlx` dentro |
| `SecretoVacio` | el **nombre del campo** cuyo secreto llegó vacío |
| `Cifrado` | el error del módulo de cifrado, en **transparente** |

La regla es una: **la causa va al registro y al cliente le llega el mensaje genérico con un `5xx`**. Nunca la ruta, ni el texto de la consulta, ni un secreto.

Dos detalles que costaron una corrección cada uno:

- La variante de consulta lleva la **sentencia**, y la lleva **por tipo** (`&'static str`), no como `String`: así es **estructuralmente imposible** interpolar un valor enlazado ahí.
- El error de cifrado es **transparente** sobre el del módulo de cifrado —el `?` encadena sin que nadie lo envuelva a mano—, de modo que un fallo de descifrado sube por la capa de acceso sin perder su variante.

## Las entidades y la capa de acceso

Una entidad y una capa de acceso por tabla del §3: `Repositorio`, `Target`, `Tarea` y `Ejecucion`, con sus payloads de alta (`AltaDe*`) y sus capas `Repositorios`, `Targets`, `Tareas` y `Ejecuciones`, cada una con alta, lectura por identificador, listado, actualización y borrado. Todas las consultas van con **prepared statements**: todo enlazado, ni una interpolación.

Lo que importa de la capa, y por qué:

- **El identificador lo genera la capa de acceso** en `Uuid::new_v4()` y lo escribe en `TEXT` con la **forma canónica con guiones**. No es cosmético: `sqlx` codifica un `Uuid` como un **BLOB de 16 bytes** y lo descodifica con `Uuid::from_slice`, así que un `TEXT` de 36 caracteres no descodifica como `Uuid` y un BLOB deja los identificadores **ilegibles para `sqlite3`** y para DB Browser. La columna del §3 dice `TEXT`, y el tipo de Rust convive con ella sin mentirse. El identificador se genera **en la capa**, no en la base: SQLite no tiene `uuid()` y un valor por defecto en el DDL obligaría a interpolarlo.
- **Las fechas son `time::OffsetDateTime`, en UTC**, y se **normalizan al escribir**: el `Encode` de `time` conserva el desplazamiento del valor enlazado, así que un llamante con `+02:00` habría dejado la columna fuera de UTC sin señal. El `DEFAULT CURRENT_TIMESTAMP` del §3 **no se toca**: la implementación de `OffsetDateTime` descodifica el formato `YYYY-MM-DD HH:MM:SS` que produce, asumiéndolo UTC, así que una fila insertada a mano con `sqlite3` sigue siendo legible.
- **Los enumerados se guardan como el texto del §3**, no como número, para que la columna diga lo mismo que el `CHECK`. Un `BOOLEAN` se escribe y se lee como entero 0/1.
- **`leer` distingue «no está» (`Ok(None)`) de «no se pudo leer» (error)**: confundirlos convierte una base caída en «no existe».
- **`listar` ordena por `created_at, id`**: el `id` desempata porque las filas del mismo segundo empatan y el orden tiene que ser estable entre dos llamadas seguidas.
- **`actualizar` es la fila entera**, deja `created_at` fuera —es la fecha de nacimiento, no un campo que se edite— y **vuelve a cifrar los secretos**, así que la fila estrena sobre y nonce.
- **`borrar` devuelve `bool`**, para que «no había fila» no se confunda con «se borró».
- **`Tareas` y `Ejecuciones` no reciben la clave**: no tienen columnas de secreto, y pasarle a una capa una clave que no usa sugeriría que sí.
- El borrado se propaga según lo que el §3 declara, y la cascada ocurre **de verdad**: el escenario recorre los tres niveles —borrar un target deja sin tareas sus `backup_jobs`, y con ellas desaparecen sus `job_executions`—, con las claves foráneas encendidas en la conexión que la ejecuta.

## El cifrado en reposo

El valor guardado es el sobre `v1:<nonce>:<cifrado>`, con **AES-256-GCM** y **nonce aleatorio por valor**. El nonce es aleatorio porque repetirlo con la misma clave rompe el AEAD: guardar dos veces el mismo secreto produce dos sobres distintos.

La clave se deriva de `KESTO_MASTER_KEY` con **HKDF-SHA256** y la etiqueta constante `kesto/secrets/v1`. La etiqueta **no** es configurable: una etiqueta que se puede tocar por variable de entorno es una forma de perder los secretos sin haber rotado nada. El prefijo de versión del sobre permite que un esquema posterior conviva con éste sin un `UPDATE` masivo que exigiría la llave vieja a mano.

- El **descifrado falla en alto** en los cinco casos: sobre mal formado, prefijo desconocido, base64 ilegible, nonce de longitud equivocada, y texto que el AEAD no autentica. **Nunca** devuelve un secreto vacío: eso sería un repositorio configurado sin credenciales que revienta más tarde, en el motor, hablando de S3. El error no lleva dentro el valor cifrado, el nonce ni el claro.
- La **escritura** tampoco acepta un secreto vacío: es la mitad simétrica de lo anterior.
- **No hay AAD, y es una decisión declarada.** Quien pueda escribir la base puede copiar un sobre válido de una columna o de una fila a otra y el AEAD lo autenticará: es integridad, no divulgación —quien escribe el fichero ya controla el plano de datos—, y atarlo obligaría a cambiar la firma de cifrar y descifrar e invalidaría las filas ya escritas.

No es SQLCipher: SQLCipher cifraría el fichero entero y obligaría a un `libsqlite3-sys` distinto, y lo que hay que proteger son tres columnas, no el histórico de ejecuciones.

## La semántica de `db_name`

`backup_targets.db_name` tiene la semántica que el §3 documenta y que este esquema **fija**: **informado** significa que la copia cubre **esa sola base**; **`NULL`** significa dump global. La columna es anulable y **sin valor por defecto**: `NULL` no es un valor que haya que adivinar, es la ausencia de la restricción. Y para un target de tipo `directory`, `db_name` queda a `NULL`: no hay base que acotar.

La semántica se fija en el esquema aunque quien la ejecute sea el motor, porque es la columna la que la define y quien lea la tabla tiene que saber qué significa un `NULL` sin buscar el change del motor.

Queda escrito, además, que **el §4.A tal cual es inaplicable con `db_name` informado**: sus dos comandos —`mariadb-dump --all-databases` y `pg_dumpall`— vuelcan el servidor entero e ignoran la columna. El change del motor tendrá que pasar a `mariadb-dump <db>` y a `pg_dump -d <db> -U <user>` cuando `db_name` no sea `NULL`. Aquí **no se ejecuta ningún comando**: se declara qué significa la columna para que quien lo haga no tenga que decidirlo.

## La verificación

Puertas del repositorio, todas en verde (el CI corre exactamente las tres primeras):

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked --no-fail-fast
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

**156 tests**: 155 en el binario y 1 de integración (`tests/arranque.rs`), de los que **62 son nuevos** sobre los 93 de `auth-capability`. Reparto **de los ficheros que este change toca o estrena**, por fichero:

| Fichero | Tests |
|---|---|
| `src/db/entities/repository.rs` | 15 |
| `src/db/crypto.rs` | 14 |
| `src/db/mod.rs` | 12 |
| `src/db/migrate.rs` | 7 |
| `src/db/entities/target.rs` | 6 |
| `src/error.rs` | 5 |
| `src/db/entities/job.rs` | 3 |
| `src/db/entities/execution.rs` | 3 |
| `src/main.rs` | 2 |
| `src/test_support.rs` | 1 |

Los 68 de esa tabla no son los 155 del binario: los 87 restantes son los tests de los changes anteriores —`api`, `auth`, `config`, `secret` y `state`—, que siguen ahí. 68 + 87 = 155, y con el de integración, 156.

Los 62 nuevos se reparten así: 11 en `db/mod.rs`, 7 en `migrate.rs`, 14 en `crypto.rs`, 15 en `repository.rs`, 6 en `target.rs`, 3 en `job.rs`, 3 en `execution.rs`, 1 en `error.rs` y 2 en `main.rs`.

Cobertura de la especificación: **11 requisitos y 50 escenarios** —`db` 7/34, capability nueva; `secrets` 4/16—, validados en modo estricto.

**Las diez comprobaciones de operador** de la fase 4 del `tasks.md`, contra el binario real y contra `sqlite3`, con lo que midieron:

| # | Qué se comprobó | Resultado medido |
|---|---|---|
| 4.1 | Un fichero nuevo tras el arranque | Las **cuatro** tablas del §3 **más** `_sqlx_migrations`, la tabla de control de las migraciones —cinco en total— |
| 4.2 | Un directorio inexistente | El arranque **se detiene** nombrando la ruta; el directorio **no** se crea |
| 4.3 | WAL y claves foráneas | `journal_mode` en `wal`. El `PRAGMA foreign_keys` de una conexión **nueva** del CLI da `0` —es una opción *por conexión* y el CLI abre la suya—, así que el encendido en **todas** las conexiones del pool no lo puede medir el CLI: lo fija el test 1.8, que lo consulta en cada conexión del pool. Además, los ficheros `kesto.db-wal` y `kesto.db-shm` están **junto a la base mientras el proceso corre** y **desaparecen** tras una parada limpia |
| 4.4 | Los secretos buscados literalmente en el fichero `.db` | Los **tres** aparecen **cero** veces; el **nombre del repositorio** aparece **una** —es lo que demuestra que la búsqueda funciona y que la ausencia no es un fichero vacío mal leído—. La columna guarda el **sobre**, con `v1:` y sus tres partes |
| 4.5 | Doble arranque | Idempotente: el esquema no cambia y ninguna migración se aplica dos veces |
| 4.6 | Una alta huérfana | Rechazada con error tipado; ninguna fila huérfana queda en la tabla |
| 4.7 | La cascada, a mano | Los **tres** niveles: borrar un target deja sin tareas sus `backup_jobs` y sin ejecuciones esas tareas |
| 4.8 | Un sobre manipulado leído por la capa de acceso | Una fila con `v1:AAAA:BBBB` da `Cifrado(NonceDeLongitudEquivocada { longitud: 3 })`; una con el valor en claro da `Cifrado(SobreMalFormado)`. **En ningún caso** un secreto vacío |
| 4.9 | `RUST_LOG=trace` durante el recorrido | Reescrita: ver abajo |
| 4.10 | Los identificadores, con `sqlite3` | `typeof(id), length(id)` es `text|36` en todas las filas —legibles desde `sqlite3`, nunca un BLOB— |

Dos salvedades honestas sobre esa tabla:

- Las filas que leyeron 4.4 y 4.8 las escribió la **propia capa de acceso** desde un arnés **temporal**: no hay endpoints que las escriban desde el proceso.
- La comprobación **4.9 se reescribió**. Su forma manual —arrancar con `RUST_LOG=trace` y dar de alta desde el panel— **no se puede ejecutar** sin los endpoints del §5. Queda fijada por **dos pruebas** que capturan el registro con el mismo suscriptor de `tracing` que usa el resto de la suite, es decir por el camino que sí existe.

## Las auditorías: qué devolvieron y qué se corrigió

Tres auditorías independientes, una por módulo, en sólo lectura, aplicadas antes de empezar el siguiente. **Cada hallazgo cerró con su prueba.**

### Módulo 1 — 8 hallazgos

- El test que cubría los índices se apoyaba en un razonamiento **falso**: daba por hecho que un `EXPLAIN QUERY PLAN` no expande los disparadores de la cascada, y **sí** los expande (medido). La aserción se sustituyó por el plan de los tres `DELETE` reales.
- El test del directorio sin permiso era **rojo garantizado si la suite corre como root** —`chmod` no impide escribir a root—; ahora tantea la escritura y sale **sin fallar, diciéndolo**.
- El test del esquema sólo miraba **nombres** de columna; ahora comprueba tipos, `NOT NULL` y valores por defecto contra el §3.
- El test de integración no ataba `main` a la base; ahora comprueba el fichero y las cuatro tablas que el binario deja.
- Faltaba variante para «un directorio que es un fichero».
- El DSN se parseaba **dos** veces.
- El test del `busy_timeout` dependía del reloj.
- El texto de una tarea no coincidía con lo declarado.

Más dos correcciones de contrato: el prefijo `V<NNN>__` que el spec exigía era **inconstruible**, y el traspaso del pool al `AppState` se **aplazó** al change de los endpoints.

### Módulo 3 — 9 hallazgos, dos de ellos altos

- **`OsRng::fill_bytes` entra en pánico si el `getrandom` subyacente falla** —un syscall bloqueado por `seccomp`, un kernel sin él—: el proceso moría en vez de devolver error y el cliente recibía la conexión cortada. Pasa a `try_fill_bytes`.
- **La invariante etiqueta↔prefijo se comprobaba con un `ends_with("v1")` hueco, sin *known-answer test*.** Cambiar la etiqueta, la sal o el algoritmo dejaba la suite **verde** y los secretos ya guardados **ilegibles**, con el síntoma apareciendo en producción, en el motor, hablando de S3. Ahora hay un candado literal de la etiqueta y un KAT calculado aparte según el RFC 5869.
- Y siete menores: la causa del base64 no llegaba al registro —su `Display` no la interpolaba—; el AAD ausente pasó de olvido a decisión documentada; el test del registro no ejecutaba el escenario que citaba; el de los dos cifrados no afirmaba que los textos cifrados difieran; el del nonce roto tiraba la longitud que la variante existe para llevar; la clave no borraba su material al caer, al revés que `SecretString`; y el mensaje del AEAD omitía el cifrado truncado.

### Módulo 2 — 3 altos y 6 medios

- La tarea que pide el `Debug` de un repositorio **y de un target** sólo probaba el repositorio.
- **El escenario «una fecha escrita por la aplicación vuelve como UTC» no tenía prueba:** se comparaba una fecha leída contra otra leída, así que una corrupción del camino de codificación habría desplazado a las dos por igual. De ahí salió la **normalización a UTC al escribir**.
- **El descifrado que falla no se probaba por donde ocurre:** los escenarios dicen «por la capa de acceso» y sólo se llamaba a la función de cifrado directamente, con la variante del error de cifrado como la única sin prueba. Ahora una fila con el sobre roto y otra con el valor en claro se escriben con **SQL directo** y se leen por la capa.
- Los medios: un secreto **vacío** se podía escribir; la variante de consulta pasó a llevar la sentencia **por tipo**; la segunda clave foránea de `backup_jobs` entró en la prueba de la huérfana; y el `false` de `actualizar`/`borrar` sin fila dejó de estar sólo en un doc-comment.

### El rojo tardío del módulo 2

Una primera pasada implementó las tres capas en el mismo encargo en que escribía sus pruebas, así que hubo que **reconstruir** el rojo devolviendo las capas a su forma sin implementar: `137 passed; 12 failed`, y las doce por «no implementado». La medida existe y está comprobada, pero **la secuencia no fue la del TDD limpio**. Al auditar no se encontró ninguna prueba escrita a la medida de la implementación.

### Versiones fijadas por la caché

El repositorio compila **sin red**, así que las versiones no son preferencia sino consecuencia:

- **`sqlx` 0.8.6** con `default-features = false`, y **no** 0.9.0. El `default` arrastra `any`, que mete los drivers de MySQL y PostgreSQL que el proyecto no usa; y 0.9.0 expande su feature `sqlite` —deja de ser un alias de «bundled»—, abre `libsqlite3-sys` a un rango que resuelve a 0.37.0 y exige MSRV 1.94.
- **Generación RustCrypto 0.10/0.12** —`aes-gcm` 0.10.3, `hkdf` 0.12.4, `sha2` 0.10.9—, que es la que **ya estaba en el árbol** por `jsonwebtoken` con `rust_crypto`. La generación nueva resuelve, pero mete **nueve bifurcaciones** nuevas.
- **`time` y no `chrono`**, porque `time` ya está en el árbol activo —lo traen `cookie` y `simple_asn1`—, así que `chrono` sería una segunda familia de fechas para lo mismo. En `sqlx` 0.8.6 `time` es dependencia opcional, así que la feature `time` de `sqlx` es la que enciende las impls `Encode`/`Decode`.
- **`base64` promovida** de `[dev-dependencies]` a `[dependencies]`, porque el sobre lo usa también en producción.

## Deuda pendiente y límites aceptados

- **El pool no está todavía en el `AppState`.** Se abre y se migra al arrancar y vive el resto del proceso, pero su traspaso al estado compartido es la **primera** tarea del change que estrene su primer consumidor —los endpoints del §5—. Adelantarlo obligaría a elegir entre un campo no opcional, que llevaría una base de datos a las más de treinta construcciones de `AppState` de las pruebas, y uno opcional, que dejaría escrito que un Kesto sin base es un estado válido, y no lo es.
- **Una violación de clave foránea colapsa hoy en un `5xx`.** El `4xx` lo decidirá el change de los endpoints; la causa se **conserva** —`sqlx::Error::Database` la lleva, con su `is_foreign_key_violation()`—.
- **La cobertura de los escenarios «genéricos» del contrato se hizo sobre `repositories`**, no tabla por tabla: las cuatro capas comparten la forma, y se probó la forma una vez.
- **Rotar `KESTO_MASTER_KEY` deja ilegibles los secretos ya guardados.** No hay clave de datos envuelta por una clave maestra rotable: sería un segundo sistema de claves que nadie ha pedido. Queda escrito para que quien rote sepa que la clave anterior hace falta para leer lo que ya está guardado.
- **El fichero `.db` no se cifra.** Lo que no aparece en claro son las tres columnas de secreto; el histórico de ejecuciones es legible **a propósito**, que es lo que hace útil abrir el fichero con `sqlite3`.
- **No hay `VACUUM` programado ni retención de `job_executions`.** El esquema lo soportará; quien borra por antigüedad es el planificador, y todavía no existe.
- **`listar` no tiene paginación ni filtros.**
- **Los ayudantes internos están copiados en las cuatro capas.**
- Las **tablas** las garantiza el `CHECK` de la base, pero el módulo de cifrado **sólo** protege las tres columnas declaradas: una columna de secreto nueva que nadie registre se guardaría en claro sin que nada avise.

Este documento es el hermano de `plans/PLAN-003-COMPLETED.md` y continúa a `docs/FEATURE-002-auth-capability.md`.
