# db Specification

## Purpose
La capa de datos del §3: la base SQLite abierta una sola vez al arrancar sobre el DSN de `DATABASE_URL` —con el fichero creado si falta y el arranque detenido, nombrando la ruta, si el directorio no existe—, con su pool y su esquema aplicado por migraciones embebidas en el binario, de modo que una base recién creada queda lista antes de aceptar la primera petición y arrancar dos veces sobre la misma base no cambia nada. Modela las cuatro tablas del §3 —`repositories`, `backup_targets`, `backup_jobs` y `job_executions`— con sus claves foráneas y sus cascadas `ON DELETE CASCADE`, y añade al SRS cuatro índices, entre ellos el que resuelve el filtrado de las ejecuciones por repositorio. Sobre ellas vive la capa de acceso, que da de alta, lee, lista, actualiza y borra cada fila con sentencias preparadas y que escribe y lee los identificadores en su forma canónica con guiones, para que el `TEXT` del §3 y el tipo de Rust no se mientan. Guarda, además, los tres secretos del §3 cifrados en reposo con el sobre `v1:<nonce>:<cifrado>` —AES-256-GCM, nonce aleatorio por valor y clave derivada de `KESTO_MASTER_KEY` con HKDF-SHA256 y la etiqueta constante `kesto/secrets/v1`—, de modo que ningún secreto en claro llega al fichero. Fuera de esta capability quedan el traspaso del pool al `AppState`, que estrena el change de los endpoints del §5 —su primer consumidor—, el `VACUUM` y la retención de `job_executions`.

## Requirements

### Requirement: La base se abre en el DSN de `DATABASE_URL` y el fichero se crea si no existe

La aplicación SHALL abrir la base de datos sobre `Config::database_url` —el valor de `DATABASE_URL`, con `sqlite:///app/data/kesto.db` como defecto del SRS— al arrancar, **una sola vez**, antes de aceptar peticiones, y el pool resultante SHALL vivir el resto del proceso y ser **el único** que la aplicación use: NO SHALL abrirse una segunda base por consumidor.

Ese pool SHALL entregarse al estado de la aplicación —el `AppState`—, y ese traspaso NO SHALL hacerse en este change, sino en el que estrene su primer consumidor —los endpoints del §5—, como su **primera** tarea. Hoy no hay ninguno, y adelantarlo obligaría a elegir entre un campo **no opcional** —que llevaría una base de datos a las más de treinta construcciones de `AppState` de las pruebas— o uno **opcional**, que dejaría escrito en el tipo que un Kesto sin base de datos es un estado posible, y no lo es: sin base, el panel no arranca. Aplazarlo no relaja nada de lo anterior: el pool se abre una vez, sobrevive al proceso y es el que los consumidores recibirán.

Si el fichero de la base no existe, SHALL crearse: una base embebida que exige un `touch` previo es una base que no arranca la primera vez.

Si **el directorio** del DSN no existe, el arranque SHALL fallar en alto con un mensaje que **nombre la ruta absoluta** del directorio que falta, y NO SHALL crearlo en silencio. Crear el directorio escondería un volumen mal montado —el del `compose` del §8 es precisamente el directorio de la base—, y el fallo se manifestaría más tarde y en otro sitio; nombrar la ruta convierte un «no arranca» en un «falta montar esto». El arranque NO SHALL delegar el diagnóstico en el error crudo de la biblioteca, que se limita a decir que no pudo abrir el fichero.

El `DATABASE_URL` SHALL aceptar la forma `sqlite://<ruta>` del SRS, que para una ruta absoluta lleva **tres** barras (`sqlite:///app/data/kesto.db`): la lectura del DSN NO SHALL tratar la tercera barra como parte del nombre del fichero.

#### Scenario: El fichero de la base se crea si no existe
- **GIVEN** un `DATABASE_URL` que apunta a un fichero que no existe, en un directorio que sí existe
- **WHEN** la aplicación arranca
- **THEN** el arranque SHALL completarse
- **AND** el fichero de la base SHALL existir después

#### Scenario: Un directorio inexistente detiene el arranque nombrando la ruta
- **GIVEN** un `DATABASE_URL` cuyo directorio no existe
- **WHEN** la aplicación arranca
- **THEN** SHALL fallar el arranque
- **AND** el mensaje SHALL contener la ruta del directorio que falta
- **AND** NO SHALL crearse el directorio

#### Scenario: El DSN por defecto del SRS se interpreta con tres barras
- **GIVEN** una `DATABASE_URL` sin definir en el entorno
- **WHEN** se lee la configuración y se resuelve la ruta del fichero
- **THEN** la ruta SHALL ser `/app/data/kesto.db`
- **AND** el nombre del fichero NO SHALL quedar con una barra de más

### Requirement: El esquema se aplica con migraciones embebidas al arrancar

El esquema SHALL aplicarse con migraciones **embebidas en el binario** mediante `sqlx::migrate!()`, versionadas en `migrations/` con un **prefijo numérico entero** (`0001_init.sql`, `0002_indexes.sql`), al arrancar y antes de aceptar peticiones. NO SHALL aplicarse con SQL ejecutado suelto al abrir la conexión: un esquema sin versión no se puede cambiar después.

La forma con `V` —`V001__init.sql`, la del estilo de Flyway— NO SHALL usarse: `sqlx` parte el nombre por el primer `_` y exige que el prefijo sea un entero, así que `V001` no parsea y `sqlx::migrate!()` **falla al compilar** con `error parsing migration filename "V001__init.sql"; expected integer version prefix (e.g. `01_foo.sql`)`. Un esquema que no compila no llega a arrancar, y el fallo aparece en el sitio equivocado —`cargo check`, no el arranque— si nadie lo sabe.

