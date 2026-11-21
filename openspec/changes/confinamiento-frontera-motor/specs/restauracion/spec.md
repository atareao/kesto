# restauracion — delta

## Purpose

Cómo se devuelve un volcado del repositorio a su target: los comandos por tipo, el camino del `stdin` y las garantías de sobrescritura. Este delta acota el `rm -f` de un target `sqlite` a la ruta declarada y exige que esa ruta sea la forma canónica dentro del contenedor.

## MODIFIED Requirements

### Requirement: El comando de restauración depende del tipo de target y es el reverso del volcado

Para un target `mariadb` el comando SHALL ser `mariadb -u<db_user>` cuando `db_name` es nulo, o `mariadb -u<db_user> <db_name>` cuando no lo es, y SHALL leer el SQL del `stdin`. Para un target `postgresql` SHALL ser `psql -U <db_user> -d postgres` cuando `db_name` es nulo, o `psql -U <db_user> -d <db_name>` cuando no lo es, y SHALL leer el SQL del `stdin`. Para un target `sqlite`, cuyo volcado es el `.dump` de SQLite, la restauración SHALL **reconstruir el fichero `db_path`** del contenedor: un primer `exec` SHALL retirar el `<db_path>` vivo y sus hermanos `-wal` y `-shm`, y un segundo `exec` —`sqlite3 <db_path>`— SHALL reimportar el volcado por el `stdin`.

El `<db_path>` NO SHALL ser un enlace simbólico ni contener `..`: **antes** de retirar ni de reimportar nada, el motor SHALL comprobar con `readlink -f` que la ruta declarada coincide con su forma canónica dentro del contenedor y SHALL rechazar la restauración cuando difiera. Con la ruta ya comprobada, el `rm -f` SHALL quedar acotado al `<db_path>` declarado y a sus hermanos `-wal` y `-shm`, y NO SHALL alcanzar ningún otro fichero —en particular, no SHALL seguir un enlace de un directorio padre hasta borrar un fichero ajeno—. La regla completa, con su porqué, vive en la capability `confinamiento`.

La credencial SHALL entregarse por el **entorno** del `exec` —`MYSQL_PWD` para MariaDB y `PGPASSWORD` para PostgreSQL—, nunca en su línea de comandos, y NO SHALL añadirse la variable cuando la contraseña esté vacía. `sqlite` NO SHALL usar `db_user`, `db_password` ni `db_name`.

El porqué: el reverso de cada volcado no es simétrico. `mariadb-dump --all-databases` se deshace con el cliente sin base nombrada; `pg_dumpall` con `psql` sobre una base a la que conectarse —`postgres`, que siempre existe—; y `pg_dump -d base` con `psql -d base`. `sqlite3 <db_path>` sobre una base viva **no** puede aplicar el `.dump` —el `.dump` emite `CREATE TABLE` sin `DROP TABLE`, así que chocaría con las tablas existentes— y por eso la reconstrucción pasa por retirar el fichero antes de reimportar, que además es lo que hace coherente el modo WAL: retirar el `-wal` y el `-shm` evita aplicar un diario viejo sobre un fichero nuevo. Y precisamente porque ese primer paso **borra**, la ruta que nombra no puede quedar sin comprobar: un `db_path` con un enlace en un directorio padre convertiría una restauración en el borrado de un fichero del contenedor que nadie pidió tocar.

#### Scenario: MariaDB restaura por el cliente sin base nombrada

- **GIVEN** un target `mariadb` con `db_name` nulo
- **WHEN** se construye el comando de restauración
- **THEN** SHALL ser `mariadb` con el usuario, sin nombrar ninguna base
- **AND** el SQL SHALL llegar por el `stdin`

#### Scenario: MariaDB restaura una sola base

- **GIVEN** un target `mariadb` con `db_name = "tienda"`
- **WHEN** se construye el comando de restauración
- **THEN** SHALL nombrar `tienda`

#### Scenario: PostgreSQL usa `psql` sobre `postgres` para el volcado global

- **GIVEN** un target `postgresql` con `db_name` nulo
- **WHEN** se construye el comando de restauración
- **THEN** SHALL usar `psql` con `-d postgres`

#### Scenario: PostgreSQL usa `psql` sobre la base nombrada

- **GIVEN** un target `postgresql` con `db_name = "almacen"`
- **WHEN** se construye el comando de restauración
- **THEN** SHALL usar `psql -d almacen`

#### Scenario: SQLite reconstruye el fichero antes de reimportar

- **GIVEN** un target `sqlite` con un `db_path` canónico
- **WHEN** se ejecuta la restauración
- **THEN** el `<db_path>` vivo y sus hermanos `-wal` y `-shm` SHALL retirarse **antes** de reimportar
- **AND** el `.dump` SHALL reimportarse con `sqlite3` sobre el `<db_path>`

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
