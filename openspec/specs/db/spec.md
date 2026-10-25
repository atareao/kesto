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

Las migraciones SHALL declarar las cuatro tablas del §3 —`repositories`, `backup_targets`, `backup_jobs` y `job_executions`— con las columnas, los tipos y los valores por defecto de ese apartado, incluidos los `CHECK` de enumerado `target_type IN ('mariadb', 'postgresql', 'directory')` y `status IN ('pending', 'running', 'success', 'failed')`, y las tres claves foráneas `ON DELETE CASCADE`.

Las restricciones SHALL estar **en la base** y no solo en Rust: un `INSERT` hecho con `sqlite3` a mano tiene que chocar con el mismo `CHECK` que choca la aplicación. Es la única forma de que el enumerado sea una garantía y no una convención.

Además de lo que el SRS declara, SHALL crearse **cuatro índices**, que el §3 no tiene y que **se declaran aquí como un añadido al SRS**:

- `idx_backup_jobs_target_id` — columna hija de la FK a `backup_targets`
- `idx_backup_jobs_repository_id` — columna hija de la FK a `repositories`
- `idx_job_executions_job_id` — columna hija de la FK a `backup_jobs`
- `idx_job_executions_status_started_at` — para el listado del dashboard, por estado y fecha

Los tres primeros SHALL existir porque **SQLite no crea índices para las claves foráneas**: sin índice en la columna hija, cada borrado en cascada recorre la tabla entera, y el SRS declara tres cascadas sin pagar ninguno de los tres recorridos. El cuarto cubre el listado del §6: sin él, la consulta del panel ordena recorriendo.

Filtrar las ejecuciones **por repositorio** SHALL resolverse con el `JOIN` por `backup_jobs` apoyado en los índices anteriores, y NO SHALL añadirse una columna `repository_id` a `job_executions`: duplicaría un dato que ya vive en la tarea y crearía una segunda verdad que se puede desincronizar.

Los índices SHALL nombrarse `idx_{tabla}_{columna}`.

#### Scenario: Las cuatro tablas existen con sus columnas
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se enumeran las tablas y sus columnas
- **THEN** SHALL existir `repositories`, `backup_targets`, `backup_jobs` y `job_executions`
- **AND** sus columnas SHALL ser las del §3

#### Scenario: Los enumerados están garantizados por un `CHECK`
- **GIVEN** una base con las migraciones aplicadas
- **WHEN** se intenta insertar un `target_type` o un `status` fuera de su enumerado, con SQL directo
- **THEN** la inserción SHALL fallar
- **AND** NO SHALL depender de que la validación la haya hecho la aplicación

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