`sqlx::migrate!()` resuelve su directorio **en tiempo de compilación**, así que `migrations/` tiene que existir **antes del primer `cargo check`** que incluya la macro: si falta, el fallo es de compilación y no de arranque, y quien no lo sepa buscará el error en el sitio equivocado.

Las migraciones aplicadas SHALL ser **idempotentes**: arrancar dos veces sobre la misma base NO SHALL cambiar el esquema ni volver a aplicar una migración ya aplicada.

Un fallo de migración SHALL ser **error de arranque**, y NO SHALL dejar el proceso sirviendo en un modo degradado. Un esquema a medias es peor que un proceso que no arranca: el motor escribiría contra columnas que no existen y el fallo aparecería como una copia perdida en vez de como un arranque fallido.

#### Scenario: Un arranque sobre una base nueva deja el esquema aplicado
- **GIVEN** una base recién creada
- **WHEN** la aplicación arranca
- **THEN** las cuatro tablas del §3 SHALL existir
- **AND** SHALL existir la tabla de control de migraciones

#### Scenario: Arrancar dos veces no cambia el esquema
- **GIVEN** una base en la que la aplicación ya arrancó una vez
- **WHEN** la aplicación arranca por segunda vez
- **THEN** el esquema SHALL ser idéntico al de la primera vez
- **AND** NO SHALL aplicarse ninguna migración de nuevo

#### Scenario: Una migración que falla impide arrancar
- **GIVEN** una base cuyo esquema impide aplicar una migración
- **WHEN** la aplicación arranca
- **THEN** SHALL fallar el arranque
- **AND** el proceso NO SHALL quedarse sirviendo peticiones

### Requirement: Los pragmas de SQLite se fijan al abrir la conexión

La base SHALL abrirse con `journal_mode=WAL`, con `busy_timeout` configurado y con las claves foráneas **encendidas**.

`foreign_keys` SHALL activarse en las **opciones de conexión del pool** —`SqliteConnectOptions::foreign_keys(true)`—, y NO SHALL fijarse con un `PRAGMA foreign_keys=ON` suelto: es una opción **por conexión** y está **desactivada por defecto** en SQLite, así que un `PRAGMA` ejecutado una vez sobre una conexión del pool solo la afecta a ella. Las demás seguirían sin claves foráneas, el `ON DELETE CASCADE` del §3 no ocurriría, y nada lo delataría: el borrado simplemente no borraría. Declararlo en las opciones es lo que hace que **toda** conexión del pool lo cumpla.

`journal_mode=WAL` SHALL activarse porque permite lecturas concurrentes mientras hay una escritura: el motor registra el progreso de una copia mientras el panel lee el listado de tareas. Es una propiedad del **fichero** y no de la conexión, así que se activa una vez y persiste.

WAL SHALL convivir con ficheros `-wal` y `-shm` **junto a la base**, así que **el directorio de la base SHALL ser escribible**. Es un requisito del volumen del `compose` del §8, que llega en un change posterior, y queda escrito aquí porque es el pragma quien lo impone.

`busy_timeout` SHALL fijarse a un valor finito: SQLite serializa las escrituras y sin espera una escritura que coincide con otra **falla** en vez de esperar.

#### Scenario: El journaling queda en WAL
- **GIVEN** una base abierta por la aplicación
- **WHEN** se consulta el modo de journaling efectivo
- **THEN** SHALL ser `wal`
- **AND** SHALL existir el fichero `-wal` junto a la base mientras hay una conexión abierta

#### Scenario: Las claves foráneas están encendidas en toda conexión del pool
- **GIVEN** un pool abierto por la aplicación con más de una conexión
- **WHEN** se comprueba el estado de las claves foráneas en cada conexión del pool
- **THEN** SHALL estar activadas en todas
- **AND** NO SHALL depender de un `PRAGMA` ejecutado sobre una sola conexión

#### Scenario: WAL exige un directorio escribible
- **GIVEN** un directorio de base de datos sin permiso de escritura
- **WHEN** la aplicación intenta abrir la base con WAL
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar la ruta de la base

#### Scenario: Una escritura que coincide con otra espera en lugar de fallar
- **GIVEN** una base con una escritura en curso y el `busy_timeout` configurado
- **WHEN** otra conexión intenta escribir
- **THEN** SHALL esperar hasta que la primera termine
- **AND** NO SHALL devolver un error de base ocupada antes de agotar la espera

### Requirement: Las cuatro tablas del §3 se reproducen exactas, con sus `CHECK`, sus cascadas y los índices añadidos

Las migraciones SHALL declarar las cuatro tablas del §3 —`repositories`, `backup_targets`, `backup_jobs` y `job_executions`— con las columnas, los tipos y los valores por defecto de ese apartado, incluidos los `CHECK` de enumerado `target_type IN ('mariadb', 'postgresql', 'directory', 'sqlite')` y `status IN ('pending', 'running', 'success', 'failed')`, y las tres claves foráneas `ON DELETE CASCADE`. `backup_targets` SHALL incluir además la columna `db_path` (`TEXT`, anulable), que junto a `container_name` describe una fuente `sqlite`. `backup_jobs` SHALL incluir además la columna `name` (`TEXT`, **no nula**, con `UNIQUE(name)`), declarada aquí **como un añadido al SRS** —el §3 no la tiene en su versión original—: un UUID no identifica la tarea al operador, y el nombre es lo que hace legible el histórico y los snapshots. La creó anulable la `0005` y la hizo obligatoria y única la `0007`; este apartado fija el **estado final**, y el escenario que describe el estado intermedio lo dice en su propio `GIVEN`. La forma exacta de esa columna la fija el requisito «La obligatoriedad y la unicidad del nombre reconstruyen `backup_jobs`, y la reconstrucción no se lleva ninguna fila».

