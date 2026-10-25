# extraction Specification

## Purpose
Sacar los datos de su sitio para que Rustic los copie: el proceso que se ejecuta dentro del contenedor de la base vía la API de Docker, el comando que le corresponde a cada tipo de target, las credenciales que viajan en el entorno del proceso y no en su línea de comandos, y el camino que recorre el `stdout` hasta la biblioteca de copia. Cubre también el caso en que no hay ningún proceso que ejecutar —un directorio— y el trato de los errores con los que el contenedor contesta. Fuera quedan el envío al repositorio, que es de `repository`, y el orden de las copias, que es de `engine`.

## ADDED Requirements

### Requirement: El volcado sale del contenedor por la API de Docker y ningún byte pasa por el disco

La extracción de un target de base de datos SHALL ejecutar el comando de volcado dentro de su contenedor a través de la API de Docker, SHALL leer el `stdout` de ese proceso como origen de la copia, y NO SHALL escribir el volcado en ningún fichero, ni temporal ni definitivo. NO SHALL usarse `docker cp`, ni un volumen compartido, ni un fichero intermedio en `/tmp`.

Es lo que pide el §4.A —«procesa la salida estándar del proceso ejecutado dentro del contenedor y la reconduce a un búfer en memoria consumido directamente por `rustic_core`»— y el motivo de que el SRS lo pida así es que un volcado de base no es pequeño. Escribirlo en un fichero duplica el espacio necesario en el peor momento —cuando la base entera acaba de salir y todavía no se ha subido nada— y en un contenedor el disco puede ser un `tmpfs` pequeño. Un volcado de veinte gigabytes que no cabe en `/tmp` no falla al copiar: falla al escribir el temporal, que es un fallo que además deja basura.

#### Scenario: El volcado no toca el disco
- **GIVEN** una ejecución de una copia de base de datos en curso
- **WHEN** se inspecciona el sistema de ficheros del proceso mientras corre
- **THEN** NO SHALL existir ningún fichero con el volcado
- **AND** el destino del `stdout` del `exec` NO SHALL ser un fichero

#### Scenario: El origen de la copia es el stdout del exec
- **GIVEN** una base que devuelve un volcado conocido por su `stdout`
- **WHEN** la copia termina
- **THEN** el snapshot SHALL contener esos bytes
- **AND** SHALL contenerlos sin que hayan pasado por el disco de Kesto

#### Scenario: Un volcado mayor que el espacio libre se copia igual
- **GIVEN** un volcado mayor que el espacio libre del contenedor de Kesto
- **WHEN** la copia se ejecuta
- **THEN** SHALL completarse
- **AND** NO SHALL fallar por falta de espacio

### Requirement: Las credenciales de la base viajan en el entorno del `exec`, nunca en su línea de comandos

La contraseña de la base SHALL entregarse al proceso de volcado por una variable de **entorno** del `exec` —`MYSQL_PWD` para MariaDB y `PGPASSWORD` para PostgreSQL—, y NO SHALL aparecer en el `cmd` del proceso.

En un contenedor, la línea de comandos de un proceso es pública para cualquiera con acceso al listado de procesos, y el `-p<contraseña>` del §4.A dejaba la credencial a la vista en el sitio donde más gente la mira. Es exactamente el cabo suelto 4 del SRS, que ya sugería esta salida. Mandarla por el entorno no la vuelve inocua —el entorno también se puede leer— pero deja de estar en el sitio donde aparece en cualquier diagnóstico de rutina.

#### Scenario: La contraseña no está en la línea de comandos
- **GIVEN** un target con una contraseña conocida
- **WHEN** se construye el `exec` de su volcado
- **THEN** el `cmd` NO SHALL contener esa contraseña
- **AND** SHALL contener el usuario y el comando de volcado

#### Scenario: La contraseña viaja en el entorno
- **GIVEN** el mismo target
- **WHEN** se construye el `exec`
- **THEN** la variable de entorno del tipo de base SHALL llevar la contraseña
- **AND** `mariadb` SHALL usar `MYSQL_PWD` y `postgresql` SHALL usar `PGPASSWORD`

#### Scenario: Un target sin contraseña no inventa una variable
- **GIVEN** un target cuyo `db_password` está vacío
- **WHEN** se construye el `exec`
- **THEN** NO SHALL añadirse la variable de entorno de la contraseña

### Requirement: El comando depende del tipo de target y `db_name` decide el alcance

Para un target `mariadb` el comando SHALL ser `mariadb-dump --single-transaction --quick --lock-tables=false -u<db_user>` seguido de `--all-databases` cuando `db_name` es nulo, o de la base nombrada cuando no lo es. Para un target `postgresql` SHALL ser `pg_dumpall -U <db_user>` cuando `db_name` es nulo, o `pg_dump -U <db_user> -d <db_name>` cuando no lo es. `db_name` nulo SHALL significar volcado global y `db_name` informado SHALL significar esa sola base.

