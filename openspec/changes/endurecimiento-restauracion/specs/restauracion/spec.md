# restauracion — delta

## Purpose

Cómo se devuelve un volcado del repositorio a su target: los comandos por tipo, el camino del `stdin` y las garantías de sobrescritura. Este delta endurece los **comandos de restauración** para que traten el contenido del volcado —que sale de un repositorio configurable por el operador— como **no confiable**: cada cliente se invoca en el modo que le hace ignorar sus metacomandos, y el volcado de PostgreSQL se trocea por base para poder ignorarlos sin que `\connect` desvíe los datos.

## MODIFIED Requirements

### Requirement: El comando de restauración depende del tipo de target y es el reverso del volcado

Para un target `mariadb` el comando SHALL ser `mariadb --binary-mode -u<db_user>` cuando `db_name` es nulo, o `mariadb --binary-mode -u<db_user> <db_name>` cuando no lo es, y SHALL leer el SQL del `stdin`. Para un target `postgresql` SHALL ser `psql -X -U <db_user> -d <base>` —una orden **por sección** del volcado, con `<base>` la de cada `\connect` y, en la primera sección, `db_name` o `postgres`— y SHALL leer el SQL del `stdin`. Para un target `sqlite`, cuyo volcado es el `.dump` de SQLite, la restauración SHALL **reconstruir el fichero `db_path`** del contenedor: un primer `exec` SHALL retirar el `<db_path>` vivo y sus hermanos `-wal` y `-shm`, y un segundo `exec` —`sqlite3 -safe <db_path>`— SHALL reimportar el volcado por el `stdin`.

El `<db_path>` NO SHALL ser un enlace simbólico ni contener `..`: **antes** de retirar ni de reimportar nada, el motor SHALL comprobar con `readlink -f` que la ruta declarada coincide con su forma canónica dentro del contenedor y SHALL rechazar la restauración cuando difiera. Con la ruta ya comprobada, el `rm -f` SHALL quedar acotado al `<db_path>` declarado y a sus hermanos `-wal` y `-shm`, y NO SHALL alcanzar ningún otro fichero —en particular, no SHALL seguir un enlace de un directorio padre hasta borrar un fichero ajeno—. La regla completa, con su porqué, vive en la capability `confinamiento`.

Los tres clientes SHALL invocarse en el modo que les hace **ignorar los metacomandos** del volcado, porque el contenido se trata como no confiable: `sqlite3` con `-safe`, `mariadb` con `--binary-mode`, y `psql` con `-X` y con el volcado **troceado e ignorado** según el requisito «La restauración ignora los metacomandos del volcado». El contenedor del target `sqlite` SHALL disponer de un `sqlite3` que soporte `-safe` (≥ 3.27); su ausencia SHALL reportarse como el fallo de la ejecución, no silenciarse. El contenedor del target `postgresql` SHALL disponer de un `psql` que honre `\restrict` (17.6, o el mínimo de su rama); su ausencia SHALL hacer fallar la restauración según el requisito «La restauración exige un cliente `psql` que honre `\restrict`».

La credencial SHALL entregarse por el **entorno** del `exec` —`MYSQL_PWD` para MariaDB y `PGPASSWORD` para PostgreSQL—, nunca en su línea de comandos, y NO SHALL añadirse la variable cuando la contraseña esté vacía. `sqlite` NO SHALL usar `db_user`, `db_password` ni `db_name`.

El porqué: el reverso de cada volcado no es simétrico. `mariadb-dump --all-databases` se deshace con el cliente sin base nombrada; `pg_dumpall` con `psql` sobre una base a la que conectarse —`postgres`, que siempre existe—; y `pg_dump -d base` con `psql -d base`. `sqlite3 <db_path>` sobre una base viva **no** puede aplicar el `.dump` —el `.dump` emite `CREATE TABLE` sin `DROP TABLE`, así que chocaría con las tablas existentes— y por eso la reconstrucción pasa por retirar el fichero antes de reimportar, que además es lo que hace coherente el modo WAL: retirar el `-wal` y el `-shm` evita aplicar un diario viejo sobre un fichero nuevo. Y precisamente porque ese primer paso **borra**, la ruta que nombra no puede quedar sin comprobar: un `db_path` con un enlace en un directorio padre convertiría una restauración en el borrado de un fichero del contenedor que nadie pidió tocar. Los tres modos de cliente son la mitad del endurecimiento: el volcado lo escribe quien controla el repositorio, y sus metacomandos no pueden llegar a ejecutarse.