Las restricciones SHALL estar **en la base** y no solo en Rust: un `INSERT` hecho con `sqlite3` a mano tiene que chocar con el mismo `CHECK` que choca la aplicación. Es la única forma de que el enumerado sea una garantía y no una convención.

Además de lo que el SRS declara, SHALL crearse **cuatro índices**, que el §3 no tiene y que **se declaran aquí como un añadido al SRS**:

- `idx_backup_jobs_target_id` — columna hija de la FK a `backup_targets`
- `idx_backup_jobs_repository_id` — columna hija de la FK a `repositories`
- `idx_job_executions_job_id` — columna hija de la FK a `backup_jobs`
- `idx_job_executions_status_started_at` — para el listado del dashboard, por estado y fecha

Los tres primeros SHALL existir porque **SQLite no crea índices para las claves foráneas**: sin índice en la columna hija, cada borrado en cascada recorre la tabla entera, y el SRS declara tres cascadas sin pagar ninguno de los tres recorridos. El cuarto cubre el listado del §6: sin él, la consulta del panel ordena recorriendo.

Filtrar las ejecuciones **por repositorio** SHALL resolverse con el `JOIN` por `backup_jobs` apoyado en los índices anteriores, y NO SHALL añadirse una columna `repository_id` a `job_executions`: duplicaría un dato que ya vive en la tarea y crearía una segunda verdad que se puede desincronizar.

Los índices SHALL nombrarse `idx_{tabla}_{columna}`. Una reconstrucción de la tabla SHALL recrearlos: el `DROP TABLE` de la vieja se los lleva con ella.

#### Scenario: Las cuatro tablas existen con sus columnas
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se enumeran las tablas y sus columnas
- **THEN** SHALL existir `repositories`, `backup_targets`, `backup_jobs` y `job_executions`
- **AND** sus columnas SHALL ser las del §3, más `backup_targets.db_path` y `backup_jobs.name`

#### Scenario: Los enumerados están garantizados por un `CHECK`
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se intenta insertar un `target_type` o un `status` fuera de su enumerado, con SQL directo
- **THEN** la inserción SHALL fallar
- **AND** NO SHALL depender de que la validación la haya hecho la aplicación

#### Scenario: El `CHECK` de `target_type` admite `sqlite`
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se inserta un `target_type = 'sqlite'` con SQL directo
- **THEN** la inserción SHALL aceptarse
- **AND** un `target_type = 'sqlite3'` SHALL rechazarse

#### Scenario: Cada clave foránea tiene su índice
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se consultan los índices declarados
- **THEN** SHALL existir `idx_backup_jobs_target_id`, `idx_backup_jobs_repository_id` y `idx_job_executions_job_id`
- **AND** el plan de una consulta que borra una fila padre NO SHALL recorrer la tabla hija entera

#### Scenario: El listado de ejecuciones tiene su índice
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se planifica el listado de ejecuciones ordenado por fecha y filtrado por estado
- **THEN** el plan SHALL usar `idx_job_executions_status_started_at`
- **AND** NO SHALL recorrer la tabla entera

#### Scenario: Filtrar por repositorio no necesita una columna duplicada
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se planifica el `JOIN` de `job_executions` con `backup_jobs` filtrando por `repository_id`
- **THEN** el plan SHALL apoyarse en los índices de clave foránea
- **AND** `job_executions` NO SHALL tener columna `repository_id`

#### Scenario: La columna `name` de `backup_jobs` existe y es anulable
- **GIVEN** una base llevada a mano hasta la `0005` —el estado intermedio que la `0007` tiene que migrar, no el final—
- **WHEN** se consulta la tabla `backup_jobs`
- **THEN** SHALL existir la columna `name` de tipo `TEXT`
- **AND** SHALL admitir `NULL` en una fila insertada con SQL directo `INSERT ... (id, target_id, repository_id, cron_expression) VALUES (...)` sin especificarla

#### Scenario: La columna `name` de `backup_jobs` es obligatoria y única
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se consulta la tabla `backup_jobs` y se intenta insertar una fila con SQL directo sin `name`, o con un `name` que ya tiene otra fila
- **THEN** SHALL existir la columna `name` de tipo `TEXT`
- **AND** su nulabilidad SHALL ser `NOT NULL`
- **AND** las dos inserciones SHALL fallar por la restricción de la base

### Requirement: Los identificadores son UUIDv4 en `TEXT` y las fechas son UTC

La columna `id` de las cuatro tablas SHALL contener un UUIDv4 **en forma canónica con guiones**, almacenado como `TEXT`, tal y como declara el §3.

El mapeo directo de la columna a `sqlx::types::Uuid` NO SHALL usarse. `sqlx-sqlite` codifica `Uuid` como un **BLOB de 16 bytes** y lo descodifica con `Uuid::from_slice`: un `TEXT` de 36 caracteres **no descodifica** como `Uuid`, y un `BLOB` de 16 bytes deja los identificadores ilegibles para `sqlite3` y para DB Browser. La capa de acceso SHALL escribir y leer la forma con guiones —`uuid::fmt::Hyphenated`—, y así el `TEXT` del §3 y el tipo de Rust conviven sin mentirse.

El identificador SHALL generarse **en la capa de acceso** al dar de alta una fila, no en la base: SQLite no tiene `uuid()` y un identificador por defecto en el DDL obligaría a interpolarlo.

Las fechas SHALL ser **UTC** y SHALL escribirse en RFC3339, que es lo que produce el mapeo de `time::OffsetDateTime`. El `DEFAULT CURRENT_TIMESTAMP` del §3 SHALL mantenerse tal cual, y NO SHALL cambiarse por una expresión con formato propio: la impl de `OffsetDateTime` de `sqlx-sqlite` descodifica además el formato `YYYY-MM-DD HH:MM:SS` que produce `CURRENT_TIMESTAMP`, asumiéndolo UTC, así que las filas insertadas a mano con `sqlite3` siguen siendo legibles.

