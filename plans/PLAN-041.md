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
- `psql -X` + troceado del volcado por `\connect` de primer nivel + descarte de todo metacomando de primer nivel + `\restrict <nonce>` como segunda capa, con una orden `psql -d <base>` por sección.

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

Las tareas TDD completas, en orden rojo/verde/refactor y con su verificación, están en `openspec/changes/endurecimiento-restauracion/tasks.md` (secciones 0 a 6). Resumen:

0. **Preparación** — `just check` en verde, rama, change aprobado, y comprobar que B está archivado (0.4).
1. **El troceador** (`src/engine/troceador.rs`, módulo puro) — reconocer comilla simple, comilla doble, dólar `$tag$`, comentario de línea, comentario de bloque anidable y bloque `COPY`; frontera solo por `\connect` de primer nivel; descartar metacomandos de primer nivel conservando el `\.` del `COPY`; el estado léxico cruza líneas; trocear un `pg_dumpall` real de dos bases sin perder una línea.
2. **Los comandos endurecidos** (`comando()`) — `--binary-mode`, `-safe`, `-X`, el campo `reparto` y su `Debug` sin valores de entorno.
3. **La ejecución por secciones** (`ejecutar_las_ordenes`) — una orden por base, fallo de una sección falla la restauración, memoria acotada, `\restrict <nonce>` por restauración con `nonce` de `rand`, y ningún metacomando hostil entregado al `psql`.
4. **La batería de punta a punta** — escenario de restauración global por base y escenario de metacomando que no ejecuta en los tres clientes.
5. **La frontera de confianza, documentada** — `docs/ARQUITECTURA.md`: frontera de confianza, residual `COPY … FROM PROGRAM`, `sqlite3 ≥ 3.27` y `psql ≥ 17.6`.
6. **Cierre** — `just check`, `cargo test --locked -- --ignored`, `just e2e`, `just check-all`, `openspec validate --strict`, review de `@rust-reviewer`, PR a `development` con `Closes #164`, plan completado, doc de la feature y archivado del change por PR (`chore/archivar-endurecimiento-restauracion`).

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
| `\restrict` no existe en `psql < 17.6` (fail-open). | La guarda principal es el **descarte** de metacomandos (capa 1), que no depende de la versión; `\restrict` es la capa 2. |
| El `\unrestrict` de un volcado hostil deshace la capa 2. | El descarte de metacomandos se lleva también el `\unrestrict` (escenario «El `\unrestrict` del volcado no deshace la guarda»). |
| `COPY … FROM PROGRAM` ejecuta en el servidor. | Residual aceptado y documentado; su sitio es la separación de privilegio del motor (#173). |
| Un `sqlite3` sin `-safe` falla la opción. | Fail-closed: la orden falla y el error del contenedor se registra, no se silencia. |
| El cambio de forma de `CaminoDeRestauracion` toca `ejecutar_las_ordenes` y el puente. | El reparto por defecto (`ALaUltimaOrden`) conserva el comportamiento actual; los tests existentes de `mariadb` y `sqlite` son la red. |

## Follow-ups

- #173 — separación de privilegio del motor (cubre `COPY … FROM PROGRAM`).
- #171 y el flake del e2e (`502 Bad Gateway` de S3/SeaweedFS).
- Mejora posible anotada y descartada: volcado en formato custom (`pg_dump -Fc`) restaurado con `pg_restore`, inmune a metacomandos por construcción; cambiaría el formato almacenado y sale del alcance de la extracción.
