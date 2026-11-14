# Tasks

## SDD

- [x] Aprobar este change proposal (`higiene-de-la-copia`).
- [x] `openspec validate higiene-de-la-copia --strict` en verde.

## Preparación

- [x] Crear rama `feature/higiene-de-la-copia` desde `development`.

## TDD — el motor no deja la instantánea de una copia fallida (issue #52)

- [x] RED: test de `src/engine/ejecucion.rs` — una copia cuyo volcado no se entrega entero acaba en `failed` **y** el repositorio NO SHALL quedarse con la instantánea que el archivado escribió. Debe fallar (hoy queda).
- [x] RED: test — si la retirada de la instantánea falla, la ejecución acaba igualmente en `failed` con el motivo **original** del volcado, no con el de la retirada.
- [x] GREEN: retirada de la instantánea (`forget`) en las ramas de error posteriores al archivado de `copiar_el_volcado`, de mejor esfuerzo y sin sustituir el motivo.
- [x] REFACTOR: clippy y fmt.

## TDD — el nodo del volcado declara un modo legible (issue #53)

- [x] RED: test de `src/rustic/origen.rs` — el nodo del volcado declara modo `0o644`, no el por defecto de la biblioteca. Debe fallar.
- [x] GREEN: fijar el modo en el `Metadata` del nodo.
- [x] REFACTOR: clippy y fmt.

## Documentación

- [x] `docs/DEFINITION.md` §4: nota de que una copia que no se da por buena no deja instantánea, y del modo del volcado restaurado (si procede).
- [x] `docs/FEATURE-015-higiene-de-la-copia.md`.

## Verificación

- [x] `just check` verde.
- [x] `just check-all` verde.
- [x] Puerta del protocolo (`just check-spec`) en verde con este change activo.

## Verificación en el banco de pruebas (manual)

- [x] Repetir el escenario de la issue #52 (target `sqlite` con `db_path` inexistente): la ejecución acaba `failed` y el repositorio queda **sin** snapshot nuevo.
- [x] Restaurar un volcado y comprobar que el `.sql` sale legible (sin `chmod`).

## Cierre

- [x] PR a `development`, CI en verde; cierra #52 y #53.
- [x] Renombrar `plans/PLAN-015.md` a `plans/PLAN-015-COMPLETED.md` y archivar el change.
