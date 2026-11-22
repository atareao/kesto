# Tasks — endurecimiento-restauracion

> Issue **#164 (SEC-08)**, hijo de la auditoría de seguridad **#156** (bloque C).
> Change de **producto**: motor (restauración), un módulo **puro** nuevo, el arnés de punta a punta y la documentación.
> Decisiones y alternativas descartadas, en `design.md`. TDD: primero el test que hoy falla (rojo), luego el mínimo
> (verde), luego el refactor. **Sin dependencias nuevas**: `rand` y `uuid` ya están en el árbol.
> Plan: `plans/PLAN-041.md`. Documento de la feature: `docs/FEATURE-041-endurecimiento-restauracion.md`.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Rama desde `development`: `feature/endurecimiento-restauracion` (verificación: `git branch --show-current`).
- [x] 0.3 Change aprobado por el usuario (verificación: aprobación explícita y `just check-spec` encuentra el change activo).
- [x] 0.4 **Orden respecto al bloque B**: los dos changes modifican el **mismo** requisito de la capability `restauracion`.
      Este delta se redacta sobre el texto **post-B** (con la comprobación de `db_path` canónico), así que B
      —PR #174— ha de estar **fusionado y archivado** antes de aplicar este change. Verificación: `openspec list`
      no muestra `confinamiento-frontera-motor` y `openspec/specs/restauracion/spec.md` ya nombra `readlink -f`.

## 1. El troceador del volcado de PostgreSQL (módulo puro)

Módulo nuevo `src/engine/troceador.rs`: una máquina de estados **sin contenedores, sin red y sin E/S** que, línea a
línea, decide qué es frontera (`\connect`), qué es metacomando a descartar y qué es contenido. Es el subconjunto de
`psqlscan` que decide dónde empieza una línea de metacomando.

### RED

- [x] 1.1 Rust: test —un `\connect tienda` a primer nivel— emite una **frontera** con la base `tienda`
      (verificación: `cargo test --locked troceador` falla por símbolo inexistente).
- [x] 1.2 Rust: test —una línea `\connect x` dentro de una comilla simple, de una comilla doble, de un dólar
      `$tag$…$tag$`, de un comentario `--` y de un comentario `/* */` anidable— NO emite frontera y se reenvía
      como contenido (es el caso que un troceador por `^\connect` rompería).
- [x] 1.2b Rust: test —un literal `E'…\'…'` con un `\connect` dentro— NO emite frontera: la barra invertida escapa
      la comilla y el cliente lo lee como texto (verificado con `psql 18.6`). Un troceador que cerrara el literal en
      `\'` desviaría el resto de la sección a otra base: es el caso hostil, no una rareza.
- [x] 1.3 Rust: test —un bloque `COPY … FROM stdin;` con una línea de datos que empieza por `\connect` o por `\!`—
      NO emite frontera y **no descarta** esa línea: la guarda es el `\.` que cierra el bloque.
- [x] 1.4 Rust: test —una línea de metacomando a primer nivel que no sea `\connect` (`\!`, `\.` fuera de `COPY`,
      `\restrict`, `\unrestrict`, `\i`)— se **descarta**, y el `\.` que cierra un `COPY` **se conserva**.
- [x] 1.4b Rust: test —un metacomando **precedido de espacios o de un tabulador**— se descarta igual: el cliente lo
      ejecuta así (verificado con `psql 18.6`), de modo que «primer nivel» es el primer byte no blanco, no la columna cero.
- [x] 1.5 Rust: test —el estado léxico cruza líneas: una comilla simple abierta en una línea no se cierra con la
      siguiente, y un dólar `$fn$` abierto tampoco (verificación: un `\connect` en medio de un cuerpo multilínea
      no es frontera).

### GREEN

- [x] 1.6 Implementar el mínimo: la máquina de estados con las cinco piezas —comilla simple (`''`), comilla doble
      (`""`), dólar (`$tag$`), comentario de línea, comentario de bloque anidable— y el bloque `COPY`
      (verificación: `cargo test --locked troceador` en verde).
- [x] 1.7 Rust: test —un volcado real generado por `pg_dumpall` de dos bases— se trocea en tres secciones
      (una inicial y una por `\connect`), sin perder ni una línea de datos
      (verificación: la concatenación de las secciones conserva todo el contenido salvo los metacomandos descartados).

### REFACTOR

