# PLAN-041 — Endurecimiento de la restauración

- **Issue**: #164 (SEC-08), hijo de la auditoría de seguridad #156 — bloque C de la campaña.
- **Rama**: `feature/endurecimiento-restauracion` (desde `development`).
- **Change de OpenSpec**: `openspec/changes/endurecimiento-restauracion/` (aprobado por el usuario antes de escribir código).
- **Documento de la feature**: `docs/FEATURE-041-endurecimiento-restauracion.md` (al cerrar).
- **Bump esperado**: minor.

## Objetivo

El volcado que se restaura sale de un repositorio S3 **configurable por el operador** y hoy se entrega tal cual al intérprete de la base, que corre como `root` dentro del contenedor destino. Un repositorio comprometido o sustituido ejecuta metacomandos de cliente (`sqlite3` dot-commands, `\!` de `mariadb`/`psql`) durante la restauración. El objetivo es tratar ese contenido como **no confiable**: cada cliente se invoca en el modo que le hace ignorar sus metacomandos, y el volcado de PostgreSQL se trocea por base para poder ignorarlos sin que `\connect` desvíe los datos a la base equivocada.

## Alcance aprobado por el usuario

Restauración por base, mitigación por cliente:

- `sqlite3 -safe` (≥ 3.27) en la reimportación.
- `mariadb --binary-mode` en la reimportación.
- `psql -X` + troceado del volcado por `\connect` de primer nivel + descarte de todo metacomando de primer nivel + `\restrict <nonce>` como segunda capa **obligatoria** (la restauración comprueba `psql --version` y falla si el cliente no la honra), con una orden `psql -d <base>` por sección.

## Fuera de alcance

- No se bloquea `COPY … FROM PROGRAM`: es SQL, no un metacomando, y exige superusuario. Residual aceptado y documentado.
- No se cambia el formato del volcado ni la extracción (siguen `pg_dump`/`pg_dumpall`, `mariadb-dump`, `.dump`).
- No se añade separación de privilegio al motor: es la otra mitad de SEC-06 y el issue de seguimiento #173.

## Dependencia de orden (bloqueante)

