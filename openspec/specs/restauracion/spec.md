# restauracion Specification

## Purpose
Devolver los datos de un target de base de datos a su sitio: el proceso inverso a `extraction`. Cubre cómo el volcado guardado en el repositorio sale de él **sin pasar por el disco** de Kesto, cómo se inyecta en el contenedor del target por el `stdin` de un `exec`, qué comando le corresponde a cada tipo de base, la confirmación explícita de sobrescritura y el trato de los errores del contenedor. Fuera quedan la resolución del vínculo `snapshot → target` y los códigos HTTP, que son de `api-resources`, y el envío y la lectura del repositorio, que son de `repository`.

## Requirements

### Requirement: El volcado restaurado sale del repositorio como un origen de lectura y llega al contenedor por su `stdin`, sin fichero temporal

La restauración de un target de base de datos SHALL obtener el volcado que la copia guardó —un único nodo de fichero dentro de la instantánea— llamando a `Repository::dump`, que escribe su contenido en un `impl Write`, y SHALL entregar esos bytes al `stdin` del proceso que corre dentro del contenedor del target. NO SHALL escribirse el volcado en ningún fichero, ni temporal ni definitivo, ni en el sistema de ficheros de Kesto ni en uno compartido con el contenedor. La conexión entre el `Write` bloqueante que exige la biblioteca y el `stdin` asíncrono del `exec` SHALL ser explícita y **acotada**, de modo que la presión de memoria no dependa de en cuál de las dos partes vaya más rápido.

Desde el transporte, la orden de restauración SHALL pedir `attach_stdin`, y NO SHALL cerrar el `stdin` antes de haber entregado el volcado entero: cerrarlo antes trunca la restauración —el cliente de la base recibe un fin de entrada con la orden a medias— y dejaría el archivo restaurado incompleto sin que el código de salida lo delate.

El porqué: un volcado de base no es pequeño y no existe como fichero en Kesto —la copia lo hizo en streaming hasta la biblioteca—, así que materializarlo para poder restaurarlo repetiría el problema que el §4 prohíbe, con las mismas consecuencias: duplicar el espacio en el peor momento y dejar basura si falla. El puente acotado es el espejo del que la extracción usa para copiar, y la misma razón lo justifica: un productor rápido contra un consumidor lento acumularía el volcado entero en memoria.

#### Scenario: El volcado no toca el disco de Kesto

- **GIVEN** una restauración de base de datos en curso
- **WHEN** se inspecciona el sistema de ficheros de Kesto mientras corre
- **THEN** NO SHALL existir ningún fichero con el volcado
- **AND** el origen del `stdin` del `exec` NO SHALL ser un fichero

#### Scenario: La memoria no crece con el tamaño del volcado

- **GIVEN** un volcado mucho mayor que el búfer del puente
- **WHEN** la restauración está en curso
- **THEN** los bytes retenidos por el puente NO SHALL crecer con el tamaño total del volcado

#### Scenario: El `stdin` no se cierra antes de entregar el volcado

- **GIVEN** una restauración de base de datos
- **WHEN** termina de entregarse el volcado
- **THEN** el `stdin` del `exec` SHALL cerrarse **después** del último byte
- **AND** la restauración NO SHALL darse por buena si el volcado quedó a medias

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

### Requirement: La restauración de un target de base exige confirmación explícita de sobrescritura

*(El título es el que le puso el change que creó el requisito y se deja intacto; su alcance, hoy, es el de los dos tipos de target que la bandera gobierna.)*

Restaurar un target de base de datos SHALL exigir la confirmación explícita de sobrescritura, y el flujo SHALL rechazar la operación —antes de lanzar ningún trabajo— cuando esa confirmación no se ha dado. La confirmación SHALL ser una bandera booleana del contrato de la petición, no un supuesto: quien restaura una base autoriza perder lo que hoy tenga.

La misma bandera SHALL gobernar la **poda** de un target de directorio. Con la bandera en `true`, la restauración de un directorio SHALL, además de escribir la instantánea, **retirar del destino** lo que no está en ella; con la bandera en `false` —o ausente, porque el cuerpo la declara con su valor por defecto— el destino SHALL **solo recibir**, sin retirar nada. Para un directorio la bandera NO SHALL ser obligatoria: la petición sin ella SHALL atenderse sin rechazo por condición previa sin cumplir, que es una puerta reservada a las bases.

La bandera NO SHALL deducirse del tipo de target. El mismo `directory` se restaura con poda o sin ella según lo que se haya confirmado, y el contrato del cuerpo SHALL llevar la bandera para todos los tipos, no sólo para el de base. La confirmación que el panel ofrece —con el aviso de lo que se pierde, y el envío de la bandera sólo al aceptarla— se declara en `ui`.

El porqué: restaurar un directorio escribe ficheros; restaurar una base **pisa datos vivos** —no hay «modo consulta»—. Pero un directorio no es inocuo: la poda **retira** del destino lo que no está en la instantánea, y lo retirado no vuelve, porque el operador no tiene copia de lo que había. El efecto irreversible es el mismo en los dos casos, y por eso la bandera es la misma: separa un `POST` de prueba de una operación destructiva. Lo que cambia entre los dos no es el aviso, es la puerta: en la base la ausencia de la bandera **rechaza** —pisa datos vivos, así que hay que decir que sí de forma deliberada—, y en el directorio **degrada** a una restauración que sólo recibe, que sigue siendo una operación útil y no destructiva. Y no se deduce del tipo porque el tipo no dice si ese destino puede permitirse perder lo que no está en la copia.