Esto es la deuda 3 del SRS, y se cierra cambiando los comandos que el propio §4.A fija: `mariadb-dump --all-databases` y `pg_dumpall` vuelcan el servidor **entero** y no miran `db_name`, así que con ellos la columna del §3 era decorativa. El esquema ya había declarado qué significa —`NULL` es global, informado es una sola base—, de modo que lo que faltaba no era una decisión sino hacer los comandos coherentes con ella. `pg_dumpall` no puede volcar una sola base: para el caso concreto hace falta `pg_dump`, que es otro binario.

#### Scenario: MariaDB sin db_name vuelca todo el servidor
- **GIVEN** un target `mariadb` con `db_name` nulo
- **WHEN** se construye el comando
- **THEN** SHALL llevar `--all-databases`
- **AND** NO SHALL nombrar ninguna base

#### Scenario: MariaDB con db_name vuelca esa base
- **GIVEN** un target `mariadb` con `db_name = "tienda"`
- **WHEN** se construye el comando
- **THEN** SHALL nombrar `tienda`
- **AND** NO SHALL llevar `--all-databases`

#### Scenario: PostgreSQL sin db_name usa pg_dumpall
- **GIVEN** un target `postgresql` con `db_name` nulo
- **WHEN** se construye el comando
- **THEN** SHALL ser `pg_dumpall`

#### Scenario: PostgreSQL con db_name usa pg_dump
- **GIVEN** un target `postgresql` con `db_name = "almacen"`
- **WHEN** se construye el comando
- **THEN** SHALL usar `pg_dump` con `-d almacen`
- **AND** NO SHALL usar `pg_dumpall`

### Requirement: Un target de directorio no pasa por Docker

Un target de tipo `directory` SHALL copiarse leyendo su `host_path` directamente desde el sistema de ficheros del proceso, sin ejecutar ningún `exec`, y su `host_path` SHALL interpretarse como una ruta del sistema de ficheros **de Kesto**.

El §4 fija los comandos de las dos bases y **no describe el flujo de un directorio**, así que esta parte se declara como añadido al SRS. Y la lectura de `host_path` es la única que tiene datos detrás: la alternativa —que fuera una ruta del host— exigiría que Kesto conociera un mapeo host→contenedor que nadie le ha dado, mientras que el `compose` del §8 ya monta los directorios del host dentro de Kesto en solo lectura, en `/backup_sources/...`. Lo que Kesto ve es lo que copia.

Que un directorio no necesite Docker tiene una consecuencia útil: un target de directorio se puede copiar aunque el socket de Docker esté caído.

#### Scenario: Un directorio se copia sin ejecutar nada en un contenedor
- **GIVEN** un target `directory` con un `host_path` que existe y es legible
- **WHEN** se ejecuta la copia
- **THEN** SHALL completarse
- **AND** NO SHALL haberse creado ningún `exec`

#### Scenario: El host_path se lee en el espacio de ficheros de Kesto
- **GIVEN** un target `directory` cuyo `host_path` apunta a un directorio visible desde el proceso
- **WHEN** se ejecuta la copia
- **THEN** el snapshot SHALL contener el contenido de ese directorio
- **AND** SHALL contenerlo con la ruta con la que se montó, no con la del host

#### Scenario: Un directorio se copia con el socket de Docker caído
- **GIVEN** el socket de Docker sin responder y un target de tipo `directory`
- **WHEN** se ejecuta la copia
- **THEN** SHALL completarse

### Requirement: El error del contenedor se lee y no se pierde

La extracción SHALL capturar el `stderr` del proceso de volcado y SHALL distinguir **dos** fallos que un `stdout` vacío no distingue: que el comando haya salido con un código distinto de cero y que el contenedor no haya podido arrancarlo. El motivo que se registre SHALL ser el que dio el proceso, no un texto genérico.

Un `stdout` vacío es ambiguo: puede ser un fallo —no hay cliente de volcado en la imagen, la base rechazó las credenciales, el disco está lleno— o puede ser una base que de verdad está vacía. Sin mirar el código de salida y el `stderr`, un fallo de extracción se convierte en un snapshot de cero bytes dado por bueno, que es la peor de las salidas: una copia que existe, que ocupa sitio y que no se puede restaurar. El mensaje que se guarde tiene que ser el del contenedor, porque es el único que dice si el problema es la contraseña o es el disco.

#### Scenario: Un comando que falla acaba en failed con el motivo del contenedor
- **GIVEN** un contenedor en el que el comando de volcado sale con código distinto de cero y escribe el motivo en `stderr`
- **WHEN** la ejecución acaba
- **THEN** SHALL acabar en `failed`
- **AND** el motivo SHALL contener el texto que escribió el contenedor

#### Scenario: Un contenedor sin el cliente de volcado se distingue de una base vacía
- **GIVEN** un contenedor en el que el binario del comando no existe
- **WHEN** la ejecución acaba
- **THEN** SHALL acabar en `failed`
- **AND** NO SHALL acabar en `success` con un snapshot vacío

