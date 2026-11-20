# Tasks — transaccion-conteo-restauraciones-145

> Change de la **capa de acceso a datos**: envuelve el `COUNT` y la página del listado de
> restauraciones en una misma transacción, como ya hace el de ejecuciones (decisión 2 de
> `2026-11-19-paginacion-de-listados`). Issue #145. No toca el contrato de la API ni el SQL.
> TDD: primero el test que hoy falla, luego el mínimo, luego el refactor. Sin dependencias nuevas.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Localizar el patrón de referencia: `Ejecuciones::listar_filtrado` (`src/db/entities/execution.rs`, `begin()`/`commit()` con `CONSULTA_ABRIR_TRANSACCION`/`CONSULTA_CERRAR_TRANSACCION`) y el actual `Restauraciones::listar_filtrado` (`src/db/entities/restauracion.rs`, dos consultas sueltas contra `self.pool`) (verificación: se citan líneas concretas).

## 1. Transacción del listado de restauraciones

### RED

- [x] 1.1 Test que fija que el `COUNT` y la consulta de página de `Restauraciones::listar_filtrado` comparten transacción (verificación: en rojo hoy —las dos van sueltas contra `self.pool`—; se ejecuta con `cargo test --locked --bin kesto restauracion`).

### GREEN

- [x] 1.2 `src/db/entities/restauracion.rs`: `listar_filtrado` abre `self.pool.begin()`, ejecuta `SQL_LISTAR_FILTRADO` y `SQL_CONTAR_FILTRADO` dentro (`&mut *transaccion`) y hace `commit()`, con dos constantes de texto para el `BEGIN` y el `COMMIT` como en `execution.rs` (verificación: 1.1 en verde).

### REFACTOR

- [x] 1.3 Rustdoc de `listar_filtrado`: decir que las dos consultas van, además de con los mismos filtros, en la **misma transacción**, y por qué (verificación: el rustdoc cita la transacción y el motivo; `cargo doc` sin warnings).
- [x] 1.4 `just check` en verde (fmt + clippy `--workspace` + tests) (verificación: termina sin error).

## 2. Verificación

- [x] 2.1 `cargo test --locked --bin kesto` en verde (mismo comando que usa el CI para el binario; verificación: 0 fallos).
- [x] 2.2 `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios (verificación: 0 warnings).

## 3. Cierre

- [x] 3.1 Marcar las tareas hechas y abrir el PR a `development` con `Closes #145` (verificación: PR creado).
- [x] 3.2 `docs/FEATURE-037-transaccion-conteo-restauraciones-145.md` con qué es, ficheros, decisiones y cómo verificar (verificación: el documento existe).
- [x] 3.3 Renombrar `plans/PLAN-037.md` a `plans/PLAN-037-COMPLETED.md` con `git mv` (verificación: renombrado).
- [x] 3.4 Archivar el change: `openspec archive transaccion-conteo-restauraciones-145` (verificación: pasa a `openspec/changes/archive/` y el requisito modificado entra en `openspec/specs/api-resources/`).
