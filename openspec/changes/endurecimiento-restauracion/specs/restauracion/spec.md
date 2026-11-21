# restauracion — delta

## Purpose

Cómo se devuelve un volcado del repositorio a su target: los comandos por tipo, el camino del `stdin` y las garantías de sobrescritura. Este delta endurece los **comandos de restauración** para que traten el contenido del volcado —que sale de un repositorio configurable por el operador— como **no confiable**: cada cliente se invoca en el modo que le hace ignorar sus metacomandos, y el volcado de PostgreSQL se trocea por base para poder ignorarlos sin que `\connect` desvíe los datos.

## MODIFIED Requirements

### Requirement: El comando de restauración depende del tipo de target y es el reverso del volcado

Para un target `mariadb` el comando SHALL ser `mariadb --binary-mode -u<db_user>` cuando `db_name` es nulo, o `mariadb --binary-mode -u<db_user> <db_name>` cuando no lo es, y SHALL leer el SQL del `stdin`. Para un target `postgresql` SHALL ser `psql -X -U <db_user> -d <base>` —una orden **por sección** del volcado, con `<base>` la de cada `\connect` y, en la primera sección, `db_name` o `postgres`— y SHALL leer el SQL del `stdin`. Para un target `sqlite`, cuyo volcado es el `.dump` de SQLite, la restauración SHALL **reconstruir el fichero `db_path`** del contenedor: un primer `exec` SHALL retirar el `<db_path>` vivo y sus hermanos `-wal` y `-shm`, y un segundo `exec` —`sqlite3 -safe <db_path>`— SHALL reimportar el volcado por el `stdin`.

El `<db_path>` NO SHALL ser un enlace simbólico ni contener `..`: **antes** de retirar ni de reimportar nada, el motor SHALL comprobar con `readlink -f` que la ruta declarada coincide con su forma canónica dentro del contenedor y SHALL rechazar la restauración cuando difiera. Con la ruta ya comprobada, el `rm -f` SHALL quedar acotado al `<db_path>` declarado y a sus hermanos `-wal` y `-shm`, y NO SHALL alcanzar ningún otro fichero —en particular, no SHALL seguir un enlace de un directorio padre hasta borrar un fichero ajeno—. La regla completa, con su porqué, vive en la capability `confinamiento`.

Los tres clientes SHALL invocarse en el modo que les hace **ignorar los metacomandos** del volcado, porque el contenido se trata como no confiable: `sqlite3` con `-safe`, `mariadb` con `--binary-mode`, y `psql` con `-X` y con el volcado **troceado e ignorado** según el requisito «La restauración ignora los metacomandos del volcado». El contenedor del target `sqlite` SHALL disponer de un `sqlite3` que soporte `-safe` (≥ 3.27); su ausencia SHALL reportarse como el fallo de la ejecución, no silenciarse.

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

El contenido del volcado SHALL tratarse como **no confiable**. Para un target `postgresql`, el motor SHALL **trocear** el volcado en secciones por los metacomandos `\connect <base>` de **primer nivel**, y SHALL entregar cada sección al `stdin` de su propia orden `psql -d <base>`. En cada sección SHALL **descartarse toda línea de metacomando de primer nivel**, conservando únicamente el terminador `\.` de los bloques `COPY`. A cada sección SHALL anteponerse `\restrict <nonce>`, con un `nonce` aleatorio por restauración, como segunda capa.

El troceado SHALL reconocer el estado léxico del volcado —comilla simple (`''`), comilla doble (`""`), dólar (`$tag$`), comentario de línea (`--`), comentario de bloque (`/* */`) y bloque `COPY`— de modo que un `\connect` o un metacomando **dentro de un literal, de un cuerpo de función o de los datos de un `COPY`** NO SHALL confundirse con una frontera ni descartarse. Un metacomando que el troceador dejara escapar SHALL quedar rechazado por `\restrict` cuando el cliente lo soporte.

El volcado NO SHALL materializarse: las secciones SHALL entregarse al `stdin` de cada `psql` conforme se leen, con la memoria acotada. La restauración SHALL darse por buena solo si **todas** las secciones acaban con éxito.

El porqué: `pg_dumpall` separa las bases con `\connect`, y el único modo de `psql` que rechaza metacomandos —`\restrict`— **bloquea `\connect`**: anteponerlo a un volcado global desvía los datos a la base equivocada. Trocear por base permite ignorar los metacomandos de cada sección sin perder el `\connect`. Ignorarlos es la guarda principal —sin metacomandos que entregar, no hay nada que ejecutar, y no depende de la versión del cliente—; `\restrict` es la segunda capa, y el `\unrestrict` que un volcado hostil pudiera traer cae con los demás metacomandos, así que no puede deshacerla. Queda fuera, y se documenta en la frontera de confianza, `COPY … FROM PROGRAM`: es SQL, no un metacomando, y solo lo admite un superusuario.

#### Scenario: Un `\connect` a primer nivel abre una sección por base

- **GIVEN** un volcado de PostgreSQL con dos bases separadas por un `\connect`
- **WHEN** se restaura
- **THEN** SHALL ejecutarse una orden `psql -d <base>` por cada base
- **AND** cada base SHALL recibir el contenido de su sección

#### Scenario: Un `\connect` dentro de un literal no es una frontera

- **GIVEN** un volcado con un cuerpo de función que contiene una línea `\connect <base>` dentro de un dólar o de una comilla
- **WHEN** se restaura
- **THEN** ese `\connect` NO SHALL tratarse como frontera
- **AND** el cuerpo de la función SHALL restaurarse íntegro

#### Scenario: Los metacomandos del volcado no se ejecutan

- **GIVEN** un volcado que contiene una línea `\!` de primer nivel con una orden de shell
- **WHEN** se restaura
- **THEN** la orden de shell NO SHALL ejecutarse
- **AND** la restauración SHALL completar el resto del volcado

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

- **GIVEN** un volcado de dos secciones donde la segunda contiene una sentencia inválida
- **WHEN** se restaura
- **THEN** la restauración SHALL acabar en `failed` con el motivo del contenedor
- **AND** NO SHALL darse por buena aunque la primera sección haya acabado bien

#### Scenario: La memoria no crece con el tamaño del volcado

- **GIVEN** un volcado de PostgreSQL mucho mayor que el búfer del puente
- **WHEN** la restauración está en curso
- **THEN** los bytes retenidos NO SHALL crecer con el tamaño total del volcado