El tipo de fecha de las entidades SHALL ser `time::OffsetDateTime`. `chrono` NO SHALL añadirse: `time` ya está en el árbol activo —lo traen `cookie` y `simple_asn1`—, así que `chrono` sería una segunda familia de fechas para representar lo mismo. En `sqlx` 0.8.6 `time` es una dependencia **opcional** de `sqlx-core` y de `sqlx-sqlite`, así que la feature `time` de `sqlx` SHALL declararse: es la que enciende las impls `Encode`/`Decode`, y sin ella las columnas de fecha no se mapean.

`BOOLEAN` SHALL escribirse y leerse como entero 0/1: SQLite no tiene tipo booleano y la afinidad `NUMERIC` del §3 guarda enteros.

#### Scenario: El identificador se guarda como texto canónico
- **GIVEN** un alta hecha por la capa de acceso
- **WHEN** se lee la columna `id` de esa fila
- **THEN** SHALL ser `TEXT` con el UUID en forma canónica con guiones
- **AND** NO SHALL ser un BLOB de 16 bytes

#### Scenario: El identificador se lee de vuelta sin ambigüedad
- **GIVEN** una fila insertada con su identificador textual
- **WHEN** se lee la entidad por su id
- **THEN** el identificador SHALL ser el mismo UUID
- **AND** SHALL conservar la forma canónica con guiones

#### Scenario: Una fecha escrita por la aplicación vuelve como UTC
- **GIVEN** una fila insertada por la capa de acceso con una fecha y hora conocidas
- **WHEN** se lee esa fila
- **THEN** la fecha SHALL ser la misma
- **AND** su desplazamiento SHALL ser cero

#### Scenario: El `DEFAULT CURRENT_TIMESTAMP` del SRS sigue siendo legible
- **GIVEN** una fila insertada con `sqlite3` a mano, sin informar `created_at`, de modo que la columna toma el valor de `CURRENT_TIMESTAMP`
- **WHEN** la capa de acceso lee esa fila
- **THEN** SHALL descodificarla como la fecha UTC que declara el texto
- **AND** NO SHALL fallar la lectura

#### Scenario: `BOOLEAN` es un entero 0/1
- **GIVEN** un alta con `enabled` a verdadero
- **WHEN** se lee el valor almacenado en la columna
- **THEN** SHALL ser el entero `1`
- **AND** al leerlo como booleano SHALL ser verdadero

### Requirement: Cada tabla tiene su entidad y su capa de acceso

SHALL existir una entidad y una capa de acceso por cada una de las cuatro tablas del §3: `repositories`, `backup_targets`, `backup_jobs` y `job_executions`.

Cada capa de acceso SHALL ofrecer el alta, la lectura por identificador, el listado, la actualización y el borrado de sus filas. La lectura por un identificador que no existe SHALL ser un resultado que distingue «no está» de «no se pudo leer», y NO SHALL confundirse con un error de base.

Todas las consultas SHALL usar **prepared statements** con las columnas enlazadas, y los valores de usuario NO SHALL interpolarse en el texto de la consulta en ningún caso, ni siquiera cuando la consulta se construye en varias piezas.

El borrado SHALL propagarse según lo que el §3 declara con `ON DELETE CASCADE`, y la cascada SHALL ocurrir **de verdad**, con las claves foráneas encendidas en la conexión que la ejecuta. El escenario que lo comprueba SHALL recorrer los **tres** niveles: borrar un target deja sin tareas sus `backup_jobs`, y con ellas desaparecen sus `job_executions`.

Los errores de la capa de acceso SHALL ser variantes tipadas y SHALL conservar la causa, que va al registro del servidor; el mensaje que llega al cliente NO SHALL publicar el texto de la consulta ni el valor de una columna de secreto.

#### Scenario: Alta y lectura por identificador
- **GIVEN** una capa de acceso sobre una base migrada
- **WHEN** se da de alta un repositorio y se lee por su identificador
- **THEN** la fila leída SHALL tener los mismos valores que se dieron de alta
- **AND** el identificador NO SHALL haber sido informado por quien llamó si la capa lo genera

#### Scenario: Leer por un identificador que no existe no es un error de base
- **GIVEN** una base migrada sin filas
- **WHEN** se lee por un identificador inventado
- **THEN** el resultado SHALL indicar que no está
- **AND** NO SHALL ser un error de base de datos

#### Scenario: El listado devuelve las filas dadas de alta
- **GIVEN** tres filas dadas de alta en la misma tabla
- **WHEN** se pide el listado
- **THEN** SHALL devolver las tres
- **AND** su orden SHALL ser estable entre dos llamadas seguidas

#### Scenario: La actualización cambia la fila
- **GIVEN** una fila dada de alta
- **WHEN** se actualiza uno de sus campos
- **THEN** la lectura posterior SHALL devolver el valor nuevo
- **AND** los campos no tocados SHALL conservar su valor

#### Scenario: El borrado quita la fila
- **GIVEN** una fila dada de alta
- **WHEN** se borra por su identificador
- **THEN** la lectura posterior SHALL indicar que no está
- **AND** el borrado SHALL haberse aplicado sin error

#### Scenario: Borrar un target arrastra sus tareas y sus ejecuciones
- **GIVEN** un repositorio, un target, dos tareas de ese target y una ejecución de cada tarea
- **WHEN** se borra el target
- **THEN** sus `backup_jobs` NO SHALL existir
- **AND** las `job_executions` de esas tareas NO SHALL existir
- **AND** el repositorio NO SHALL haberse borrado