#### Scenario: Un contenedor que no existe no se confunde con un fallo de volcado
- **GIVEN** un target cuyo `container_name` no corresponde a ningún contenedor en marcha
- **WHEN** se ejecuta la copia
- **THEN** SHALL acabar en `failed`
- **AND** el motivo SHALL nombrar el contenedor

### Requirement: El motor acota la inactividad de la extracción, no su duración

La extracción SHALL acotar el tiempo **sin recibir salida**, y NO SHALL acotar la duración total de la orden: un volcado que avanza tarda lo que tenga que tardar y no puede fallar por eso. Cuando la orden deje de producir salida durante más que un plazo que decide el motor, la extracción SHALL fallar con un motivo propio que nombre el contenedor y el plazo, y ese desenlace NO SHALL confundirse con una extracción que terminó sin escribir nada.

El plazo no es una precaución, es lo único que acota el trabajo: el `timeout` del cliente de la API de Docker se resuelve **cuando llegan las cabeceras**, así que el cuerpo del `exec` se drena fuera de él y un `mariadb-dump` colgado deja la lectura esperando indefinidamente. La consecuencia de no acotar no es un fallo aislado: la ejecución se queda en `running`, y con ella ocupado el worker secuencial de su repositorio, de modo que todo el trabajo posterior de ese repositorio hace cola detrás para siempre. Es exactamente la salida que el requisito del motor prohíbe.

#### Scenario: Una extracción lenta que avanza no falla
- **GIVEN** una orden de volcado que produce salida a intervalos, todos menores que el plazo
- **WHEN** la extracción termina
- **THEN** SHALL completarse con su código de salida
- **AND** el plazo NO SHALL haber vencido

#### Scenario: Una orden que deja de producir salida falla con un motivo propio
- **GIVEN** una orden que no escribe nada durante más que el plazo de inactividad
- **WHEN** la extracción espera su salida
- **THEN** SHALL fallar nombrando el contenedor y el plazo
- **AND** el fallo NO SHALL confundirse con una extracción que terminó sin escribir nada

### Requirement: La credencial no aparece en el registro del proceso

La extracción NO SHALL dejar el valor de la contraseña de la base en el registro de Kesto, ni con el nivel de registro más hablador. La variable de entorno del `exec` lleva la credencial —es lo que fija el requisito de arriba—, y el valor NO SHALL salir por debajo de esta capa: la biblioteca del motor registra a `DEBUG` el **cuerpo serializado** de la petición, con el entorno dentro, así que la exposición se corta por **filtro** de registro y esa directiva SHALL estar declarada donde arranca el proceso, no dejada a la memoria de quien lo ejecute.

Una credencial que aparece en un registro acaba en un informe de fallo, en una captura de pantalla y en un buscador de incidencias: el sitio donde menos conviene. Y no se puede arreglar con un `Debug` propio, porque el valor no pasa por ningún tipo de este crate: viaja dentro de una petición que serializa la biblioteca.

#### Scenario: La contraseña no sale en un arranque con el registro hablador
- **GIVEN** un proceso arrancado con el registro a `debug`
- **WHEN** se ejecuta la extracción de un target con contraseña
- **THEN** el registro NO SHALL contener el valor de esa contraseña
- **AND** la directiva de registro que lo impide SHALL estar en el arranque declarado del proyecto

### Requirement: El volcado llega a la biblioteca de copia como un origen de lectura, no como un fichero

El `stdout` del `exec` SHALL entregarse a la biblioteca de copia como un origen de lectura del tipo que su API acepta, y NO SHALL materializarse en un fichero para poder leerlo. La conexión entre el `stream` asíncrono de la API de Docker y esa interfaz SHALL ser explícita y SHALL estar acotada, de modo que la presión de memoria no dependa de lo rápido que lea una de las dos partes.

La API de `rustic_core` no acepta un `AsyncRead` —el diagrama del §4.A lo dibuja así, pero la biblioteca pide un trait bloqueante—, así que hay un puente que construir, y su forma importa: sin un tope, un productor rápido contra un consumidor lento acumula el volcado entero en memoria. Con el tope, el productor espera, que es lo que hace que la memoria consumida sea un valor acotado y no una función del tamaño de la base.

#### Scenario: El origen es un lector y no un fichero
- **GIVEN** una copia en curso
- **WHEN** se inspecciona el espacio de ficheros
- **THEN** NO SHALL haber ningún fichero con el volcado

#### Scenario: La memoria no crece con el tamaño del volcado
- **GIVEN** un volcado mucho mayor que el búfer del puente
- **WHEN** la copia está en curso
- **THEN** los bytes retenidos por el puente NO SHALL crecer con el tamaño total del volcado

#### Scenario: El tamaño del origen puede ser desconocido
- **GIVEN** el origen de un volcado por `stdout`
- **WHEN** se le pregunta su tamaño
- **THEN** SHALL poder contestar que no lo conoce
- **AND** la copia SHALL completarse igualmente