#### Scenario: MariaDB restaura por el cliente sin base nombrada

- **GIVEN** un target `mariadb` con `db_name` nulo
- **WHEN** se construye el comando de restauración
- **THEN** SHALL ser `mariadb --binary-mode` con el usuario, sin nombrar ninguna base
- **AND** el SQL SHALL llegar por el `stdin`

#### Scenario: MariaDB restaura una sola base

- **GIVEN** un target `mariadb` con `db_name = "tienda"`
- **WHEN** se construye el comando de restauración
- **THEN** SHALL ser `mariadb --binary-mode` y SHALL nombrar `tienda`

#### Scenario: PostgreSQL usa `psql` sobre `postgres` para el volcado global

- **GIVEN** un target `postgresql` con `db_name` nulo
- **WHEN** se construye el comando de restauración
- **THEN** SHALL usar `psql -X` con `-d postgres` como base de la primera sección

#### Scenario: PostgreSQL usa `psql` sobre la base nombrada

- **GIVEN** un target `postgresql` con `db_name = "almacen"`
- **WHEN** se construye el comando de restauración
- **THEN** SHALL usar `psql -X -d almacen` como base de la primera sección

#### Scenario: SQLite reconstruye el fichero antes de reimportar

- **GIVEN** un target `sqlite` con un `db_path` canónico
- **WHEN** se ejecuta la restauración
- **THEN** el `<db_path>` vivo y sus hermanos `-wal` y `-shm` SHALL retirarse **antes** de reimportar
- **AND** el `.dump` SHALL reimportarse con `sqlite3 -safe` sobre el `<db_path>`

#### Scenario: Un db_path que no es canónico se rechaza antes de borrar

- **GIVEN** un target `sqlite` cuyo `db_path` no coincide con su forma canónica dentro del contenedor
- **WHEN** se ejecuta la restauración
- **THEN** SHALL acabar en `failed`
- **AND** NO SHALL retirarse ningún fichero

#### Scenario: La contraseña no está en la línea de comandos

- **GIVEN** un target de base con contraseña
- **WHEN** se construye el `exec` de su restauración
- **THEN** el `cmd` NO SHALL contener esa contraseña
- **AND** la variable de entorno del tipo de base SHALL llevarla

#### Scenario: Un `sqlite3` sin `-safe` hace fallar la restauración en voz alta

- **GIVEN** un contenedor `sqlite` cuyo `sqlite3` no soporta `-safe`
- **WHEN** se ejecuta la restauración
- **THEN** SHALL acabar en `failed` con el error del contenedor
- **AND** NO SHALL darse por buena la restauración

## ADDED Requirements

### Requirement: La restauración ignora los metacomandos del volcado

El contenido del volcado SHALL tratarse como **no confiable**. Para un target `postgresql`, el motor SHALL **trocear** el volcado en secciones por los metacomandos `\connect <base>` de **primer nivel**, y SHALL entregar cada sección al `stdin` de su propia orden `psql -d <base>`. En cada sección SHALL **descartarse toda línea de metacomando de primer nivel**, conservando únicamente el terminador `\.` de los bloques `COPY`. Una línea de metacomando de primer nivel es la que, **fuera de un literal, de un comentario o de un bloque `COPY`**, tiene una barra invertida como **primer byte no blanco**: el cliente acepta el metacomando precedido de **cualquier** blanco de su juego —`[ \t\n\r\f\v]`, esto es espacio, tabulador, salto de línea, retorno de carro, avance de página y tabulador vertical—, así que NO SHALL reconocerse solo por la columna cero **ni** con un juego de blancos más estrecho: `\f\!`, `\v\!` y `\r\!` ejecutan en el cliente (verificado). A cada sección SHALL anteponerse `\restrict <nonce>`, con un `nonce` aleatorio por restauración.

