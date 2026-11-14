# FEATURE-014 — SQLite como motor de copia

**Issue:** [#11](https://github.com/atareao/kesto/issues/11) · **Plan:** `plans/PLAN-014.md` · **Change OpenSpec:** `openspec/changes/sqlite-copy-engine/`

## Qué es

Añade **SQLite** como tipo de origen de copia, junto a `mariadb`, `postgresql` y `directory`. Es la base de datos de buena parte del software autoalojado (Vaultwarden, Immich, Miniflux, tududi…), y hasta ahora quien lo alojaba dependía de scripts sueltos o de un `cp` del fichero, a menudo **en caliente y sin consistencia garantizada**: con WAL activo el `.db` puede ir por detrás de su `.wal`, y la copia resultante puede restaurarse con corrupción o con datos perdidos sin que nada lo delate.

La copia es **consistente en caliente** mediante `sqlite3 <db_path> .dump` ejecutado **dentro del contenedor del target**, con el `stdout` en streaming al repositorio `rustic` por el mismo camino que los volcados de MariaDB y PostgreSQL: **sin ficheros temporales** y sin detener el servicio que usa la base.

Un target `sqlite` se describe con `container_name` —el contenedor donde vive el fichero— y `db_path` —la ruta absoluta del `.db` dentro de él— y **no** usa `db_user`, `db_password`, `db_name` ni `host_path`.

## Ficheros implicados

- `migrations/0003_sqlite_target.sql` — amplía el `CHECK` de `target_type` y añade `db_path`, **reconstruyendo** `backup_targets`.
- `src/db/migrate.rs` — aplica las migraciones con `foreign_keys` apagado en la conexión y fuera de transacción, y comprueba después que no queden filas huérfanas.
- `src/db/entities/target.rs` — variante `TipoDeTarget::Sqlite`, columna `db_path` en los `SELECT`/`INSERT`/`UPDATE` y el mapeo de la fila.
- `src/db/entities/{execution,job,repository}.rs` · `src/api/{executions,jobs}.rs` · `src/state.rs` · `src/scheduler/pruebas.rs` — propagación de `db_path` en los tipos que viajan con el target.
- `src/api/targets.rs` — coherencia de los campos del target para `sqlite` y `db_path` en los DTO del cable.
- `src/engine/extraccion.rs` — `Objetivo.db_path` y la rama `Sqlite` de `ruta()`: `Ruta::PorContenedor` con `["sqlite3", <db_path>, ".dump"]`.
- `ui/src/services/api/types.ts` · `ui/src/components/ModalDeTarget/ModalDeTarget.tsx` · `ui/src/pages/ajustes.tsx` — opción `sqlite` y campos condicionales del formulario de fuente.
- `docs/DEFINITION.md` (§3 y §4) · `compose.yaml` — SRS y ejemplo de despliegue.

## Cómo funciona

### La copia: `.dump`, no `cp`

`extraccion::ruta()` decide **por dónde** sale la copia sin tocar el sistema de ficheros. Para un target `sqlite` devuelve una `Ruta::PorContenedor` cuyo comando es:

```sh
sqlite3 <db_path> .dump
```

- `.dump` produce un **volcado SQL** —las sentencias que reconstruyen el esquema y los datos—, no una copia de bytes. Es una lectura **consistente** (transacción de lectura), válida con el **modo WAL** activo y con la base **en uso**: no hay que parar el servicio.
- El volcado sale por el `stdout` del `exec` de bollard y **viaja en streaming** hasta `rustic_core`, sin fichero temporal en Kesto (§4). Se restaura de forma trivial: `sqlite3 nueva.db < copia.sql`.
- Por qué **no** un `cp`: copiar el `.db` —y su `-wal`— mientras la base está abierta no es copiar la base, es copiar bytes en un instante sin garantía de consistencia. Con WAL, el `.db` puede ir por detrás de su `.wal` y el resultado se puede restaurar con corrupción sin que nada lo delate.
- La ruta viaja como **primer argumento posicional** de `sqlite3`, así que un `db_path` que empezara por `-` se leería como una opción (`-help`, `-version`). La guarda `sin_ser_bandera` lo rechaza con el mismo criterio que el usuario y el nombre de base de los targets de cliente/servidor.
- Un fichero SQLite no tiene usuario ni contraseña: el entorno del `exec` va **vacío** por diseño, y no hay credencial que ocultar en la línea de comandos.

### El esquema y la migración `0003`

- `backup_targets` gana la columna `db_path` (`TEXT`, anulable: `NULL` en cualquier target que no sea `sqlite`) y su `CHECK` pasa a `target_type IN ('mariadb', 'postgresql', 'directory', 'sqlite')`.
- SQLite **no** permite modificar un `CHECK` con `ALTER TABLE`. La migración **reconstruye** la tabla —crear la nueva con el `CHECK` ampliado y la columna, copiar las filas columna a columna, borrar la vieja y renombrar—, que es el procedimiento canónico; preserva todas las filas (con `db_path` a `NULL`) y recrea los índices.
- El `DROP TABLE` de la vieja es lo delicado: con las claves foráneas encendidas dispararía el `ON DELETE CASCADE` de `backup_jobs` y se llevaría por delante sus filas antes de copiar nada. Por eso `db::migrate::aplicar` corre las migraciones con `foreign_keys` apagado —en la **conexión** y fuera de transacción, que es la única forma de que el pragma surta efecto—, vuelve a encenderlo y comprueba con `pragma_foreign_key_check` que no quedaran huérfanos. La FK de `backup_jobs` apunta a `backup_targets` **por nombre**, y ese nombre vuelve a existir, así que la tabla hija no se toca.

### El contrato: coherencia en el borde HTTP

La tabla es permisiva (columnas anulables y un único `CHECK`), así que la coherencia se exige en el alta. `target_type = sqlite` **exige** `container_name` y `db_path`, y **prohíbe** `db_user`, `db_password`, `db_name` y `host_path`: una fuente incompleta o con campos de otro tipo se rechaza con `400` **nombrando el campo** (por ejemplo, un `db_path` en blanco se rechaza nombrando `db_path`), en vez de dejar una fila que el motor no sabrá usar y que fallará en la primera copia con un error sobre un contenedor que no existe.

### El panel

El selector de tipo de fuente de Ajustes ofrece los cuatro valores; al elegir `sqlite` muestra `container_name` y `db_path` y **oculta** `host_path`, `db_user`, `db_password` y `db_name`. `db_path` es un texto normal, **no** un secreto.

## Cómo habilitar y desplegar

Sin variables de entorno nuevas. El único requisito operativo es del **contenedor del target**:

- **Debe traer el binario `sqlite3`** —lo mismo que un target MariaDB exige `mariadb-dump`—. Si la imagen no lo incluye, añade una capa que lo instale (`apk add --no-cache sqlite` / `apt-get install -y sqlite3`) o usa una imagen que ya lo traiga. Su ausencia se reporta como el fallo de la extracción, con el error del contenedor **leído y no perdido**.
- La fuente **no se monta** en Kesto ni se lee del host: la copia ocurre dentro del contenedor del target, a través del socket del daemon. Basta declarar el target con `container_name` + `db_path`.

Alta desde el panel (**Ajustes → Fuentes**, tipo SQLite) o por API:

```sh
curl -fsS -X POST http://localhost:8080/api/v1/targets \
  -H 'content-type: application/json' \
  -d '{"name":"vaultwarden","target_type":"sqlite",
       "container_name":"vaultwarden","db_path":"/data/db.sqlite"}'
```

`compose.yaml` trae un ejemplo comentado de servicio autoalojado con su SQLite y la equivalencia por API.

## Cómo verificar

- `just check` — fmt + clippy + tests: esquema (`CHECK` admite `'sqlite'` y rechaza `'sqlite3'`; las filas e índices sobreviven a la migración), coherencia del alta, `ruta()` (`["sqlite3", db_path, ".dump"]`, campos que faltan o en blanco, `db_path` que parece bandera, entorno vacío) y el panel.
- `just check-all` — lo anterior más el panel.
- **Prueba end-to-end:** copiar una base SQLite **en uso y con WAL**, restaurarla con `sqlite3 nueva.db < copia.sql` sin errores y comprobando que conserva las transacciones confirmadas antes de la copia.

## Decisiones

- **`.dump` y no `.backup`.** `.backup` (la API `sqlite3_backup`) produce una copia binaria fiel, pero exige un fichero temporal dentro del contenedor del target antes de su `cat`, lo que rompe la regla del §4 «sin ficheros temporales» y añade un paso. `.dump` sale por `stdout`, se restaura de forma trivial y es homogéneo con los volcados lógicos de MariaDB y PostgreSQL.
- **El tipo se llama `sqlite`, no `sqlite3`.** `sqlite3` es el **binario**; el valor que guarda el `CHECK` y que lee el operador en la columna es `sqlite`.
- **Reconstruir la tabla en la migración.** Es el único modo de cambiar un `CHECK` en SQLite; hacerlo con cuidado —sin pérdida de filas y con las claves foráneas bajo control— mantiene la garantía de la base intacta.
- **Prohibir credenciales/campos de host en `sqlite`.** Un fichero SQLite no tiene usuario, contraseña ni nombre de base, y una combinación incoherente solo aparecería como un fallo opaco en la primera copia.
- **`db_path` se guarda en claro.** Es una ruta **dentro** del contenedor, no un secreto; los tres secretos del §3 siguen siendo los de siempre.

## Follow-ups

- **`.backup` binario** como alternativa futura, si se prefiere una copia binaria fiel a costa del fichero temporal que hoy se descarta.
- **Pre-chequeo del binario `sqlite3`** en el alta o en el arranque, para dar un error temprano y explícito en vez de esperar a la primera copia.
- **Restauración desde el panel:** igual que en el resto de tipos, Kesto **lista** snapshots pero no los abre ni los devuelve a su sitio; restaurar un volcado SQLite es hoy manual.
