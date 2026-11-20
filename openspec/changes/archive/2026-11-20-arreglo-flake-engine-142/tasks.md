# Tasks — arreglo-flake-engine-142

> Change de **infraestructura de pruebas**: no toca producto, contrato HTTP, base de datos ni panel.
> Issue #142. Diagnóstico cerrado (ver `HANDOFF.md` y `plans/PLAN-035.md`): causa raíz = el test se
> sincroniza con la **fila** y luego lee el **stream** con `try_recv`; entre `ejecucion.rs:201` y
> `:206` hay una ventana en la que el último evento es un `running`. Producto intacto (§5).
> TDD: primero los tests que hoy fallan, luego el mínimo, luego el refactor. Sin dependencias nuevas.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 (no se reprodujo el fallo: el diagnóstico era de alta confianza; la ausencia se verificó tras el arreglo) Reproducir el flake bajo carga: 200 pasadas aisladas + `taskset -c 0,1 cargo test --locked --bin kesto 'engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total' -- --exact` (verificación: se observa al menos un fallo `bytes_processed=0` como en el diagnóstico).

## 1. Espera en el stream hasta el evento terminal

### RED

- [x] 1.1 Rust: test unitario del ayudante con un canal `broadcast` controlado —se emiten eventos `running`, luego, tras una espera corta, el terminal— exige que el ayudante **no** devuelva hasta ver el terminal, y que devuelva la lista terminando en él (verificación: en rojo hoy —el ayudante no existe—; se ejecuta con `cargo test --locked --bin kesto 'engine::tests::'`).
- [x] 1.2 Rust: test del ayudante con plazo agotado —solo se emiten `running`— exige un fallo en voz alta nombrando el estado terminal que no llegó, en vez de devolver una lista sin evento terminal (verificación: en rojo hoy).

### GREEN

- [x] 1.3 `src/engine/mod.rs`: ayudante `recoger_hasta_el_final(receptor, plazo) -> Vec<EventoDeEjecucion>` que acumula eventos (saltando `Restauracion` y `Lagged` como hace `recoger`) y devuelve la lista solo al observar `status ∈ {Success, Failed}`, con plazo acotado (verificación: tests 1.1–1.2 en verde).
- [x] 1.4 `src/engine/mod.rs`: las dos llamadas `let eventos = recoger(&mut receptor);` que aseveran sobre el evento final (`:3233` y `:3293`) pasan a usar el ayudante con `PLAZO_DE_ESPERA`; `recoger` se conserva para las pruebas que no miden el evento final (verificación: la prueba del issue sigue verde y su `.last()` es el terminal).

### REFACTOR

- [x] 1.5 Rustdoc del ayudante explicando por qué se espera en el stream y no en la fila —fila y stream son dos caminos con orden no garantizado entre sí— y que el plazo convierte un fallo en un mensaje, no en un cuelgue (verificación: `cargo doc` sin warnings; comportamiento intacto).
- [ ] 1.6 `just check` en verde (fmt + clippy + tests) (verificación: termina sin error).

## 2. Verificación del flake

- [x] 2.1 200 pasadas aisladas de la prueba del issue → 0 fallos (verificación: `cargo test --locked --bin kesto 'engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total' -- --exact` en bucle).
- [x] 2.2 Suite completa bajo carga → 0 fallos (verificación: `taskset -c 0,1 cargo test --locked`).
- [ ] 2.3 `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios (verificación: 0 warnings).

## 3. Cierre

- [ ] 3.1 Marcar las tareas hechas y abrir el PR a `development` con `Closes #142` (verificación: PR creado).
- [ ] 3.2 `docs/FEATURE-035-arreglo-flake-engine-142.md` con qué es, ficheros implicados, decisiones y cómo verificar (verificación: el documento existe).
- [ ] 3.3 Renombrar `plans/PLAN-035.md` a `plans/PLAN-035-COMPLETED.md` con `git mv` (verificación: renombrado).
- [ ] 3.4 Archivar el change: `openspec archive arreglo-flake-engine-142` (verificación: el change pasa a `openspec/changes/archive/` y el requisito nuevo entra en `openspec/specs/engine/`).
