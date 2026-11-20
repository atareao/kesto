# Proposal

## Why

La decisión 2 del change archivado `2026-11-19-paginacion-de-listados` fijó que, en un listado paginado, el `total` y la página salgan de la **misma instantánea de datos**: si el `COUNT` y la consulta de la página van sueltos, entre una y otra puede entrar una escritura del motor y el `total` diría un número que la página no sostiene. Se aplicó a `GET /api/v1/executions` —`Ejecuciones::listar_filtrado` abre un `begin()` común para `SQL_LISTAR_FILTRADO` y `SQL_CONTAR_FILTRADO`— pero **no** a `GET /api/v1/restorations`, cuyas dos consultas (`Restauraciones::listar_filtrado`) siguen yendo sueltas contra el pool. Es una asimetría con la misma ventana de inconsistencia que la decisión quiso cerrar (issue #145).

## What Changes

- `src/db/entities/restauracion.rs`: `Restauraciones::listar_filtrado` pasa a abrir una **transacción** (`pool.begin()`) y ejecutar dentro de ella la página (`SQL_LISTAR_FILTRADO`) y el conteo (`SQL_CONTAR_FILTRADO`), con las dos consultas informadas por sus propias constantes, y a **confirmarla** al final —exactamente el patrón de `execution.rs`—. Son dos lecturas, así que la transacción no bloquea a nadie mientras dura.
- El rustdoc del método deja de decir que las dos consultas «van con los mismos filtros» a secas y pasa a decir que van, además, en la **misma transacción** y por qué.
- Prueba que fija el comportamiento (en la misma línea que las de `execution.rs`): el listado sigue devolviendo página y total coherentes; se añade el caso que documenta el contrato de la transacción.

## Non-goals

- NO se toca el contrato publicado de `GET /api/v1/restorations`: mismos filtros (`status`, `target_id`), misma ventana (`limit`/`offset`), misma envoltura (`restorations`, `total`), mismo orden y mismos campos por fila.
- NO se cambian las consultas SQL ni el esquema.
- NO se toca `executions`: ya está hecho.
- NO se unifica el helper de conteo en una abstracción nueva: la reutilización, si la hay, se limita a compartir el patrón `begin()`/`commit()`; no se estrena un tipo común solo para esto (la lógica de cada entidad difiere en filtros y en el mapeo de filas).

## Capabilities

### New Capabilities

_(ninguna)_

### Modified Capabilities

- `api-resources`: el requisito «El histórico de restauraciones se lista con filtros y paginación» explicita que el `total` y la página del listado de restauraciones salen de la **misma transacción**, igual que el de ejecuciones.