- [x] 1.8 Refactor sin cambiar comportamiento: nombres, `#[cfg(test)]` colocado y doc del módulo con el porqué
      (verificación: `cargo test --locked troceador` sigue en verde y `cargo clippy --all-targets -- -D warnings` limpio).

## 2. Los comandos endurecidos (`src/engine/restauracion.rs::comando`)

### RED

- [x] 2.1 Rust: test —un target `mariadb`— exige `mariadb --binary-mode` y el usuario, con y sin `db_name`
      (verificación: el test falla porque hoy falta `--binary-mode`).
- [x] 2.2 Rust: test —un target `sqlite`— exige que la reimportación sea `sqlite3 -safe <db_path>`
      (verificación: el test falla porque hoy falta `-safe`).
- [x] 2.3 Rust: test —un target `postgresql`— exige que el camino traiga el **reparto por secciones** con la
      plantilla `psql -X -U <usuario> -d` y la base inicial `postgres` cuando `db_name` es nulo, o `db_name`
      cuando no lo es (verificación: el test falla porque hoy hay una sola orden y falta `-X`).
- [x] 2.4 Rust: test —un target `postgresql` o `mariadb` con contraseña— exige que la credencial siga viajando
      **solo** por el entorno de la orden (verificación: el `cmd` no la contiene).

### GREEN

- [x] 2.5 `--binary-mode` en la orden de `mariadb` (verificación: 2.1 en verde).
- [x] 2.6 `-safe` en la orden de reimportación de `sqlite` (verificación: 2.2 en verde).
- [x] 2.7 `-X` y el reparto por secciones en `postgresql`: `CaminoDeRestauracion::PorContenedor` gana el campo
      `reparto`, con las variantes «el volcado entero, a la última orden» (mariadb, sqlite) y «una orden por
      sección» (postgresql, desde la plantilla y la base inicial) (verificación: 2.3 en verde).
- [x] 2.8 El `Debug` derivado de la variante nueva NO publica el valor de ninguna variable de entorno
      (verificación: test que imprime el camino con una contraseña y comprueba que no aparece).

### REFACTOR

- [x] 2.9 Actualizar el encabezado del módulo y los comentarios de `comando` con el porqué de cada modo
      (verificación: `cargo test --locked restauracion` y `cargo clippy` limpios).

## 3. La ejecución por secciones (`src/engine/restauracion.rs::ejecutar_las_ordenes`)

### RED

- [x] 3.1 Rust: test —un volcado con dos `\connect` y un reparto por secciones— ejecuta **tres** órdenes `psql`,
      una por base, y cada base recibe solo su sección (verificación: el test falla porque hoy hay una sola orden).
- [x] 3.2 Rust: test —una sección que falla— da la restauración por fallida aunque las demás acaben bien
      (verificación: el resultado es `failed` con el motivo del contenedor).
- [x] 3.3 Rust: test —un volcado mucho mayor que el búfer del puente, con reparto por secciones— retiene memoria
      acotada (verificación: el pico de bytes retenidos no crece con el tamaño total; se mide como ya se mide hoy
      en el escenario de memoria del puente).
- [x] 3.3b Rust: test —un `\connect` cuyo nombre empieza por `-`— exige que ese nombre se entregue como **valor** de
      `-d` y no como un argumento suelto (verificación: el `cmd` de la orden es `["psql","-X","-U",<usuario>,"-d",<base>]`).
- [x] 3.4 Rust: test —cada sección— lleva delante `\restrict <nonce>` con un `nonce` distinto por restauración
      (verificación: la orden recibe la línea y dos restauraciones no comparten el `nonce`).
- [x] 3.5 Rust: test —un volcado hostil con `\!`, `\restrict` y `\unrestrict` de primer nivel— NO entrega ninguno
      de esos metacomandos al `psql`, y el `\.` de un `COPY` sí (verificación: la entrada observada por la orden
      no contiene los metacomandos).

### GREEN

- [x] 3.6 Implementar el reparto por secciones: leer del túnel, trocear, y entregar cada sección al `stdin` de su
      propia orden `psql -X -U <usuario> -d <base>`, construida desde la plantilla (verificación: 3.1–3.5 en verde).
- [x] 3.7 El `nonce` de `\restrict` se genera con `rand` por restauración y no se deriva del volcado
      (verificación: test de que dos restauraciones dan nonces distintos).

### REFACTOR