#### Scenario: Borrar un repositorio arrastra sus tareas y sus ejecuciones
- **GIVEN** un repositorio, un target, una tarea y una ejecución
- **WHEN** se borra el repositorio
- **THEN** la tarea NO SHALL existir
- **AND** su ejecución NO SHALL existir
- **AND** el target NO SHALL haberse borrado

#### Scenario: Una inserción huérfana se rechaza
- **GIVEN** una base migrada
- **WHEN** se intenta dar de alta una tarea que apunta a un target inexistente
- **THEN** SHALL fallar con un error tipado
- **AND** NO SHALL quedar ninguna fila huérfana en la tabla

#### Scenario: Un valor con comillas no rompe la consulta
- **GIVEN** un alta cuyo nombre contiene comillas simples y la cadena `); DROP TABLE backup_jobs; --`
- **WHEN** se da de alta y se lee de vuelta
- **THEN** el nombre SHALL llegar íntegro
- **AND** las cuatro tablas SHALL seguir existiendo

#### Scenario: El error no publica la consulta ni el secreto
- **GIVEN** un fallo de base provocado por una violación de restricción sobre una tabla con columnas de secreto
- **WHEN** se traduce el error a la respuesta de la API
- **THEN** el cuerpo SHALL llevar el mensaje genérico de los fallos del servidor
- **AND** NO SHALL contener el texto de la consulta ni el valor de ninguna columna de secreto

### Requirement: `db_name` informado significa dump de esa sola base

`backup_targets.db_name` SHALL tener la semántica que el §3 documenta y que este esquema fija: **informado** significa que la copia cubre **esa sola base**; **`NULL`** significa dump global.

La columna SHALL ser anulable y NO SHALL llevar valor por defecto: `NULL` no es un valor que haya que adivinar, es la ausencia de la restricción.

La semántica se fija **en el esquema** aunque quien la ejecute sea el motor, porque es la columna la que la define, y quien lea la tabla tiene que saber qué significa un `NULL` sin buscar el change del motor.

Queda escrito, además, que el §4.A **tal cual está es inaplicable con `db_name` informado**: sus dos comandos —`mariadb-dump --all-databases` y `pg_dumpall`— vuelcan el servidor entero e ignoran la columna. El change del motor tendrá que pasar a `mariadb-dump <db>` y a `pg_dump -d <db> -U <user>` cuando `db_name` no sea `NULL`. Aquí no se ejecuta ningún comando: se declara qué significa la columna para que quien lo haga no tenga que decidirlo.

Para un target de tipo `directory`, `db_name` SHALL quedar a `NULL`: no hay base que acotar.

#### Scenario: Un target con `db_name` acota la copia a esa base
- **GIVEN** un target de tipo `mariadb` con `db_name` informado
- **WHEN** se lee la configuración del target
- **THEN** `db_name` SHALL devolverse informado
- **AND** la copia de ese target SHALL entenderse acotada a esa sola base

#### Scenario: Un target sin `db_name` es dump global
- **GIVEN** un target de tipo `postgresql` con `db_name` a `NULL`
- **WHEN** se lee la configuración del target
- **THEN** `db_name` SHALL ser `NULL`
- **AND** la copia de ese target SHALL entenderse global

#### Scenario: `NULL` se distingue de la cadena vacía
- **GIVEN** un alta con `db_name` a `NULL`
- **WHEN** se lee la fila
- **THEN** el valor SHALL ser `NULL`
- **AND** NO SHALL leerse como una cadena vacía

#### Scenario: Un target de directorio no lleva base
- **GIVEN** un target de tipo `directory`
- **WHEN** se lee la fila
- **THEN** `db_name` SHALL quedar a `NULL`
- **AND** `container_name`, `db_user` y `db_password` NO SHALL ser necesarios

### Requirement: El pool se entrega al estado de la aplicación y es el único de la instancia

El pool abierto y migrado al arrancar SHALL entregarse al estado de la aplicación, que SHALL compartirlo con todos los manejadores; NO SHALL abrirse un segundo pool, y el campo del estado NO SHALL declararse **opcional**.

El aplazamiento fue deliberado y está escrito en el requisito «La base se abre en el DSN de `DATABASE_URL` y el fichero se crea si no existe» de esta capability: `db-schema` dejó el traspaso fuera porque, cuando se escribió, **no había ningún consumidor** —la capa de acceso estaba escrita, probada y auditada, y no la alcanzaba nadie desde el proceso—, y adelantarlo obligaba a elegir entre un campo **no opcional**, que llevaría una base de datos a los **16** sitios que construyen un `AppState` —once en pruebas y cinco fuera, cuatro en `src/state.rs` y uno en `src/main.rs`— y a los **61** puntos que llaman a los siete ayudantes del arnés, que pasan a ser asíncronos porque construir el estado exige una base abierta, y uno **opcional**, que dejaría escrito en el tipo que un Kesto sin base de datos es un estado posible. No lo es: sin base, el panel no arranca y no hay ni una fila que servir.

Aquel texto lo dejó como la **primera tarea del change que estrenara su primer consumidor**. Éste es ese change, y el consumidor son los manejadores del §5, así que la elección ya está tomada por la vía que la propia spec de `db` dejó abierta: el campo es **no opcional** y se lleva la base a las construcciones de estado de las pruebas, que pasan a construirse sobre una base migrada en memoria o en un temporal.

Que sea **el único** pool de la instancia no es una preferencia de estilo: SQLite admite varias conexiones al mismo fichero, así que un segundo pool no falla al abrirse —funciona— y el daño aparece más tarde y de forma intermitente, como una fila que un manejador acaba de escribir y otro no ve. Y hay un motivo de configuración: los pragmas que la base necesita —`foreign_keys` **por conexión**, WAL y el `busy_timeout`— se fijan en las opciones de conexión de **este** pool; un pool abierto por otro camino tendría que acordarse de fijarlos, y el `ON DELETE CASCADE` del §3 dejaría de ocurrir sin que nada lo delatara.

