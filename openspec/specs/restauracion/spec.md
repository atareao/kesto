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