- [x] 3.8 Refactor sin cambiar comportamiento: extraer el bucle de secciones a una función con nombre, y dejar
      `ejecutar_las_ordenes` como el despachador del reparto (verificación: `cargo test --locked restauracion` en verde).
- [x] 3.9 Doc del porqué en el módulo: por qué se trocea, por qué la capa 1 (ignorar) es la principal y la 2
      (`\restrict`) la de respaldo (verificación: `cargo doc`/`cargo clippy` sin avisos).

## 4. La batería de punta a punta (`tests/e2e/`)

### RED

- [x] 4.1 Rust: escenario —un volcado de PostgreSQL con dos bases sembradas— se restaura global y cada base
      conserva sus datos (verificación: `just e2e` —hoy la restauración global no existe como escenario).
- [x] 4.2 Rust: escenario —un origen con una tabla cuyos valores son texto que parece un metacomando
      (`\! …`, `\.shell`, `\connect …`)— los conserva **íntegros** en el destino tras el ciclo, para
      `postgresql` y `mariadb` (verificación: `just e2e`; fija que el descarte de metacomandos no se come datos).
- [x] 4.2b Rust: escenario —un target `postgresql` **sin** `db_name`— cubre el volcado global: se siembran dos
      bases en el origen, se copia con `pg_dumpall` y se restaura, y cada base queda con **sus** datos
      (verificación: `just e2e`; hoy el e2e no tiene ningún target postgres sin `db_name`, así que la
      restauración global no está cubierta de punta a punta).
- [x] 4.3 Arnés: el contenedor `sqlite` del e2e (`alpine` + `apk add sqlite`) trae un `sqlite3 ≥ 3.27` —para `-safe`—
      y `readlink` de busybox —para la comprobación de forma canónica del bloque B—; el `postgres:18-alpine` trae
      `psql ≥ 17.6` (verificación: los escenarios 4.1, 4.2 y 4.2b no se omiten por falta de herramienta, y la
      versión de `sqlite3` se comprueba en el arranque del contenedor, no se supone).

### GREEN

- [x] 4.4 Poner en verde 4.1–4.3 (verificación: `just e2e` completo, con el motor de contenedores de `DOCKER_HOST`).

### REFACTOR

- [x] 4.5 Refactor del arnés sin cambiar comportamiento: reutilizar el sembrado y la comparación de filas que ya
      existen en el escenario de postgres (verificación: `just e2e` sigue en verde).

## 5. La frontera de confianza, documentada

- [x] 5.1 `docs/ARQUITECTURA.md` declara que el **repositorio de copias es una frontera de confianza**, que el
      contenido restaurado se trata como no confiable, y el **residual aceptado**: `COPY … FROM PROGRAM` es SQL,
      no un metacomando, y solo lo admite un superusuario (verificación: la sección nombra los tres clientes y el residual).
- [x] 5.2 `docs/ARQUITECTURA.md` (o `docs/ENTORNO.md`) recoge el **requisito de cliente**: `sqlite3 ≥ 3.27` para
      `-safe`, y que `\restrict` exige `psql ≥ 17.6` —donde no esté, la capa 1 sigue siendo la guarda efectiva—
      (verificación: ambos números aparecen con su consecuencia).

## 6. Cierre