#### Scenario: El estado lleva el pool
- **GIVEN** el estado construido al arrancar la aplicación
- **WHEN** se consulta el estado
- **THEN** SHALL llevar el pool abierto y migrado al arrancar
- **AND** el campo NO SHALL ser opcional

#### Scenario: La escritura y la lectura ven la misma fila
- **GIVEN** el estado con su pool
- **WHEN** se escribe una fila por el estado y después se lee **con SQL directo** sobre el mismo fichero de la base
- **THEN** la fila SHALL estar
- **AND** SHALL tener los valores escritos

#### Scenario: Arrancar no abre una segunda base
- **GIVEN** un `DATABASE_URL` que apunta a un fichero conocido
- **WHEN** la aplicación arranca y el estado queda construido
- **THEN** SHALL existir **un** pool sobre ese fichero
- **AND** NO SHALL haberse abierto un segundo fichero ni un segundo pool sobre el mismo

### Requirement: La ampliación del enumerado `target_type` reconstruye la tabla, porque SQLite no altera un `CHECK` en sitio

La migración `0003` SHALL añadir `sqlite` al `CHECK` de `target_type` y la columna `db_path`, y SHALL hacerlo **reconstruyendo** `backup_targets` —crear la tabla nueva con el `CHECK` y la columna, copiar las filas, borrar la vieja y renombrar—, porque SQLite no permite modificar un `CHECK` existente con `ALTER TABLE`. La reconstrucción SHALL preservar todas las filas existentes, con `db_path` a `NULL`, y SHALL recrear los índices de la tabla. El manejo de `foreign_keys` durante la reconstrucción SHALL ser correcto, de modo que la operación no dispare cascadas ni deje filas huérfanas.

El porqué: la tabla del §3 fue declarada con el enumerado cerrado y la feature lo amplía con un valor más. Rehacer la tabla es el procedimiento canónico de SQLite para cambiar un `CHECK`; hacerlo con cuidado —dentro de la propia migración, sin pérdida de filas y con las claves foráneas bajo control— es lo único que mantiene la garantía de la base intacta.

#### Scenario: La migración conserva las filas existentes
- **GIVEN** una base con targets `mariadb`, `postgresql` y `directory` ya escritos
- **WHEN** se aplica la migración `0003`
- **THEN** todas las filas SHALL seguir ahí, con las mismas columnas de antes
- **AND** su `db_path` SHALL ser `NULL`

#### Scenario: Los índices sobreviven a la reconstrucción
- **GIVEN** una base migrada a `0003`
- **WHEN** se consultan los índices de `backup_targets` y de las tablas con FK a ella
- **THEN** los índices declarados SHALL seguir existiendo
- **AND** las claves foráneas que apuntan a `backup_targets` NO SHALL quedar rotas

### Requirement: El esquema registra las ejecuciones de restauración

La migración `0006_restauraciones.sql` SHALL crear la tabla `restore_executions` con: `id` (`TEXT`, clave primaria, UUID en su forma canónica con guiones), `snapshot_id` (`TEXT`, no nulo), `target_id` (`TEXT`, anulable, clave foránea a `backup_targets(id)` con `ON DELETE CASCADE`), `status` (`TEXT`, no nulo, `CHECK (status IN ('pending','running','success','failed'))`, por defecto `'pending'`), `error_message` (`TEXT`, anulable), `started_at` (`TEXT`, no nulo) y `finished_at` (`TEXT`, anulable). SHALL declarar además los índices `idx_restore_executions_status_started_at`, para el listado por estado y fecha, y `idx_restore_executions_target_id`, columna hija de la clave foránea y apoyo del filtro `target_id` del listado, por coherencia con el resto del esquema.

La tabla SHALL declararse **como un añadido al SRS** —el §3 modela la copia, no la restauración—, y la migración SHALL seguir la forma de nombre con prefijo numérico entero que exige `sqlx::migrate!()`. La capa de acceso SHALL escribir y leer los identificadores en su forma canónica con guiones, como en el resto del esquema.

`target_id` SHALL ser anulable y la clave foránea SHALL ser `ON DELETE CASCADE`, de modo que borrar un target borre su histórico de restauraciones, coherente con el resto del esquema.

El porqué: sin registro, el `execution_id` que hoy devuelve el `200` no dice nada después —no se sabe si la restauración acabó, ni cuándo, ni sobre qué—, y el operador no tiene histórico. La ejecución de restauración merece la misma trazabilidad que la de copia.

#### Scenario: La tabla existe tras migrar

- **GIVEN** una base recién migrada
- **WHEN** se consultan las tablas y sus índices
- **THEN** SHALL existir `restore_executions` con sus columnas, su `CHECK` de `status` y su clave foránea a `backup_targets`
- **AND** SHALL existir `idx_restore_executions_status_started_at` e `idx_restore_executions_target_id`

#### Scenario: El estado no admite un valor fuera del enumerado

- **GIVEN** la tabla `restore_executions`
- **WHEN** se intenta escribir un `status` distinto de `pending`, `running`, `success` o `failed`
- **THEN** la escritura SHALL rechazarse por el `CHECK`

#### Scenario: Borrar el target borra su histórico de restauraciones

- **GIVEN** una restauración ligada a un target
- **WHEN** se borra ese target
- **THEN** su fila en `restore_executions` NO SHALL existir

### Requirement: La obligatoriedad y la unicidad del nombre reconstruyen `backup_jobs`, y la reconstrucción no se lleva ninguna fila