El bloque B (`confinamiento-frontera-motor`, PR #174) modifica **el mismo** requisito de la capability `restauracion`. Este delta está redactado sobre el texto **post-B** (incluye la comprobación de `db_path` canónico con `readlink -f` y el escenario «Un db_path que no es canónico se rechaza antes de borrar»). Por tanto: **el PR #174 ha de estar fusionado y el change B archivado antes de aplicar este change**.

## Ficheros implicados

| Fichero | Qué cambia |
|---|---|
| `src/engine/troceador.rs` (nuevo) | Módulo **puro**: máquina de estados léxica que trocea el volcado y decide frontera / metacomando / contenido. |
| `src/engine/restauracion.rs` | `comando()` añade `--binary-mode`, `-safe`, `-X` y el campo `reparto`; `ejecutar_las_ordenes` despacha por reparto y ejecuta sección a sección. |
| `src/engine/mod.rs` | El puente `tunel_de_restauracion` y la llamada a `ejecutar_las_ordenes` con el `reparto`. |
| `tests/e2e/escenarios.rs` | Escenarios nuevos: restauración global por base y metacomando que no ejecuta. |
| `tests/e2e/arnes.rs` | Contenedor `sqlite` con `sqlite3 ≥ 3.27` y `readlink`; imágenes ya declaradas. |
| `docs/ARQUITECTURA.md` | Frontera de confianza, residual `COPY … FROM PROGRAM` y requisitos de cliente. |
| `openspec/changes/endurecimiento-restauracion/` | proposal, design, deltas (`restauracion`, `verificacion-e2e`) y tasks. |

## Modelo de ejecución (decisión)

`CaminoDeRestauracion::PorContenedor` gana un campo `reparto`:

- `Reparto::ALaUltimaOrden` — comportamiento de hoy: `mariadb` (una orden) y `sqlite` (retirar + reimportar, el volcado a la última).
- `Reparto::PorSeccionesDePostgres { plantilla, base_inicial }` — `plantilla` es una `Orden` con `cmd` = `psql -X -U <usuario> -d` sin base y el entorno con `PGPASSWORD`; cada sección completa el `cmd` con su base. Se guarda como `Orden` a propósito: su `Debug` publica los **nombres** de las variables de entorno y nunca sus valores, así que la contraseña no se filtra.

Las bases no se conocen hasta leer el volcado, así que las órdenes de las secciones se construyen al vuelo desde la plantilla. `ejecutar_las_ordenes` despacha según el `reparto`; en `PorSeccionesDePostgres` ejecuta la primera sección contra `base_inicial` (`db_name` o `postgres`) y una por cada frontera `\connect`.

## Tareas

Las tareas TDD completas, en orden rojo/verde/refactor y con su verificación, están en `openspec/changes/endurecimiento-restauracion/tasks.md` (secciones 0 a 7). Resumen:

0. **Preparación** — `just check` en verde, rama, change aprobado, y comprobar que B está archivado (0.4).
1. **El troceador** (`src/engine/troceador.rs`, módulo puro) — reconocer comilla simple, comilla doble, dólar `$tag$`, comentario de línea, comentario de bloque anidable y bloque `COPY`; frontera solo por `\connect` de primer nivel; descartar metacomandos de primer nivel conservando el `\.` del `COPY`; el estado léxico cruza líneas; trocear un `pg_dumpall` real de dos bases sin perder una línea.
2. **Los comandos endurecidos** (`comando()`) — `--binary-mode`, `-safe`, `-X`, el campo `reparto` y su `Debug` sin valores de entorno.
3. **La ejecución por secciones** (`ejecutar_las_ordenes`) — una orden por base, fallo de una sección falla la restauración, memoria acotada, `\restrict <nonce>` por restauración con `nonce` de `rand`, y ningún metacomando hostil entregado al `psql`.
4. **La batería de punta a punta** — escenario de restauración global por base y escenario de metacomando que no ejecuta en los tres clientes.
5. **La frontera de confianza, documentada** — `docs/ARQUITECTURA.md`: frontera de confianza, residual `COPY … FROM PROGRAM`, `sqlite3 ≥ 3.27` y `psql ≥ 17.6`.
6. **Cierre** — `just check`, `cargo test --locked -- --ignored`, `just e2e`, `just check-all`, `openspec validate --strict`, review de `@rust-reviewer`, PR a `development` con `Closes #164`, plan completado, doc de la feature y archivado del change por PR (`chore/archivar-endurecimiento-restauracion`).
7. **Correcciones tras la revisión** — puerto fiel del troceador (blancos `[ \t\n\r\f\v]`, detección de `COPY` como `psql`, alias `\c`, `\connect` sin nombre, dólar no ASCII) y la capa 2 como precondición obligatoria (`psql --version` antes de restaurar, fallo sin `\restrict`), con escenario e2e negativo y la documentación y la spec actualizadas.

## Verificación

- `just check` (fmt + clippy `-D warnings` + `cargo test --locked`).
- `cargo test --locked -- --ignored` con el motor de contenedores (`podman`, vía `DOCKER_HOST`).
- `just e2e` (todos los escenarios, incluidos los nuevos).
- `just check-all` y `openspec validate endurecimiento-restauracion --strict`.
- Review de `@rust-reviewer` sobre el diff completo.

## Riesgos y decisiones asociadas

| Riesgo | Mitigación |
|---|---|
| Un troceador por línea rompe un volcado legítimo (`\connect` dentro de un dólar o comilla). | Máquina de estados léxica, con los casos límite como tests (tarea 1.2, 1.5). |
| `\restrict` no existe en `psql < 17.6`. | **Precondición obligatoria**: la restauración comprueba `psql --version` antes de leer el volcado y **falla** nombrando el mínimo si el cliente no lo honra. El descarte de metacomandos (capa 1) es la guarda principal; `\restrict` es la capa 2 y ya no es opcional. |
| Un troceador que buscara la subcadena `FROM stdin` daría falsos positivos (`COPY … TO STDOUT; -- FROM stdin`) y falsos negativos (cabecera partida en dos líneas). | Detección **fiel** a `psql`: letra inicial de los identificadores de primer nivel, tope ocho, sin los de paréntesis, y decisión al `;` a profundidad cero (tarea 7.1.2). |
| El juego de blancos delante de un metacomando no es solo `[ \t]`: `\f`, `\v` y `\r` también lo preceden y el cliente los ejecuta. | El troceador usa el juego completo `[ \t\n\r\f\v]` (tarea 7.1.1). |
| El `\unrestrict` de un volcado hostil deshace la capa 2. | El descarte de metacomandos se lleva también el `\unrestrict` (escenario «El `\unrestrict` del volcado no deshace la guarda»). |
| El nombre de base del `\connect` puede tener forma de **cadena de conexión** (`=`, `postgres://`): `psql -d` lo leería como *conninfo* y desviaría la conexión —con `PGPASSWORD` puesto— al servidor del atacante. | **Fail-closed**: `parece_cadena_de_conexion` y rechazo **antes** de componer la orden de la sección; la restauración falla sin ejecutar esa sección ni las siguientes, y el motivo no publica el valor (tarea 7.5.1, D5c). |
| ¿La comprobación del esquema coincide **exactamente** con lo que libpq expande? | No, y es deliberado: se compara **sin distinguir mayúsculas**, así que es un **superconjunto** de libpq (`POSTGRES://…` se rechaza aunque libpq lo trataría como nombre plano). *Fail-closed*; el límite queda en la doc de `parece_cadena_de_conexion` y en el spec (tareas 7.6.1 y 7.6.5). |
| ¿Se valida también la `base_inicial`? | No: la elige el operador en la configuración del target, no viene del volcado; un test ancla la asimetría para que nadie la «arregle» (tarea 7.6.3). |
| Los errores de SQL **dentro** de una sección no se detectan: `psql` sin `ON_ERROR_STOP` sale con `0`. | Aplazado a **#175**; la garantía real —«se falla cuando una orden sale con un código distinto de cero»— queda escrita en la doc de `ejecutar_por_secciones` (tarea 7.5.3). |
| `COPY … FROM PROGRAM` ejecuta en el servidor. | Residual aceptado y documentado; su sitio es la separación de privilegio del motor (#173). |
| Un `sqlite3` sin `-safe` falla la opción. | Fail-closed: la orden falla y el error del contenedor se registra, no se silencia. |
| El cambio de forma de `CaminoDeRestauracion` toca `ejecutar_las_ordenes` y el puente. | El reparto por defecto (`ALaUltimaOrden`) conserva el comportamiento actual; los tests existentes de `mariadb` y `sqlite` son la red. |

## Follow-ups

- #173 — separación de privilegio del motor (cubre `COPY … FROM PROGRAM`).
- **#175 — `ON_ERROR_STOP`**: `psql` sin `ON_ERROR_STOP` sale con `0` ante un error de SQL, así que los errores **dentro** de una sección **no** se detectan y una sección con sentencias fallidas se da por buena. La garantía actual es «se falla cuando una orden sale con un código distinto de cero»; decidir la política (¿aborta la restauración entera ante el primer error de SQL?) va en #175. Cerrado el vector del nombre de base con forma de conninfo en la tarea 7.5.1 (D5c).
- #171 y el flake del e2e (`502 Bad Gateway` de S3/SeaweedFS).
- Mejora posible anotada y descartada: volcado en formato custom (`pg_dump -Fc`) restaurado con `pg_restore`, inmune a metacomandos por construcción; cambiaría el formato almacenado y sale del alcance de la extracción.
- **Tope por línea del troceador** (tarea 7.1.5): la memoria está acotada a la línea en curso, pero una línea gigante sin `\n` la acumula entera. Residuo aceptado; su sitio es un tope por línea, no el módulo.
- **Anclar el troceador a `psqlscan.l`**: los bordes de `COPY`, blancos y `\connect` se han fijado contra `psql 18.6`; si PostgreSQL cambia su lexer, el puerto habría que revisarlo. Es la razón de que el troceador sea un puerto y no una heurística.
