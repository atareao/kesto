# Tasks — endurecimiento-restauracion

> Issue **#164 (SEC-08)**, hijo de la auditoría de seguridad **#156** (bloque C).
> Change de **producto**: motor (restauración), un módulo **puro** nuevo, el arnés de punta a punta y la documentación.
> Decisiones y alternativas descartadas, en `design.md`. TDD: primero el test que hoy falla (rojo), luego el mínimo
> (verde), luego el refactor. **Sin dependencias nuevas**: `rand` y `uuid` ya están en el árbol.
> Plan: `plans/PLAN-041.md`. Documento de la feature: `docs/FEATURE-041-endurecimiento-restauracion.md`.

## 0. Preparación

- [ ] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [ ] 0.2 Rama desde `development`: `feature/endurecimiento-restauracion` (verificación: `git branch --show-current`).
- [ ] 0.3 Change aprobado por el usuario (verificación: aprobación explícita y `just check-spec` encuentra el change activo).
- [ ] 0.4 **Orden respecto al bloque B**: los dos changes modifican el **mismo** requisito de la capability `restauracion`.
      Este delta se redacta sobre el texto **post-B** (con la comprobación de `db_path` canónico), así que B
      —PR #174— ha de estar **fusionado y archivado** antes de aplicar este change. Verificación: `openspec list`
      no muestra `confinamiento-frontera-motor` y `openspec/specs/restauracion/spec.md` ya nombra `readlink -f`.

## 1. El troceador del volcado de PostgreSQL (módulo puro)

Módulo nuevo `src/engine/troceador.rs`: una máquina de estados **sin contenedores, sin red y sin E/S** que, línea a
línea, decide qué es frontera (`\connect`), qué es metacomando a descartar y qué es contenido. Es el subconjunto de
`psqlscan` que decide dónde empieza una línea de metacomando.

### RED

- [ ] 1.1 Rust: test —un `\connect tienda` a primer nivel— emite una **frontera** con la base `tienda`
      (verificación: `cargo test --locked troceador` falla por símbolo inexistente).
- [ ] 1.2 Rust: test —una línea `\connect x` dentro de una comilla simple, de una comilla doble, de un dólar
      `$tag$…$tag$`, de un comentario `--` y de un comentario `/* */` anidable— NO emite frontera y se reenvía
      como contenido (es el caso que un troceador por `^\connect` rompería).
- [ ] 1.3 Rust: test —un bloque `COPY … FROM stdin;` con una línea de datos que empieza por `\connect` o por `\!`—
      NO emite frontera y **no descarta** esa línea: la guarda es el `\.` que cierra el bloque.
- [ ] 1.4 Rust: test —una línea de metacomando a primer nivel que no sea `\connect` (`\!`, `\.` fuera de `COPY`,
      `\restrict`, `\unrestrict`, `\i`)— se **descarta**, y el `\.` que cierra un `COPY` **se conserva**.
- [ ] 1.5 Rust: test —el estado léxico cruza líneas: una comilla simple abierta en una línea no se cierra con la
      siguiente, y un dólar `$fn$` abierto tampoco (verificación: un `\connect` en medio de un cuerpo multilínea
      no es frontera).

### GREEN

- [ ] 1.6 Implementar el mínimo: la máquina de estados con las cinco piezas —comilla simple (`''`), comilla doble
      (`""`), dólar (`$tag$`), comentario de línea, comentario de bloque anidable— y el bloque `COPY`
      (verificación: `cargo test --locked troceador` en verde).
- [ ] 1.7 Rust: test —un volcado real generado por `pg_dumpall` de dos bases— se trocea en tres secciones
      (una inicial y una por `\connect`), sin perder ni una línea de datos
      (verificación: la concatenación de las secciones conserva todo el contenido salvo los metacomandos descartados).

### REFACTOR

- [ ] 1.8 Refactor sin cambiar comportamiento: nombres, `#[cfg(test)]` colocado y doc del módulo con el porqué
      (verificación: `cargo test --locked troceador` sigue en verde y `cargo clippy --all-targets -- -D warnings` limpio).