La migración `0007_nombre_de_tarea_obligatorio.sql` SHALL declarar `backup_jobs.name` como `TEXT NOT NULL`, SHALL declarar la restricción `UNIQUE(name)` y SHALL hacerlo **reconstruyendo** la tabla —crear la nueva con la columna y la restricción, copiar las filas, borrar la vieja y renombrar—, porque SQLite no puede reescribir con `ALTER TABLE` la nulabilidad de una columna que ya existe —el `ADD COLUMN` de la `0005` la declaró anulable— ni añadir una restricción de tabla a una tabla existente. El nombre del fichero SHALL llevar el prefijo numérico entero que exige `sqlx::migrate!()`.

La reconstrucción SHALL copiar **columna a columna, nombradas** —no con un `SELECT *`, que dependería del orden de la tabla vieja—, SHALL recrear los dos índices de la tabla, `idx_backup_jobs_target_id` e `idx_backup_jobs_repository_id`, que el `DROP TABLE` se lleva con la tabla vieja, y SHALL declarar `name` en la **última** posición, como la dejó la `0005`, para que el orden que publica `PRAGMA table_info` no cambie bajo los pies de nadie.

`backup_jobs` es **padre** de `job_executions`: su `DROP TABLE` con las claves foráneas encendidas dispara el `ON DELETE CASCADE` y se lleva por delante las ejecuciones de **todas** las tareas —y con ellas el histórico de copias— antes de que esta migración haya copiado nada, sin que nada lo delate. La migración SHALL correr con `foreign_keys` **apagado**, como corre el resto del migrador (`db::migrate::aplicar`, que lo apaga en la conexión y fuera de transacción), y la comprobación posterior del migrador SHALL confirmar que no quedaron filas huérfanas. Ninguna fila de ninguna tabla SHALL perderse por la reconstrucción.

Las filas que ya existían con `name` a `NULL` SHALL rellenarse **dentro de la propia migración** con un nombre derivado, porque `NOT NULL` no admite un `NULL` preexistente y una migración que dejara filas ilegibles no serviría de nada. El derivado SHALL ser único entre **todas** las filas de la tabla —no sólo entre las rellenadas—, NO SHALL coincidir con ningún nombre que ya estuviera presente, NO SHALL ser el identificador de la tarea ni una copia de él, SHALL quedar no vacío tras recortar y SHALL medir **120** caracteres o menos. La forma del derivado SHALL ser `tarea <octeto corto del id>`: el literal `tarea `, un espacio y el **primer octeto** del identificador de la tarea en minúsculas —los ocho primeros dígitos hexadecimales, no el UUID entero—, de donde salen 14 caracteres; SHALL derivarse en SQL general, aplicable a cualquier fila sin nombre y no sólo a las de este entorno, y NO SHALL chocar con un nombre ya presente: si el derivado de una fila coincidiera con un nombre que ya estaba, la migración SHALL usar el siguiente tramo del identificador hasta quedar libre. El detalle de qué tramo se usa ante ese choque es una elección de la implementación; lo que el contrato fija es que el resultado es único, no vacío, distinto del identificador y de 120 caracteres o menos.

Las filas que ya tenían un nombre SHALL **conservarlo**, y el nombre pasa a ser una identidad de la tarea: dos tareas distintas NO SHALL poder compartirlo. El contrato anterior era explícitamente **no único** —el borde HTTP no comprobaba duplicados—, así que una base puede tener homónimos y la migración NO SHALL abortar por ellos: de cada grupo de filas que comparten nombre, la de **menor `rowid`** SHALL conservar el suyo tal cual y las demás SHALL recibir un nombre desambiguado con el sufijo ` (2)`, ` (3)`, … —añadido al nombre tal y como estaba, de modo que uno que ya acabe en `(N)` se desambigua añadiendo otro—, avanzando hasta dar con uno **libre en toda la tabla** y no sólo entre las filas de su grupo. La desambiguación SHALL ser determinista: el mismo `rowid` produce el mismo resultado. Un `name` **vacío o en blanco** SHALL tratarse como **ausente** y recibir el nombre derivado: el `CHECK` de no-vacío de la columna nueva no lo admitiría, y la migración no puede abortar por una fila que la aplicación no puede escribir. La unicidad SHALL ser sensible a mayúsculas —la colación `BINARY` de SQLite, el defecto sin colación declarada—, de modo que lo que la base rechaza es exactamente lo que el borde HTTP contesta `409`.

La columna SHALL declarar además `CHECK (length(trim(name)) > 0)`, y esta parte es la **decisión D7 del proposal, aprobada**: un nombre en blanco es una fila sin valor, y el momento de cerrarle la puerta en la base es **éste**, porque sólo se reconstruye una vez.

El §3 del SRS SHALL declarar la columna en su forma nueva —`name TEXT NOT NULL` con `UNIQUE(name)`—: sigue siendo un añadido al SRS, y esta migración actualiza su forma. El tope de **120** caracteres NO SHALL declararse como `CHECK`: lo impone el borde HTTP, único que puede contestar un `400` nombrando el campo, como decidió la `0005`.

El porqué de reconstruir en vez de parchear: es el procedimiento canónico de SQLite para lo que `ALTER TABLE` no sabe hacer, ya usado por la `0003` para ampliar un `CHECK`, y el único camino que convierte «el nombre es obligatorio y único» en una garantía de la base en vez de una convención de la aplicación. Y el porqué del relleno: una tarea sin nombre es un estado **anterior** a esta migración, no un estado válido después de ella; conservar las filas rellenándolas es lo que permite exigir el nombre sin borrar el histórico de nadie.

#### Scenario: La migración deja la columna obligatoria y única

