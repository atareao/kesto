# Proposal

## Why

Issue de entrada: [#11](https://github.com/atareao/kesto/issues/11). Plan: `plans/PLAN-014.md`.

Kesto da soporte a orígenes `mariadb`, `postgresql` y `directory`, y vuelca en streaming a repositorios `rustic` sobre S3. **No cubre SQLite**, que es la base de datos de buena parte del software autoalojado (Vaultwarden, Immich, Miniflux, tududi, entre otros). Hoy quien lo aloja depende de scripts sueltos o de un `cp` del fichero, a menudo **en caliente y sin consistencia garantizada**: con WAL activo, el `.db` puede ir por detrás de su `.wal`, y la copia resultante puede restaurarse con corrupción o con datos perdidos sin que nada lo delate.

## What Changes

- Nuevo tipo de origen `sqlite`, junto a `mariadb`, `postgresql` y `directory`.
- Copia **consistente en caliente** mediante la API de copia de SQLite ejecutada dentro del contenedor del target; **nunca** un `cp` del `.db`.
- Soporte de bases con el **modo WAL** activo y con la base en uso, sin detener el servicio.
- Dos campos nuevos para el target `sqlite`: `container_name` (contenedor donde vive el fichero) y `db_path` (ruta absoluta del `.db` dentro del contenedor). No usa `db_user`, `db_password` ni `db_name`.
- Migración `0003` que añade `db_path` a `backup_targets` y amplía el `CHECK` de `target_type` para admitir `sqlite`. Como SQLite no permite alterar un `CHECK` en sitio, la migración **reconstruye la tabla** preservando filas e índices.
- Panel: nueva opción `sqlite` en el formulario de fuente, con sus campos condicionales.
- Documentación: `docs/DEFINITION.md` §3/§4 y un ejemplo en el `compose.yaml` de referencia; documento de feature `docs/FEATURE-014-sqlite-copy-engine.md`.

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `db`: el `CHECK` de `backup_targets.target_type` admite `sqlite` y la tabla gana la columna `db_path`; la migración reconstruye la tabla.
- `extraction`: nuevo mecanismo de copia para el tipo `sqlite`, consistente en caliente y compatible con WAL.
- `api-resources`: la coherencia de los campos del target admite `sqlite` (exige `container_name` + `db_path`, prohíbe el resto).
- `ui`: el formulario de fuente ofrece `sqlite` y sus campos propios.

## Decisión de diseño (a confirmar antes de implementar)

La copia en caliente se propone con **`sqlite3 <db_path> .dump`** ejecutado dentro del contenedor del target, cuyo `stdout` se entrega en streaming al repositorio `rustic` por el pipeline ya existente. Ventajas: es una lectura consistente (transacción de lectura) válida con WAL y con la base en uso, **no crea ningún fichero temporal** —ni en Kesto ni en el contenedor— y se restaura de forma trivial (`sqlite3 nueva.db < copia.sql`), igual que los volcados lógicos de MariaDB y PostgreSQL.

Alternativa considerada y **no** elegida por defecto: **`.backup` (API `sqlite3_backup`)**, que produce una copia binaria fiel pero exige un fichero temporal dentro del contenedor del target antes de su `cat`, lo que rompe la regla del §4 «sin ficheros temporales» y añade un paso. Queda como opción si se prefiere fidelidad binaria a costa del temporal.

Consecuencia operativa: el contenedor del target SHALL disponer del binario `sqlite3` (lo mismo que un target MariaDB exige `mariadb-dump`). Su ausencia se reporta como el fallo del contenedor, leído y no perdido.

## Impact

- **Ficheros backend:** `src/db/entities/target.rs`, `src/api/targets.rs`, `src/engine/extraccion.rs`, `migrations/0003_sqlite_target.sql`, `src/db/migrate.rs`.
- **Ficheros frontend:** `ui/src/services/api/types.ts`, `ui/src/components/ModalDeTarget/ModalDeTarget.tsx` y sus tests.
- **Docs:** `docs/DEFINITION.md`, `compose.yaml`, `docs/FEATURE-014-sqlite-copy-engine.md`.
- Sin variable de entorno nueva. El contrato de la API se amplía: `target_type` admite `sqlite` y aparece `db_path` en el target.