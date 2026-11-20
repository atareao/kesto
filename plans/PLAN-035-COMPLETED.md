# PLAN-035 — Arreglo del flake `engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total` (#142)

- **Issue:** #142 · **Change OpenSpec:** `arreglo-flake-engine-142` · **Rama:** `feature/arreglo-flake-engine-142` (desde `development`).
- **Tipo:** infraestructura de pruebas — **no toca producto, contrato HTTP, base de datos ni panel**.

## 1. Objetivo

Eliminar el flake del test `engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total`, que falla una vez cada muchas corridas (más probable en la suite completa bajo carga) con `bytes_processed=0` cuando se esperaban `500`. Pasa 5/5 en aislamiento.

## 2. Diagnóstico (ya cerrado, no repetir la investigación)

- El trabajador escribe la fila terminal en `src/engine/ejecucion.rs:201` y **después** publica el evento final en `:206`. Ese orden es deliberado y lo fija el §5 («la fila queda constancia antes del aviso»).
- La prueba se sincroniza con la **fila** (`esperar_el_final`, `src/engine/mod.rs:2380-2411`, sondeo cada 10 ms) y acto seguido vacía el receptor con `recoger` (`:3116-3133`, un `try_recv` que no espera).
- Entre la escritura de la fila y la publicación del evento hay una ventana: si el test lee en esa ventana, `eventos.last()` es el último evento `running` (con `percentage=None` y `bytes_processed=0`), no el evento final. El porcentaje se calcula bien (`subidos=500`, `total=2000` → `25.0`; `src/engine/ejecucion.rs:1222-1227`).
- Causa raíz: la fila y el stream son **dos caminos con orden no garantizado entre sí para el observador**. Confianza ~85 %.
- Se agrava bajo carga porque los `#[tokio::test]` multi-hilo corren en paralelo y sube la preempción entre `:201` y `:206`.

## 3. Decisión de arquitectura

Arreglo **en el lado de la prueba**, sin tocar producción: un ayudante que espere en el **stream** hasta observar `status ∈ {Success, Failed}` (con plazo acotado) y solo entonces recolecte; así `.last()` es siempre el evento terminal. Reordenar el producto (publicar antes de la fila) queda **descartado**: cambia la semántica del §5.

## 4. Alcance

- `src/engine/mod.rs` (módulo `#[cfg(test)]`): nuevo ayudante `recoger_hasta_el_final(receptor, plazo)` y sustitución de las dos llamadas a `recoger` que aseveran sobre el evento final (`:3233` y `:3293`). `recoger` se conserva para el resto de pruebas.
- Requisito de especificación añadido en `openspec/changes/arreglo-flake-engine-142/specs/engine/spec.md`: la fila terminal se escribe antes de publicar el evento final, ambos coinciden en `status` y cifras, y quien mida el evento final espera en el stream, no en la fila.
- Documentación: `docs/FEATURE-035-arreglo-flake-engine-142.md`.

## 5. Tareas (TDD)

### 0. Preparación
- [ ] 0.1 `just check` en verde antes de tocar nada.
- [ ] 0.2 Reproducir el flake: 200 pasadas aisladas + `taskset -c 0,1 cargo test --locked --bin kesto 'engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total' -- --exact`.

### 1. Espera en el stream hasta el evento terminal
- **RED**
  - [ ] 1.1 Test unitario del ayudante con un `broadcast` controlado: emite `running`, tras una espera corta emite el terminal; exige que el ayudante no devuelva hasta ver el terminal y que la lista acabe en él.
  - [ ] 1.2 Test del ayudante con plazo agotado (solo `running`): exige fallo en voz alta nombrando el estado terminal que no llegó.
- **GREEN**
  - [ ] 1.3 `src/engine/mod.rs`: `recoger_hasta_el_final(receptor, plazo) -> Vec<EventoDeEjecucion>` que acumula eventos (saltando `Restauracion` y `Lagged` como `recoger`) y devuelve la lista solo al ver `Success`/`Failed`, con plazo acotado.
  - [ ] 1.4 Sustituir `let eventos = recoger(&mut receptor);` en `:3233` y `:3293` por el ayudante con `PLAZO_DE_ESPERA`.
- **REFACTOR**
  - [ ] 1.5 Rustdoc del ayudante: por qué se espera en el stream y no en la fila; el plazo convierte un fallo en un mensaje y no en un cuelgue.
  - [ ] 1.6 `just check` en verde.

### 2. Verificación del flake
- [ ] 2.1 200 pasadas aisladas de la prueba del issue → 0 fallos.
- [ ] 2.2 Suite completa bajo carga (`taskset -c 0,1 cargo test --locked`) → 0 fallos.
- [ ] 2.3 `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios.

### 3. Cierre
- [ ] 3.1 Marcar tareas y abrir el PR a `development` con `Closes #142`.
- [ ] 3.2 `docs/FEATURE-035-arreglo-flake-engine-142.md` (qué es, ficheros, decisiones, cómo verificar).
- [ ] 3.3 Renombrar `plans/PLAN-035.md` → `plans/PLAN-035-COMPLETED.md` con `git mv`.
- [ ] 3.4 `openspec archive arreglo-flake-engine-142`.

## 6. Criterios de aceptación

- 0 fallos en 200 pasadas aisladas y en la suite bajo carga.
- `just check` (`fmt` + `clippy -- -D warnings` + `test`) en verde.
- Cobertura preservada: la prueba del issue sigue aseverando sobre el evento final y sobre la fila.
- Producto intacto: sin cambios en `src/engine/ejecucion.rs`, contrato HTTP, esquema ni panel.