- **GIVEN** una base con las migraciones aplicadas hasta la `0007`
- **WHEN** se consulta `PRAGMA table_info(backup_jobs)` y se intentan dos inserciones directas —una sin `name` y otra con un `name` repetido—
- **THEN** la columna `name` SHALL ser `NOT NULL`
- **AND** SHALL existir un índice único sobre `name`
- **AND** las dos inserciones SHALL fallar

#### Scenario: Las tareas que no tenían nombre se rellenan con nombres únicos

- **GIVEN** una base llevada a mano hasta la `0006` con dos tareas cuyo `name` es `NULL` y una tercera con un nombre puesto
- **WHEN** se aplica la `0007`
- **THEN** las tres filas SHALL tener un nombre no vacío
- **AND** los tres nombres SHALL ser distintos entre sí
- **AND** el nombre de la tercera NO SHALL haber cambiado
- **AND** ningún nombre derivado SHALL ser el identificador de su tarea

#### Scenario: Los nombres repetidos se desambiguan antes de exigir la unicidad

- **GIVEN** una base llevada a mano hasta la `0006` con tres tareas cuyo `name` es el mismo, una cuarta tarea cuyo `name` es ése mismo más el sufijo ` (2)`, y ejecuciones colgando de las cuatro
- **WHEN** se aplica la `0007`
- **THEN** la homónima de menor `rowid` SHALL conservar su nombre
- **AND** las otras dos SHALL recibir un nombre con sufijo, saltándose el ` (2)` que ya estaba ocupado
- **AND** la tarea que ya tenía el ` (2)` NO SHALL haber cambiado de nombre
- **AND** los cuatro nombres SHALL ser distintos entre sí
- **AND** ninguna fila de `job_executions` SHALL perderse

#### Scenario: Un nombre vacío o en blanco se trata como ausente

- **GIVEN** una base llevada a mano hasta la `0006` con dos tareas cuyo `name` es, respectivamente, la cadena vacía y una cadena de sólo espacios
- **WHEN** se aplica la `0007`
- **THEN** la migración NO SHALL abortar
- **AND** las dos tareas SHALL quedar con un nombre derivado, no vacío y distinto del de la otra

#### Scenario: Un nombre que ya acaba en `(N)` se desambigua añadiendo otro sufijo

- **GIVEN** una base llevada a mano hasta la `0006` con dos tareas cuyo `name` es `copia (2)`
- **WHEN** se aplica la `0007`
- **THEN** la de menor `rowid` SHALL conservar `copia (2)`
- **AND** la otra SHALL quedar con `copia (2) (2)`

#### Scenario: El nombre derivado no choca con uno que ya estaba

- **GIVEN** una tarea ya nombrada cuyo nombre coincide con el derivado que le tocaría a otra tarea sin nombre
- **WHEN** se aplica la `0007`
- **THEN** los nombres de las dos filas SHALL seguir siendo distintos
- **AND** el nombre que ya estaba NO SHALL haber cambiado

#### Scenario: Ninguna ejecución se pierde en la reconstrucción

- **GIVEN** una base con tareas —algunas sin nombre— y filas en `job_executions` colgando de ellas
- **WHEN** se aplica la `0007`
- **THEN** todas las filas de `job_executions` SHALL seguir ahí, con su `job_id` resolviendo a su tarea
- **AND** NO SHALL haber quedado ninguna fila huérfana
- **AND** las claves foráneas de `job_executions` NO SHALL quedar rotas

#### Scenario: Los índices de la tabla sobreviven a la reconstrucción

- **GIVEN** una base migrada a la `0007`
- **WHEN** se consultan los índices de `backup_jobs`
- **THEN** SHALL seguir existiendo `idx_backup_jobs_target_id` e `idx_backup_jobs_repository_id`
- **AND** SHALL existir el índice único de `name`

#### Scenario: Un nombre en blanco no cabe en la base

- **GIVEN** una base migrada a la `0007`
- **WHEN** se inserta con SQL directo una fila cuyo `name` es una cadena de solo espacios
- **THEN** la inserción SHALL fallar por la restricción de la columna
- **AND** NO SHALL depender de que la validación la haya hecho la aplicación

### Requirement: La fila de un repositorio guarda su estado de salud

La tabla `repositories` SHALL incorporar tres columnas anulables —el **estado** de salud, la **fecha** de la última comprobación y un **motivo** acotado— mediante una migración con prefijo numérico entero (la forma que la herramienta de migración exige). El estado SHALL estar restringido por un `CHECK` a un conjunto cerrado de valores, y las columnas SHALL ser anulables para que las filas ya existentes queden en «desconocido» sin reescribirse. La migración NO SHALL alterar ni borrar ninguna fila existente.

La capa de acceso SHALL exponer un método que anote en una fila el desenlace de una comprobación —estado, motivo y fecha—, y NO SHALL permitir que el alta ni la actualización del repositorio (reemplazo de los campos mutables) toquen esas columnas: la salud no es un campo que el operador edite, la escribe la comprobación.

#### Scenario: Las filas existentes quedan en desconocido
- **GIVEN** una base con filas de `repositories` anteriores a la migración
- **WHEN** se aplica la migración
- **THEN** cada fila SHALL conservar sus valores
- **AND** su estado de salud SHALL quedar a nulo, es decir «desconocido»

#### Scenario: La comprobación persiste el desenlace
- **GIVEN** una fila de repositorio
- **WHEN** se anota el desenlace de su comprobación
- **THEN** SHALL quedar guardados el estado, la fecha y el motivo
- **AND** SHALL leerse de vuelta con esos valores

#### Scenario: Editar el repositorio no reescribe su salud
- **GIVEN** una fila con un estado de salud anotado
- **WHEN** se actualiza el repositorio con sus campos mutables
- **THEN** el estado, la fecha y el motivo de salud SHALL conservarse