- [x] 6.1 `just check` en verde (verificación: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` y `cargo test --locked`).
- [x] 6.2 `cargo test --locked -- --ignored` con el motor de contenedores (verificación: termina sin error).
- [x] 6.3 `just e2e` (verificación: todos los escenarios, incluidos los nuevos, en verde).
- [x] 6.4 `just check-all` y `openspec validate endurecimiento-restauracion --strict` (verificación: sin error).
- [x] 6.5 Review de `@rust-reviewer` sobre el diff completo (verificación: sin hallazgos bloqueantes; las
      correcciones aplicadas y re-verificadas).
- [x] 6.6 PR a `development` con `Closes #164` (verificación: PR abierto y checks en verde).
- [x] 6.7 `plans/PLAN-041.md` completado y renombrado a `PLAN-041-COMPLETED.md` (verificación: `git mv` y tareas marcadas).
- [x] 6.8 `docs/FEATURE-041-endurecimiento-restauracion.md` escrito (verificación: cubre qué es, ficheros, cómo
      funciona, requisitos de cliente, decisiones y follow-ups).
- [x] 6.9 Change archivado tras fusionar el PR: rama `chore/archivar-endurecimiento-restauracion` con
      `just spec-archive endurecimiento-restauracion` (verificación: `openspec list` ya no lo muestra).

## 7. Correcciones tras la revisión del troceador y la capa 2 (SEC-08)

> Cierre de huecos verificados contra `psql 18.6` y endurecimiento de la capa 2. La capa 2 (`\restrict`)
> **deja de ser opcional**: es una precondición de ejecución y la restauración **falla** si el cliente
> no la honra.

### 7.1 El troceador, fiel a `psql`

- [x] 7.1.1 El juego de blancos de un metacomando es el **completo** de `psqlscan.l` (`[ \t\n\r\f\v]`), no
      solo espacio y tabulador: `\f\!`, `\v\!` y `\r\!` ejecutan en el cliente y tenían que descartarse
      (verificación: tests `un_metacomando_con_avance_de_pagina_...` y `un_connect_con_avance_de_pagina_...`).
- [x] 7.1.2 La detección de `COPY … FROM stdin` es el **puerto fiel** de `psqlscan_is_copy_from_stdin`:
      letra inicial de los identificadores de primer nivel (tope 8, sin los de dentro de paréntesis) y
      decisión al `;` a profundidad cero (verificación: tests de falso positivo en comentario y en
      paréntesis, cabecera partida en dos líneas, y `COPY` sin `;`).
- [x] 7.1.3 `\connect`: se acepta el alias `\c`; la palabra ha de terminar ahí (`\connectx`, `\cd` no son
      frontera); un `\connect` **sin nombre** no abre sección; el nombre entrecomillado resuelve `""`
      (verificación: tests `el_alias_c_abre_una_frontera`, `el_metacomando_cd_no_es_frontera`,
      `un_connect_sin_nombre_no_es_frontera`, `un_connect_con_comilla_escapada_en_el_nombre_es_frontera`).
- [x] 7.1.4 La etiqueta de un dólar admite el juego **no ASCII** de `psql` (`dolq_start`/`dolq_cont`)
      (verificación: test `un_dolar_con_etiqueta_no_ascii_no_deja_pasar_el_connect`).
- [x] 7.1.5 El residuo aceptado —la memoria acotada a la línea en curso, que un volcado con una línea
      gigante sin terminador infla el proceso— queda **dicho** en la doc del módulo, sin cambiar el
      comportamiento.

### 7.2 La capa 2, obligatoria

- [x] 7.2.1 `comprobar_que_el_cliente_soporta_restrict`: antes del bucle de secciones se ejecuta
      `<binario> --version` —sin credencial— y, si el cliente no honra `\restrict`, la restauración
      **falla** nombrando el contenedor, la versión y el mínimo (verificación: `ejecutar_por_secciones`
      la llama antes de leer una línea).
- [x] 7.2.2 `minimo_con_restrict` codifica el mínimo por rama del ciclo de parches de 2025-08-14
      (17.6, 16.10, 15.14, 14.19, 13.22; PG 18 de serie) y `soporta_restrict` es **fail-closed**
      (verificación: tests `soporta_restrict_aplica_el_minimo_de_cada_rama` y
      `soporta_restrict_es_fail_closed_sin_version`).
- [x] 7.2.3 Las pruebas del reparto instalan un **doble de cliente** honesto que contesta `--version`
      con un `psql 18.6` (verificación: los tests de secciones siguen en verde con la precondición).

### 7.3 La batería de punta a punta

- [x] 7.3.1 Escenario `un_psql_sin_restrict_falla_la_restauracion_sin_restaurar_nada` con una imagen de
      PostgreSQL por debajo del umbral (17.5-alpine, respaldo 16.9-alpine) y guarda de disponibilidad
      (verificación: la restauración acaba en `failed` nombrando el mínimo y no repone la base).

### 7.4 Documentación y spec

- [x] 7.4.1 `design.md` D5: la capa 2 es precondición obligatoria y la restauración falla sin ella, con
      el porqué; D4 recoge el puerto fiel de `COPY`, el juego de blancos y los bordes de `\connect`;
      D9 anota «degradarse en silencio» como alternativa descartada.
- [x] 7.4.2 `specs/restauracion/spec.md`: requisito nuevo «La restauración exige un cliente `psql` que
      honre `\restrict`» con su escenario, y escenarios nuevos del troceador (blancos, `COPY` falsos
      positivos, `\connect` sin nombre, alias `\c`).
- [x] 7.4.3 `docs/ARQUITECTURA.md` y `AGENTS.md`: `\restrict` es precondición **comprobada en tiempo de
      ejecución** y la restauración **falla** sin ella.

### 7.5 Correcciones de la segunda revisión (el vector conninfo y los comentarios)

> La segunda ronda encontró un vector **real** —el nombre de base del `\connect` puede ser una cadena de
> conexión— y varios comentarios que afirmaban cosas falsas. El vector se cierra **validando y
> rechazando**; el otro hallazgo (`ON_ERROR_STOP`) se aplaza a un issue aparte.

- [x] 7.5.1 `parece_cadena_de_conexion` (puro) y **rechazo fail-closed** en el reparto: una base que
      salga de un `\connect` y tenga forma de *conninfo* —un `=`, o `postgres://`/`postgresql://`—
      hace **fallar** la restauración sin ejecutar esa sección ni las siguientes; la `base_inicial` no
      se valida (la elige el operador), y el motivo no publica el valor (verificación:
      `parece_cadena_de_conexion_reconoce_la_forma_de_conninfo` y
      `un_connect_con_nombre_de_conninfo_falla_la_restauracion`).
- [x] 7.5.2 El comentario de `orden_de_una_seccion` deja de afirmar que hereda la guarda de la
      extracción: la posición tras `-d` solo evita que un nombre que empiece por `-` se lea como opción,
      y la forma de cadena de conexión se rechaza **aparte** (verificación:
      `un_nombre_de_base_que_empieza_por_guion_va_como_valor_de_d`).
- [x] 7.5.3 El comentario de `ejecutar_por_secciones` dice la garantía **real** —se falla cuando una
      orden sale con un código distinto de cero— y anota que los errores de SQL de dentro de una
      sección **no** se detectan sin `ON_ERROR_STOP`: **issue #175**.
- [x] 7.5.4 El comentario de `cierra_el_copy` ya no justifica el recorte con volcados que dejan
      espacios detrás: no los dejan, y `psql 18.6` **rechaza** ese marcador con `end-of-copy marker is
      not alone on its line` (verificado). El comportamiento no se toca.
- [x] 7.5.5 La divergencia de `E'…'` —`es_caracter_de_palabra` sin los bytes ≥ 0x80, y la heurística
      del byte anterior frente a la tokenización de `flex`— queda documentada en la doc del módulo como
      divergencia conocida, con el porqué de que no es fail-open (la capa 2 la cubre) ni alcanzable en
      un `pg_dumpall` real.
- [x] 7.5.6 Spec (`restauracion`), design (D5c), `docs/ARQUITECTURA.md`, `AGENTS.md` y
      `plans/PLAN-041.md` recogen el rechazo del nombre con forma de conninfo y el aplazamiento de
      `ON_ERROR_STOP` a **#175**.

### 7.6 Cierre de la revisión dirigida del delta conninfo

- [x] 7.6.1 Doc de `parece_cadena_de_conexion`: deja dicho que el prefijo se compara **sin distinguir
      mayúsculas** mientras libpq es sensible a mayúsculas y no salta blancos (`uri_prefix_length` /
      `recognized_connection_string` de `fe-connect.c`), así que la función es un **superconjunto**
      *fail-closed* y deliberado.
- [x] 7.6.2 El test de contenedor que prometía medir la posición de `-d` pasa a decir lo que mide
      —el nombre sobrevive el viaje hasta el cliente— y la posición la fija el test **puro** del
      `argv` (`un_nombre_de_base_que_empieza_por_guion_va_como_valor_de_d`).
- [x] 7.6.3 Test nuevo que ancla la asimetría deliberada: con una `base_inicial` que **parece**
      conninfo (`host=x`) la primera sección **sí** se ejecuta con esa base, porque la elige el
      operador y no viene del volcado (`la_base_inicial_no_se_valida_porque_la_elige_el_operador`).
- [x] 7.6.4 Test nuevo del camino del `\connect` hostil como **primera** línea del volcado: la
      restauración **falla** y la base hostil **no** llega a ejecutarse
      (`un_connect_hostil_en_la_primera_linea_falla_la_restauracion`).
- [x] 7.6.5 Spec (`restauracion`): el requisito y el escenario del nombre con forma de conninfo
      recogen la comparación **sin distinguir mayúsculas** (superconjunto deliberado), sin renombrar
      ningún escenario.
