# FEATURE-037 — Conteo y página de restauraciones en la misma transacción (#145)

- **Issue:** #145 · **Change OpenSpec:** `transaccion-conteo-restauraciones-145` · **Plan:** `plans/PLAN-037.md`.
- **Tipo:** capa de acceso a datos (`src/db/entities`). No toca el contrato de la API ni el SQL.

## Qué es

`Restauraciones::listar_filtrado` —la capa que sirve `GET /api/v1/restorations`— resuelve ahora la **página** (`SQL_LISTAR_FILTRADO`) y el **total** (`SQL_CONTAR_FILTRADO`) dentro de una **misma transacción**, exactamente como ya hacía `Ejecuciones::listar_filtrado` para `GET /api/v1/executions`.

## Causa / motivo

La decisión 2 del change archivado `2026-11-19-paginacion-de-listados` (issue #140) fijó que, en un listado paginado, el total y la página salgan de la misma instantánea de datos. Se aplicó a ejecuciones pero no a restauraciones, cuyas dos consultas iban sueltas contra `self.pool`: entre una y otra podía entrar una escritura del motor y el `total` diría un número que la página no sostiene. Era una asimetría con la misma ventana de inconsistencia que la decisión quiso cerrar.

## Ficheros implicados

- `src/db/entities/restauracion.rs`: `listar_filtrado` (transacción), dos constantes de texto (`CONSULTA_ABRIR_TRANSACCION`/`CONSULTA_CERRAR_TRANSACCION`), rustdoc y un test nuevo.
- `src/db/mod.rs`: `opciones_del_dsn` pasa a `pub(crate)` (lo usa el test para construir su pool instrumentado).
- `openspec/changes/transaccion-conteo-restauraciones-145/` · `docs/FEATURE-037-transaccion-conteo-restauraciones-145.md` · `plans/PLAN-037.md`.

## Cómo funciona

- `listar_filtrado` abre `self.pool.begin()` (con `CONSULTA_ABRIR_TRANSACCION` como texto del fallo), ejecuta las dos consultas con `&mut *transaccion` y hace `commit()` (con `CONSULTA_CERRAR_TRANSACCION`). El mapeo de filas a la entidad ocurre **después** del `commit` (no es trabajo de base).
- Son dos **lecturas**: la transacción no bloquea a nadie mientras dura.
- El contrato publicado de `GET /api/v1/restorations` **no cambia**: mismos filtros (`status`, `target_id`), misma ventana (`limit`/`offset`), misma envoltura (`restorations`, `total`), mismo orden y mismos campos por fila.

## Por qué un test que cuenta adquisiciones

La coherencia total/página solo se rompe si una escritura entra **entre** las dos consultas, algo que no se puede forzar de forma determinista desde fuera del método. La prueba mide su **causa**: cuántas veces el listado pide una conexión al pool, con un pool de **una sola conexión** que incrementa un contador en `before_acquire`.
- Sin transacción: **2** adquisiciones (una por consulta) → el RED que se observó (`left: 2, right: 1`).
- Con la transacción: **1** adquisición (la conexión se retiene hasta el `commit`) → el GREEN.
Determinista, sin concurrencia ni temporizadores.

## Cómo verificar

```sh
cargo test --locked --bin kesto el_listado_resuelve_la_pagina_y_el_conteo_en_una_sola_transaccion
just check    # fmt + clippy --workspace -D warnings + test --workspace
```

## Decisiones

- **Inline, no un helper nuevo**: se replica el patrón `begin()`/`commit()` de `execution.rs` en vez de estrenar una abstracción común; la lógica de cada entidad difiere en filtros y mapeo. `executions` no se toca.
- **`desde_fila_listada` tras el `commit`**: el mapeo no toca la base, así que no tiene por qué ir dentro de la transacción (igual que en `execution.rs`).
- **`opciones_del_dsn` → `pub(crate)`**: cambio mínimo de visibilidad para que el test construya un pool con `before_acquire` reutilizando las **mismas** opciones (WAL, `foreign_keys`, `busy_timeout`) en vez de duplicarlas.

## Follow-ups

- Ninguno de esta feature. `Ejecuciones::listar_filtrado` ya tenía la transacción desde #140.