El troceado SHALL reconocer el estado léxico del volcado —comilla simple (`''`), comilla doble (`""`), dólar (`$tag$`), comentario de línea (`--`), comentario de bloque (`/* */`) y bloque `COPY`— de modo que un `\connect` o un metacomando **dentro de un literal, de un cuerpo de función o de los datos de un `COPY`** NO SHALL confundirse con una frontera ni descartarse. En un literal precedido del prefijo `E` (`E'…'`) SHALL reconocerse la **barra invertida como escape** —`\'` NO SHALL cerrar el literal—, porque el cliente lo lee así y un troceador que cerrara ahí vería una **frontera falsa** y desviaría el resto de la sección a otra base.

La frontera `\connect` SHALL reconocerse como la reconoce el cliente: su **alias `\c`** SHALL aceptarse, y la palabra SHALL terminar ahí —`\connectx`, `\cd`, `\conninfo` o `\copy` NO SHALL ser fronteras—. Un `\connect` **sin nombre** reconecta a la **misma** base (verificado), así que NO SHALL abrir una sección nueva. Dentro de un nombre entrecomillado, `""` SHALL leerse como una comilla escapada (`\connect "a""b"` nombra la base `a"b`), y la etiqueta de un dólar SHALL admitir el juego **no ASCII** del cliente (`dolq_start = [A-Za-z\200-\377_]`), de modo que un `$café$…$café$` no deje pasar el `\connect` que llevara dentro.

El bloque `COPY … FROM stdin` SHALL reconocerse **como lo reconoce `psql`**, no por la subcadena `FROM stdin`: el motor SHALL registrar la **letra inicial de los primeros identificadores de primer nivel** de la sentencia —con el tope de ocho del cliente, y **sin** registrar los de dentro de paréntesis— y SHALL entrar en modo `COPY` cuando la sentencia **termine** en un `;` a profundidad de paréntesis cero. Así, `COPY nope TO STDOUT; -- FROM stdin` y `COPY (SELECT 1 FROM stdin_x) TO STDOUT;` NO SHALL abrir un bloque `COPY`, y una cabecera partida en dos líneas (`COPY t (a)` + `FROM stdin;`) SHALL abrirlo.

El nombre de base que sale de un `\connect` SHALL entregarse como **valor** de la opción `-d`, nunca como un argumento suelto: un nombre que empiece por `-` NO SHALL interpretarse como una opción de `psql` (verificado: `psql -d -c …` busca una base llamada `-c`, y `-d --help` una llamada `--help`). Y un nombre con forma de **cadena de conexión** —con un `=` o con el esquema `postgres://` o `postgresql://`, que es lo que hace que libpq lea el valor como *conninfo* y de ahí saque `host`, `port`, `user`…— SHALL **rechazarse**: la restauración SHALL fallar sin ejecutar esa sección ni las siguientes, y el motivo NO SHALL publicar el valor. La comprobación del esquema SHALL ser **sin distinguir mayúsculas** —libpq reconoce el prefijo literal y es sensible a mayúsculas, así que el motor SHALL ser un **superconjunto** de lo que libpq expande y SHALL rechazar también `POSTGRES://…`, que libpq trataría como nombre plano—: rechazar de más es **fail-closed** y deliberado. La base de la **primera** sección —`db_name` o `postgres`— NO SHALL validarse: la elige el operador en la configuración del target, no viene del volcado.

El volcado NO SHALL materializarse: las secciones SHALL entregarse al `stdin` de cada `psql` conforme se leen, con la memoria acotada a la línea en curso. La restauración SHALL darse por buena solo si **todas** las secciones acaban con **código de salida cero**, y los errores de SQL **dentro** de una sección NO SHALL detectarse **por ahora**: `psql` sin `ON_ERROR_STOP` sale con `0` ante un error de SQL, así que una sección cuyas sentencias fallaron se da por buena. El spec declara ese límite a propósito —es preferible a prometer una garantía que el código no da—, y cerrarlo va en el issue **#175**.

