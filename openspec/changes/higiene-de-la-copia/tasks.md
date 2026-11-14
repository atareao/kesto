# Tasks

## SDD

- [ ] Aprobar este change proposal (`higiene-de-la-copia`).
- [ ] `openspec validate higiene-de-la-copia --strict` en verde.

## Preparación

- [ ] Crear rama `feature/higiene-de-la-copia` desde `development`.

## TDD — el motor no deja la instantánea de una copia fallida (issue #52)

- [ ] RED: test de `src/engine/ejecucion.rs` — una copia cuyo volcado no se entrega entero acaba en `failed` **y** el repositorio NO SHALL quedarse con la instantánea que el archivado escribió. Debe fallar (hoy queda).
- [ ] RED: test — si la retirada de la instantánea falla, la ejecución acaba igualmente en `failed` con el motivo **original** del volcado, no con el de la retirada.
- [ ] GREEN: retirada de la instantánea (`forget`) en las ramas de error posteriores al archivado de `copiar_el_volcado`, de mejor esfuerzo y sin sustituir el motivo.
- [ ] REFACTOR: clippy y fmt.

## TDD — el nodo del volcado declara un modo legible (issue #53)

- [ ] RED: test de `src/rustic/origen.rs` — el nodo del volcado declara modo `0o644`, no el por defecto de la biblioteca. Debe fallar.
- [ ] GREEN: fijar el modo en el `Metadata` del nodo.
- [ ] REFACTOR: clippy y fmt.

## Documentación

- [ ] `docs/DEFINITION.md` §4: nota de que una copia que no se da por buena no deja instantánea, y del modo del volcado restaurado (si procede).
- [ ] `docs/FEATURE-015-higiene-de-la-copia.md`.

## Verificación

- [ ] `just check` verde.
- [ ] `just check-all` verde.
- [ ] Puerta del protocolo (`just check-spec`) en verde con este change activo.

## Verificación en el banco de pruebas (manual)

- [ ] Repetir el escenario de la issue #52 (target `sqlite` con `db_path` inexistente): la ejecución acaba `failed` y el repositorio queda **sin** snapshot nuevo.
- [ ] Restaurar un volcado y comprobar que el `.sql` sale legible (sin `chmod`).

## Cierre

- [ ] PR a `development`, CI en verde; cierra #52 y #53.
- [ ] Renombrar `plans/PLAN-015.md` a `plans/PLAN-015-COMPLETED.md` y archivar el change.
