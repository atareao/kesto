# Proposal — endurecimiento de la restauración

Issue **#164 (SEC-08)**, hijo de la auditoría de seguridad **#156** (bloque C de la campaña).

## Why

El volcado que se restaura sale de un repositorio **configurable por el operador** y se entrega **tal cual** al intérprete de la base, sin validar. El repositorio es una **frontera de confianza**: un repositorio comprometido —o sustituido— entrega un volcado con metacomandos que el cliente ejecuta dentro del contenedor destino, habitualmente como `root`.

Comprobado contra contenedores reales (`postgres:18-alpine`, `mariadb:11`, `sqlite3` local):

- `printf '.shell …' | sqlite3 <db>` ejecuta el dot-command por el `stdin`; la reimportación de `sqlite` (`src/engine/restauracion.rs`, `sqlite3 <db_path>`) es exactamente eso.
- `mariadb -u<db_user>` interpreta `\!` y el resto de comandos de cliente; es la orden de `mariadb` (`src/engine/restauracion.rs`).
- `psql -U <db_user> -d <db>` interpreta `\!`, y el SQL admite `COPY … FROM PROGRAM`; es la orden de `postgresql`.

Que la restauración exija un `snapshot_id` vinculado a una `job_execution` mitiga el caso de un repositorio ajeno, pero **no** el de un repositorio propio comprometido: el operador configura las credenciales y la frontera sigue estando ahí.

## What Changes

### SQLite: la reimportación ignora los dot-commands que escapan (`-safe`)

La reimportación SHALL ejecutarse como `sqlite3 -safe <db_path>`. El modo seguro de SQLite rechaza `.shell`, `.system`, `.read`, `.import`, `.output`, `.once`, `ATTACH` y `load_extension()` **antes** de ejecutarlos, y deja pasar el SQL del `.dump` —incluidos los `PRAGMA`— sin cambiar el resultado de una restauración legítima (verificado ida y vuelta sobre un `.dump` real).

El contenedor del target `sqlite` SHALL disponer de un `sqlite3` que soporte `-safe` (≥ 3.27); su ausencia SHALL reportarse como el fallo de la ejecución, no silenciarse.

### MariaDB: la reimportación ignora los comandos de cliente (`--binary-mode`)

La reimportación SHALL ejecutarse como `mariadb --binary-mode -u<db_user> [<db_name>]`. `--binary-mode` desactiva el análisis de todos los comandos de cliente salvo `\C` y `DELIMITER`, de modo que un `\!` del volcado SHALL rechazarse y NO SHALL ejecutarse, mientras que el SQL del volcado —`CREATE DATABASE`, `USE`, `INSERT`— entra igual (verificado).

### PostgreSQL: se ignoran los metacomandos y se restaura base por base

El volcado de PostgreSQL no se puede entregar a `psql` sin más. `pg_dumpall` separa las bases con metacomandos `\connect`, y la única forma que da `psql` de rechazar metacomandos —el modo restringido de `\restrict`— **bloquea `\connect`** y desviaría los datos a la base equivocada. Por eso la restauración SHALL:

- **Trocear** el volcado en secciones por los `\connect` de **primer nivel**, reconociendo comillas simples y dobles, dólares (`$tag$…$tag$`), comentarios y bloques `COPY`, de modo que un `\connect` dentro de un literal, de un cuerpo de función o de los datos de un `COPY` NO SHALL confundirse con una frontera.
- **Ignorar los metacomandos**: en cada sección SHALL descartarse toda línea de metacomando de primer nivel, conservando solo el terminador `\.` de los bloques `COPY`. Es exactamente el «modo de importación que ignore metacomandos» del issue.
- **Restaurar cada sección** con `psql -X -U <db_user> -d <base>`, donde `<base>` es la de cada `\connect` y, en la primera sección, `db_name` o `postgres`. A cada sección SHALL anteponerse `\restrict <nonce>` como segunda capa, con un `nonce` aleatorio por restauración.

La restauración por base NO SHALL materializar el volcado entero: las secciones SHALL entregarse al `stdin` de cada `psql` conforme se leen, con la memoria acotada.

### La frontera de confianza, documentada

`docs/ARQUITECTURA.md` SHALL declarar que el repositorio de copias es una **frontera de confianza**, que el contenido restaurado se trata como no confiable, y el **residual aceptado**: `COPY … FROM PROGRAM` es SQL —no un metacomando— y solo lo admite un superusuario; NO SHALL bloquearse, y queda documentado.

## Non-goals

- **No se bloquea `COPY … FROM PROGRAM`**: es SQL, no un metacomando, y exige superusuario en el servidor destino. Se documenta como residual.
- **No se cambia el formato del volcado ni la extracción**: el `.dump`, el `mariadb-dump` y el `pg_dumpall`/`pg_dump` siguen siendo los mismos; el endurecimiento es de la restauración.
- **No se añade separación de privilegio al motor**: es la otra mitad de SEC-06 y el issue de seguimiento **#173**.

## Impact

- **Deltas**: `restauracion` (los comandos endurecidos y el ignorado de metacomandos) y `verificacion-e2e` (los escenarios de la frontera probados de punta a punta).
- **Sin dependencias nuevas**: `-safe` es de `sqlite3`, `--binary-mode` de `mariadb` y `\restrict` de `psql`; las tres son opciones de cliente.
- **Requisito de cliente**: el `sqlite3` del contenedor ha de soportar `-safe` (≥ 3.27). `\restrict` exige `psql` ≥ 17.6; donde no esté, la línea se rechaza sin efecto y el ignorado de metacomandos sigue siendo la guarda efectiva.
- Bump esperado: **minor**.