El porqué: `pg_dumpall` separa las bases con `\connect`, y el único modo de `psql` que rechaza metacomandos —`\restrict`— **bloquea `\connect`**: anteponerlo a un volcado global desvía los datos a la base equivocada. Trocear por base permite ignorar los metacomandos de cada sección sin perder el `\connect`. Ignorarlos es la guarda principal —sin metacomandos que entregar, no hay nada que ejecutar—; `\restrict` es la segunda capa, y el `\unrestrict` que un volcado hostil pudiera traer cae con los demás metacomandos, así que no puede deshacerla. Queda fuera, y se documenta en la frontera de confianza, `COPY … FROM PROGRAM`: es SQL, no un metacomando, y solo lo admite un superusuario.

#### Scenario: Un `\connect` a primer nivel abre una sección por base

- **GIVEN** un volcado de PostgreSQL con dos bases separadas por un `\connect`
- **WHEN** se restaura
- **THEN** SHALL ejecutarse una orden `psql -d <base>` por cada base
- **AND** cada base SHALL recibir el contenido de su sección

#### Scenario: El nombre de base del `\connect` no se convierte en una opción

- **GIVEN** un volcado cuyo `\connect` nombra una base que empieza por `-`
- **WHEN** se restaura
- **THEN** ese nombre SHALL entregarse como valor de `-d`
- **AND** `psql` NO SHALL leerlo como una opción

#### Scenario: El nombre de base del `\connect` no se convierte en una cadena de conexión

- **GIVEN** un volcado cuyo `\connect` nombra una base con forma de cadena de conexión
- **WHEN** la restauración reparte las secciones
- **THEN** SHALL fallar sin ejecutar esa sección ni las siguientes
- **AND** la comprobación NO SHALL distinguir mayúsculas: `POSTGRES://…` SHALL rechazarse igual, aunque libpq lo trataría como un nombre plano (superconjunto deliberado, *fail-closed*)

#### Scenario: Un `\connect` dentro de un literal no es una frontera

- **GIVEN** un volcado con un cuerpo de función que contiene una línea `\connect <base>` dentro de un dólar o de una comilla
- **WHEN** se restaura
- **THEN** ese `\connect` NO SHALL tratarse como frontera
- **AND** el cuerpo de la función SHALL restaurarse íntegro

#### Scenario: Un `\connect` tras una comilla escapada no es una frontera

- **GIVEN** un volcado con un literal `E'…\'…'` que contiene, en una línea, un `\connect <otra>`
- **WHEN** se restaura
- **THEN** ese `\connect` NO SHALL tratarse como frontera
- **AND** la sección SHALL seguir entregándose a la base en curso

#### Scenario: Los metacomandos del volcado no se ejecutan

- **GIVEN** un volcado que contiene una línea `\!` de primer nivel con una orden de shell
- **WHEN** se restaura
- **THEN** la orden de shell NO SHALL ejecutarse
- **AND** la restauración SHALL completar el resto del volcado

#### Scenario: Un metacomando precedido de espacios se descarta igual

- **GIVEN** un volcado con un `\!` de primer nivel precedido de espacios o de un tabulador
- **WHEN** se restaura
- **THEN** ese metacomando SHALL descartarse como cualquier otro
- **AND** NO SHALL entregarse al `psql`

#### Scenario: Los datos de un `COPY` no se descartan

- **GIVEN** un volcado con un bloque `COPY` cuyos datos incluyen una línea que parece un metacomando
- **WHEN** se restaura
- **THEN** esos datos SHALL restaurarse íntegros
- **AND** NO SHALL descartarse ninguna línea del bloque

#### Scenario: El `\unrestrict` del volcado no deshace la guarda

- **GIVEN** un volcado que trae un `\unrestrict <clave>` de primer nivel
- **WHEN** se restaura
- **THEN** ese `\unrestrict` SHALL descartarse como cualquier otro metacomando
- **AND** NO SHALL entregarse al `psql`

