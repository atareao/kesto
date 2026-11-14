# Proposal

## Why

Issues de entrada: [#52](https://github.com/atareao/kesto/issues/52) y [#53](https://github.com/atareao/kesto/issues/53). Plan: `plans/PLAN-015.md`.

Dos defectos reales encontrados al probar Kesto **contra un banco de pruebas de verdad** (RustFS + fuentes MariaDB/PostgreSQL/SQLite/directorio, copias y restauración con `restic`):

1. **Una copia fallida deja un snapshot vacío huérfano en el repositorio.** La biblioteca de archivado (`rustic_core`) silencia el error del lector y sigue, devolviendo `Ok` y **guardando la instantánea igualmente** —documentado en `src/rustic/origen.rs`—. El motor ya no hereda ese `Ok`: comprueba la entrega del volcado y marca la ejecución `failed`. Pero **no retira** la instantánea que la biblioteca ya escribió, así que queda un snapshot de 0 bytes que ninguna fila reclama: se acumula en el bucket, sale en el explorador de snapshots y cuenta como espacio. Reproducido: tras una copia `sqlite` contra una ruta inexistente, `restic snapshots` lista `sqlite-malo.sql 0 B` y la ejecución sale con `snapshot_id: null`.

2. **Los volcados restaurados salen en modo `000`.** El nodo virtual del volcado se construye con `Metadata::default()` (`src/rustic/origen.rs`), y ese modo por defecto no permite leer el fichero: un `.sql` restaurado queda `----------` hasta un `chmod`. Los ficheros de un target `directory` sí conservan sus permisos.

## What Changes

- **Motor:** cuando una ejecución acaba en `failed` **después** de que el archivado haya escrito una instantánea, el motor retira esa instantánea del repositorio (`delete_snapshots`), de mejor esfuerzo: si la retirada falla, la ejecución acaba igualmente en `failed` con su motivo original.
- **Extracción:** el nodo del volcado declara un modo de fichero utilizable (`0o644`) en vez del modo por defecto de la biblioteca.

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `engine`: nuevo requisito — una copia que no se da por buena no deja su instantánea en el repositorio.
- `extraction`: nuevo requisito — el nodo del volcado declara un modo de fichero legible al restaurar.

## Decisión de diseño (a confirmar antes de implementar)

- **Retirada de mejor esfuerzo, no bloqueante.** En las ramas de error posteriores al archivado (`copiar_el_volcado`), el motor olvida el snapshot recién escrito. Si el `forget` falla, se registra y **no** sustituye al motivo original: la ejecución sigue `failed` por lo que de verdad pasó, y el residuo se podrá limpiar con el `prune` de una ejecución posterior. La alternativa —no retirar— es la que deja basura; la alternativa —fallar por no poder retirar— convertiría un problema de limpieza en un fallo que oculta el de la copia.
- **Solo el camino del volcado.** La retirada se aplica donde el motor **sabe** que la copia no vale (la entrega del volcado no llegó entera o el productor falló). El camino `directory` no tiene señal de completitud —su límite está documentado en el encabezado del módulo— y se queda como está.
- **Modo del nodo:** `0o644` (lectura para el dueño, el grupo y el resto; escritura solo el dueño). Es un volcado, no un secreto ejecutable.

## Impact

- **Ficheros backend:** `src/engine/ejecucion.rs` (retirada de la instantánea en las ramas de error), `src/rustic/origen.rs` (modo del nodo del volcado), y, si hace falta, un método en `src/rustic/repositorio.rs` para exponer la retirada.
- **Ficheros frontend:** ninguno.
- **Docs:** `docs/FEATURE-015-higiene-de-la-copia.md`; nota en `docs/DEFINITION.md` §4 si procede.
- Sin cambios de contrato de API, de esquema, ni de variables de entorno.