## 2. Los comandos endurecidos (`src/engine/restauracion.rs::comando`)

### RED

- [ ] 2.1 Rust: test —un target `mariadb`— exige `mariadb --binary-mode` y el usuario, con y sin `db_name`
      (verificación: el test falla porque hoy falta `--binary-mode`).
- [ ] 2.2 Rust: test —un target `sqlite`— exige que la reimportación sea `sqlite3 -safe <db_path>`
      (verificación: el test falla porque hoy falta `-safe`).
- [ ] 2.3 Rust: test —un target `postgresql`— exige que el camino traiga el **reparto por secciones** con la
      plantilla `psql -X -U <usuario> -d` y la base inicial `postgres` cuando `db_name` es nulo, o `db_name`
      cuando no lo es (verificación: el test falla porque hoy hay una sola orden y falta `-X`).
- [ ] 2.4 Rust: test —un target `postgresql` o `mariadb` con contraseña— exige que la credencial siga viajando
      **solo** por el entorno de la orden (verificación: el `cmd` no la contiene).

### GREEN

- [ ] 2.5 `--binary-mode` en la orden de `mariadb` (verificación: 2.1 en verde).
- [ ] 2.6 `-safe` en la orden de reimportación de `sqlite` (verificación: 2.2 en verde).
- [ ] 2.7 `-X` y el reparto por secciones en `postgresql`: `CaminoDeRestauracion::PorContenedor` gana el campo
      `reparto`, con las variantes «el volcado entero, a la última orden» (mariadb, sqlite) y «una orden por
      sección» (postgresql, desde la plantilla y la base inicial) (verificación: 2.3 en verde).
- [ ] 2.8 El `Debug` derivado de la variante nueva NO publica el valor de ninguna variable de entorno
      (verificación: test que imprime el camino con una contraseña y comprueba que no aparece).

### REFACTOR

- [ ] 2.9 Actualizar el encabezado del módulo y los comentarios de `comando` con el porqué de cada modo
      (verificación: `cargo test --locked restauracion` y `cargo clippy` limpios).

## 3. La ejecución por secciones (`src/engine/restauracion.rs::ejecutar_las_ordenes`)

### RED

- [ ] 3.1 Rust: test —un volcado con dos `\connect` y un reparto por secciones— ejecuta **tres** órdenes `psql`,
      una por base, y cada base recibe solo su sección (verificación: el test falla porque hoy hay una sola orden).
- [ ] 3.2 Rust: test —una sección que falla— da la restauración por fallida aunque las demás acaben bien
      (verificación: el resultado es `failed` con el motivo del contenedor).
- [ ] 3.3 Rust: test —un volcado mucho mayor que el búfer del puente, con reparto por secciones— retiene memoria
      acotada (verificación: el pico de bytes retenidos no crece con el tamaño total; se mide como ya se mide hoy
      en el escenario de memoria del puente).
- [ ] 3.4 Rust: test —cada sección— lleva delante `\restrict <nonce>` con un `nonce` distinto por restauración
      (verificación: la orden recibe la línea y dos restauraciones no comparten el `nonce`).
- [ ] 3.5 Rust: test —un volcado hostil con `\!`, `\restrict` y `\unrestrict` de primer nivel— NO entrega ninguno
      de esos metacomandos al `psql`, y el `\.` de un `COPY` sí (verificación: la entrada observada por la orden
      no contiene los metacomandos).

### GREEN

- [ ] 3.6 Implementar el reparto por secciones: leer del túnel, trocear, y entregar cada sección al `stdin` de su
      propia orden `psql -X -U <usuario> -d <base>`, construida desde la plantilla (verificación: 3.1–3.5 en verde).
- [ ] 3.7 El `nonce` de `\restrict` se genera con `rand` por restauración y no se deriva del volcado
      (verificación: test de que dos restauraciones dan nonces distintos).

### REFACTOR