#### Scenario: Una sección que falla falla la restauración entera

- **GIVEN** un volcado de dos secciones donde la orden de la segunda sale con un código distinto de cero
- **WHEN** se restaura
- **THEN** la restauración SHALL acabar en `failed` con el motivo del contenedor
- **AND** NO SHALL darse por buena aunque la primera sección haya acabado bien

#### Scenario: La memoria no crece con el tamaño del volcado

- **GIVEN** un volcado de PostgreSQL mucho mayor que el búfer del puente
- **WHEN** la restauración está en curso
- **THEN** los bytes retenidos NO SHALL crecer con el tamaño total del volcado

#### Scenario: Un metacomando precedido de avance de página, tabulador vertical o retorno se descarta

- **GIVEN** un volcado con un `\!` de primer nivel precedido de `\f`, `\v` o `\r`
- **WHEN** se restaura
- **THEN** ese metacomando SHALL descartarse como cualquier otro
- **AND** NO SHALL entregarse al `psql`

#### Scenario: Un `FROM stdin` que no abre un `COPY` no se confunde con uno que sí

- **GIVEN** un volcado con `COPY nope TO STDOUT; -- FROM stdin` y con `COPY (SELECT 1 FROM stdin_x) TO STDOUT;`
- **WHEN** se restaura
- **THEN** NO SHALL entrar en modo `COPY`
- **AND** un `\connect` posterior SHALL seguir tratándose como frontera

#### Scenario: Un `\connect` sin nombre no abre una sección

- **GIVEN** un volcado con un `\connect` sin nombre de base
- **WHEN** se restaura
- **THEN** NO SHALL abrirse una sección nueva
- **AND** la base en curso NO SHALL cambiar

#### Scenario: El alias `\c` abre una sección como `\connect`

- **GIVEN** un volcado con un `\c <base>` de primer nivel
- **WHEN** se restaura
- **THEN** SHALL tratarse como una frontera, igual que `\connect <base>`
- **AND** `\cd` y `\conninfo` NO SHALL tratarse como fronteras

### Requirement: La restauración exige un cliente `psql` que honre `\restrict`

La restauración de un target `postgresql` SHALL comprobar, **antes** de leer una sola línea del volcado y con el **mismo binario** que usará el reparto, que el cliente del contenedor honra `\restrict`. La comprobación SHALL ejecutar `<binario> --version` —sin la credencial, porque `--version` no conecta— y SHALL aceptar al cliente solo si su versión es `psql` 17.6 o el mínimo de su rama: 16.10, 15.14, 14.19, 13.22; PostgreSQL 18 SHALL aceptarse por traerlo de serie. Cuando la sonda no llegue a término, salga con un código distinto de cero, o la versión **no** honre `\restrict` —incluida una salida de la que no se pueda leer la versión, que SHALL tratarse como «no lo honra»—, la restauración SHALL acabar en `failed` con un motivo que nombre el contenedor, la versión leída y el mínimo, y NO SHALL restaurar nada. El motivo NO SHALL publicar la credencial.

El porqué: `\restrict` es la **segunda** capa del endurecimiento, la que convierte un fallo de la primera —un borde léxico del troceador que no se reconozca— en un rechazo del cliente. Sin ella, un volcado hostil conseguiría ejecutar metacomandos como `root` en el contenedor destino, y «la capa 1 sigue en pie» no es una garantía de producción sino una afirmación que la batería de pruebas verifica a mano. Por eso la restauración **falla en voz alta** en vez de degradarse.

#### Scenario: Un `psql` que no honra `\restrict` falla la restauración

- **GIVEN** un contenedor `postgresql` cuyo `psql` es anterior al mínimo de su rama (por ejemplo, 17.5)
- **WHEN** se restaura un volcado
- **THEN** la restauración SHALL acabar en `failed`
- **AND** el motivo SHALL nombrar el contenedor, la versión leída y el mínimo (17.6, o el de su rama)
- **AND** NO SHALL restaurarse ninguna sección del volcado