#### Scenario: Sin confirmación explícita no se restaura

- **GIVEN** un snapshot de un target de base de datos
- **WHEN** se pide restaurarlo **sin** la confirmación de sobrescritura
- **THEN** NO SHALL lanzarse ninguna ejecución de restauración
- **AND** NO SHALL publicarse ningún `restore_progress`
- **AND** la operación SHALL rechazarse por condición previa sin cumplir

#### Scenario: Con confirmación explícita se restaura

- **GIVEN** un snapshot de un target de base de datos
- **WHEN** se pide restaurarlo **con** la confirmación de sobrescritura
- **THEN** SHALL lanzarse la ejecución de restauración
- **AND** SHALL publicarse `restore_progress`

#### Scenario: Un directorio con la bandera poda el destino

- **GIVEN** un snapshot de un target de directorio y un destino con ficheros que no están en la instantánea
- **WHEN** se pide restaurarlo con la bandera de sobrescritura en `true`
- **THEN** SHALL lanzarse la restauración
- **AND** el destino SHALL quedarse con la instantánea y **sin** lo que no estaba en ella

#### Scenario: Un directorio sin la bandera solo recibe, y no se rechaza

- **GIVEN** el mismo snapshot y el mismo destino
- **WHEN** se pide restaurarlo con la bandera en `false` o sin la bandera en el cuerpo
- **THEN** SHALL lanzarse la restauración
- **AND** el destino SHALL recibir la instantánea **sin** retirar lo que no está en ella
- **AND** NO SHALL rechazarse la petición por condición previa sin cumplir

#### Scenario: La bandera no se deduce del tipo de target

- **GIVEN** el mismo snapshot de un target de directorio
- **WHEN** se restauran dos veces, una con la bandera y otra sin ella
- **THEN** los dos desenlaces SHALL ser distintos —con poda y sólo recibiendo—
- **AND** la diferencia NO SHALL venir del tipo de target, que es el mismo en las dos

### Requirement: El error del contenedor se lee y no se pierde

La restauración SHALL capturar el `stderr` del proceso de restauración y el código de salida, y SHALL distinguir **dos** fallos que un cierre sin más no distingue: que el comando haya salido con un código distinto de cero y que el contenedor no haya podido arrancarlo, ni exista. El motivo que se registre en la ejecución de restauración SHALL ser el que dio el proceso, y SHALL guardarse en `restore_executions.error_message`.

Una restauración que falla a mitad es peor que una que no empieza: puede dejar el target a medias. Por eso el desenlace —y el motivo exacto del contenedor— tienen que quedar registrados, y no basta con un estado genérico.

#### Scenario: Un comando que falla acaba registrado con el motivo del contenedor

- **GIVEN** un contenedor en el que el comando de restauración sale con código distinto de cero y escribe el motivo en `stderr`
- **WHEN** la restauración acaba
- **THEN** la fila de `restore_executions` SHALL quedar en `failed`
- **AND** su `error_message` SHALL contener el texto que escribió el contenedor

#### Scenario: Un contenedor que no existe no se confunde con un fallo de restauración

- **GIVEN** un target cuyo `container_name` no corresponde a ningún contenedor en marcha
- **WHEN** se ejecuta la restauración
- **THEN** SHALL acabar en `failed`
- **AND** el motivo SHALL nombrar el contenedor

#### Scenario: Falta el cliente de la base en el contenedor

- **GIVEN** un target cuyo contenedor no tiene el binario del comando de restauración
- **WHEN** se ejecuta la restauración
- **THEN** SHALL acabar en `failed`
- **AND** el motivo SHALL reflejar el fallo del contenedor, sin silenciarse

### Requirement: La credencial no aparece en el registro del proceso

La restauración NO SHALL dejar el valor de la contraseña de la base en el registro de Kesto, ni con el nivel de registro más hablador. La variable de entorno del `exec` lleva la credencial, y el valor NO SHALL salir por debajo de esta capa: la exposición se corta por el mismo **filtro** de registro que ya declara la extracción, en el arranque del proceso.

#### Scenario: La contraseña no sale en un arranque con el registro hablador

- **GIVEN** un proceso arrancado con el registro a `debug`
- **WHEN** se restaura un target con contraseña
- **THEN** el registro NO SHALL contener el valor de esa contraseña

### Requirement: Dos restauraciones sobre el mismo destino no caben a la vez

Dos restauraciones simultáneas sobre el **mismo destino** —el mismo `container_name` y la misma base, o el mismo `db_path`— NO SHALL ejecutarse a la vez: la segunda SHALL rechazarse. Las restauraciones sobre destinos distintos NO SHALL bloquearse entre sí.

Es la misma guarda que ya protege la restauración a directorio, extendida al destino de una base: dos procesos escribiendo lo mismo a la vez dejarían el target en un estado que no es el de ninguna de las dos copias.

#### Scenario: Dos restauraciones sobre el mismo destino no caben

- **GIVEN** una restauración de una base en curso
- **WHEN** se pide una segunda sobre el mismo destino
- **THEN** la segunda SHALL rechazarse
- **AND** la primera SHALL seguir en curso

#### Scenario: Destinos distintos no se bloquean

- **GIVEN** una restauración de una base en curso
- **WHEN** se pide otra sobre un destino distinto
- **THEN** las dos SHALL poder estar en curso a la vez
