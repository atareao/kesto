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

## D5 — Ignorar metacomandos, con `\restrict` como segunda capa

Dos capas, en este orden:

1. **Ignorar** (descartar) toda línea de metacomando de primer nivel de cada sección, conservando el terminador `\.` de los bloques `COPY`. Es la guarda **principal**: sin metacomandos que entregar, no hay nada que ejecutar, y no depende de la versión del cliente. Es, literalmente, el «modo de importación que ignore metacomandos» del issue. Y descarta también el `\unrestrict` que un volcado hostil pudiera traer: el atacante no puede deshacer la capa 2 porque su propia línea de desbloqueo es, para el troceador, un metacomando más.
2. Anteponer `\restrict <nonce>` a cada sección, con un `nonce` aleatorio por restauración. Es una guarda **de segunda línea**: si el troceador dejara escapar un metacomando, `psql` lo rechazaría. Exige `psql` ≥ 17.6; donde no esté, la línea se rechaza sin efecto y la capa 1 sigue en pie. El `nonce` se genera con `rand` —ya es dependencia— y **no** se deriva de nada del volcado.

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
- Bloquear `COPY … FROM PROGRAM` (D6).