- [ ] 3.8 Refactor sin cambiar comportamiento: extraer el bucle de secciones a una función con nombre, y dejar
      `ejecutar_las_ordenes` como el despachador del reparto (verificación: `cargo test --locked restauracion` en verde).
- [ ] 3.9 Doc del porqué en el módulo: por qué se trocea, por qué la capa 1 (ignorar) es la principal y la 2
      (`\restrict`) la de respaldo (verificación: `cargo doc`/`cargo clippy` sin avisos).

## 4. La batería de punta a punta (`tests/e2e/`)

### RED

- [ ] 4.1 Rust: escenario —un volcado de PostgreSQL con dos bases sembradas— se restaura global y cada base
      conserva sus datos (verificación: `just e2e` —hoy la restauración global no existe como escenario).
- [ ] 4.2 Rust: escenario —un volcado con un metacomando de cliente que escribiría un fichero de prueba en el
      contenedor— restaura los datos y **no** crea el fichero, para los tres clientes (sqlite, mariadb, postgres)
      (verificación: `just e2e` falla hoy en los tres).
- [ ] 4.3 Arnés: `tests/e2e/arnes.rs` dispone de las imágenes ya declaradas (`postgres:18-alpine`, `mariadb:11`,
      `alpine:3.23`) y de un contenedor `sqlite` con `sqlite3 ≥ 3.27` y `readlink` (verificación: los escenarios
      4.1 y 4.2 no se omiten por falta de imagen o de herramienta).

### GREEN

- [ ] 4.4 Poner en verde 4.1–4.3 (verificación: `just e2e` completo, con el motor de contenedores de `DOCKER_HOST`).

### REFACTOR

- [ ] 4.5 Refactor del arnés sin cambiar comportamiento: reutilizar el sembrado y la comparación de filas que ya
      existen en el escenario de postgres (verificación: `just e2e` sigue en verde).

## 5. La frontera de confianza, documentada

- [ ] 5.1 `docs/ARQUITECTURA.md` declara que el **repositorio de copias es una frontera de confianza**, que el
      contenido restaurado se trata como no confiable, y el **residual aceptado**: `COPY … FROM PROGRAM` es SQL,
      no un metacomando, y solo lo admite un superusuario (verificación: la sección nombra los tres clientes y el residual).
- [ ] 5.2 `docs/ARQUITECTURA.md` (o `docs/ENTORNO.md`) recoge el **requisito de cliente**: `sqlite3 ≥ 3.27` para
      `-safe`, y que `\restrict` exige `psql ≥ 17.6` —donde no esté, la capa 1 sigue siendo la guarda efectiva—
      (verificación: ambos números aparecen con su consecuencia).

## 6. Cierre

- [ ] 6.1 `just check` en verde (verificación: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` y `cargo test --locked`).
- [ ] 6.2 `cargo test --locked -- --ignored` con el motor de contenedores (verificación: termina sin error).
- [ ] 6.3 `just e2e` (verificación: todos los escenarios, incluidos los nuevos, en verde).
- [ ] 6.4 `just check-all` y `openspec validate endurecimiento-restauracion --strict` (verificación: sin error).
- [ ] 6.5 Review de `@rust-reviewer` sobre el diff completo (verificación: sin hallazgos bloqueantes; las
      correcciones aplicadas y re-verificadas).
- [ ] 6.6 PR a `development` con `Closes #164` (verificación: PR abierto y checks en verde).
- [ ] 6.7 `plans/PLAN-041.md` completado y renombrado a `PLAN-041-COMPLETED.md` (verificación: `git mv` y tareas marcadas).
- [ ] 6.8 `docs/FEATURE-041-endurecimiento-restauracion.md` escrito (verificación: cubre qué es, ficheros, cómo
      funciona, requisitos de cliente, decisiones y follow-ups).
- [ ] 6.9 Change archivado tras fusionar el PR: rama `chore/archivar-endurecimiento-restauracion` con
      `just spec-archive endurecimiento-restauracion` (verificación: `openspec list` ya no lo muestra).
