# PLAN-037 — Envolver el conteo de restauraciones en la misma transacción que la página (#145)

- **Issue:** #145 · **Change OpenSpec:** `transaccion-conteo-restauraciones-145` · **Rama:** `feature/transaccion-conteo-restauraciones-145` (desde `development`).
- **Tipo:** capa de acceso a datos (`src/db/entities`). **No** toca el contrato de la API ni el SQL.

## 1. Objetivo

Que `GET /api/v1/restorations` haga el `COUNT` y la consulta de la página dentro de la **misma transacción**, igual que ya hace `GET /api/v1/executions`. Hoy `Restauraciones::listar_filtrado` ejecuta `SQL_LISTAR_FILTRADO` y `SQL_CONTAR_FILTRADO` sueltas contra `self.pool`; entre las dos puede entrar una escritura del motor y el `total` puede contar un conjunto distinto del que acota la página.

## 2. Contexto

- La decisión 2 de `openspec/changes/archive/2026-11-19-paginacion-de-listados/design.md` fijó que el total y la página salgan de la misma instantánea de datos. Se aplicó a `executions` (`Ejecuciones::listar_filtrado`, `src/db/entities/execution.rs`) pero no a `restorations` (issue #145).
- Patrón de referencia en `execution.rs`: `self.pool.begin()` con `CONSULTA_ABRIR_TRANSACCION`, las dos consultas con `&mut *transaccion`, y `commit()` con `CONSULTA_CERRAR_TRANSACCION`; los fallos de apertura/cierre se informan con `error_de_consulta(...)`.

## 3. Decisión de arquitectura

- Envolver la página y el conteo de `Restauraciones::listar_filtrado` en un `begin()`/`commit()` común, replicando el patrón de `execution.rs`. Son dos **lecturas**: la transacción no bloquea a nadie mientras dura.
- Cerrar la transacción **antes** de mapear las filas a la entidad (las conversiones no son de base), como hace `execution.rs`.
- No se estrena un helper común: la lógica de cada entidad difiere en filtros y en el mapeo de filas; la reutilización se limita al patrón.

## 4. Alcance

- `src/db/entities/restauracion.rs`: `listar_filtrado` (transacción) + dos constantes de texto para `BEGIN`/`COMMIT` + rustdoc.
- Test que fija el comportamiento en el mismo fichero (`#[cfg(test)]`).
- Spec: requisito modificado en la capability `api-resources`. Doc: `docs/FEATURE-037-transaccion-conteo-restauraciones-145.md`.

## 5. Tareas (TDD)

### 0. Preparación
- [ ] 0.1 `just check` en verde antes de tocar nada.
- [ ] 0.2 Localizar el patrón de `execution.rs` y el `listar_filtrado` actual de `restauracion.rs`.

### 1. Transacción del listado
- **RED**
  - [ ] 1.1 Test que fija que el `COUNT` y la página comparten transacción.
- **GREEN**
  - [ ] 1.2 `listar_filtrado` abre `begin()`, ejecuta las dos consultas dentro y hace `commit()`, con constantes propias de texto para el `BEGIN` y el `COMMIT`.
- **REFACTOR**
  - [ ] 1.3 Rustdoc de `listar_filtrado` (misma transacción y por qué).
  - [ ] 1.4 `just check` en verde.

### 2. Verificación
- [ ] 2.1 `cargo test --locked --bin kesto` en verde.
- [ ] 2.2 `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios.

### 3. Cierre
- [ ] 3.1 PR a `development` con `Closes #145`.
- [ ] 3.2 `docs/FEATURE-037-transaccion-conteo-restauraciones-145.md`.
- [ ] 3.3 Renombrar `plans/PLAN-037.md` → `plans/PLAN-037-COMPLETED.md` con `git mv`.
- [ ] 3.4 `openspec archive transaccion-conteo-restauraciones-145`.

## 6. Criterios de aceptación

- El `COUNT` y la consulta de la página de `Restauraciones::listar_filtrado` van dentro de un mismo `begin()`/`commit()`.
- El contrato de la API no cambia: mismos filtros, misma ventana, misma envoltura (`restorations`, `total`), mismo orden y mismos campos.
- `executions` no se toca.
- `just check` en verde.
