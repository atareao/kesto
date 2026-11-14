# PLAN-015 — Higiene de la copia (FEATURE-015)

- **Issues:** [#52](https://github.com/atareao/kesto/issues/52) y [#53](https://github.com/atareao/kesto/issues/53)
- **Change SDD:** `openspec/changes/higiene-de-la-copia/`
- **Estado:** completada (PR #54)

## Objetivo

Cerrar dos defectos encontrados al probar Kesto contra un banco de pruebas real (RustFS + fuentes MariaDB/PostgreSQL/SQLite/directorio, copias y restauración con `restic`):

1. **Sin residuos (#52):** una copia que no se da por buena **no deja** su instantánea en el repositorio.
2. **Volcados legibles (#53):** el nodo del volcado declara modo `0o644`, de modo que el `.sql` restaurado se puede leer sin `chmod`.

## Alcance

1. **Motor (`src/engine/ejecucion.rs`):** en las ramas de error posteriores al archivado de `copiar_el_volcado`, retirar del repositorio la instantánea ya escrita (`delete_snapshots`), de mejor esfuerzo y sin sustituir el motivo original.
2. **Extracción (`src/rustic/origen.rs`):** fijar `0o644` en el `Metadata` del nodo del volcado.
3. **Docs:** `docs/FEATURE-015-higiene-de-la-copia.md` y, si procede, nota en `docs/DEFINITION.md` §4.

## Decisión

- Retirada de **mejor esfuerzo**: si falla, la ejecución acaba igualmente en `failed` con el motivo del volcado (no con el de la limpieza) y el worker sigue.
- Solo el camino del volcado (donde el motor sabe que la copia no vale); el camino `directory` no tiene señal de completitud y se queda como está.
- Modo `0o644`.

## Fases (SDD + TDD)

1. Aprobar el change proposal y `openspec validate higiene-de-la-copia --strict`.
2. Rama `feature/higiene-de-la-copia` desde `development`.
3. RED → GREEN → REFACTOR por requisito, con `just check` en verde entre pasos.
4. Documentación y code review (`rust-reviewer`).
5. `just check-all` y verificación manual en el banco de pruebas.

## Verificación

- `just check` y `just check-all` verdes.
- En el banco: repetir #52 → la ejecución falla y el repositorio **no** gana instantáneas; restaurar un volcado → el `.sql` sale legible.
