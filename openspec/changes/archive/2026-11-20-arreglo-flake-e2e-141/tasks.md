# Tasks — arreglo-flake-e2e-141

> Change de **infraestructura de pruebas**: no toca producto, contrato HTTP, base de datos ni panel.
> Issue #141. Diagnóstico ya cerrado (ver `HANDOFF.md` y `plans/PLAN-034.md`): causa raíz = `TOPE_DEL_DESTINO` (10 s) agotado por el `init` de rustic bajo carga; un `502` no deja fila parcial → reintentar es seguro.
> TDD: primero los tests que hoy fallan, luego el mínimo, luego el refactor. Sin dependencias nuevas (build offline y versiones pinadas).

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Reproducir el flake bajo carga: `cargo test --locked --features e2e --all-targets` en paralelo (verificación: se observan fallos `502` en el alta, como en el diagnóstico).
- [x] 0.3 Confirmar que `plans/PLAN-034.md` cita el issue #141 (verificación: el plan existe y cita #141).

## 1. Reintento acotado del alta

### RED

- [x] 1.1 Rust: test unitario del bucle de reintento puro —un «intento» simulado que responde `502` una vez y `200` después— exige exactamente 2 llamadas y que devuelva el `Value` del `200` (verificación: en rojo hoy —el bucle no existe—; se ejecuta con `cargo test --locked --features e2e 'e2e::arnes'`).
- [x] 1.2 Rust: test del bucle con un «intento» que responde `502` **siempre** exige exactamente N intentos y que devuelva el último `502` —para que el llamador falle en voz alta en vez de devolver vacío— (verificación: en rojo hoy).
- [x] 1.3 Rust: test de que el backoff respeta la espera inyectada: con una espera diminuta y 3 intentos, la duración total es al menos 2× la espera (verificación: en rojo hoy).

### GREEN

- [x] 1.4 `tests/e2e/arnes.rs`: constante `INTENTOS_DEL_ALTA` (2–3) y `ESPERA_ENTRE_ALTA`, y un helper puro `reintentar(intentos, espera, intento)` donde `intento` es una clausura `async` que devuelve `(StatusCode, Value)` (verificación: tests 1.1–1.3 en verde).
- [x] 1.5 `tests/e2e/arnes.rs`: `crear_repositorio` usa el helper —`2xx` → devuelve `data`; `502` agotado → falla en voz alta nombrando el código y el cuerpo—; el resto del arnés no cambia (verificación: el alta sigue funcionando contra el S3 real).

### REFACTOR

- [x] 1.6 Rustdoc del helper y de por qué reintentar es seguro (un `502` = cero filas escritas; la fila va después de abrir el destino) (verificación: `cargo doc` sin warnings; comportamiento intacto).
- [x] 1.7 `just check` en verde (fmt + clippy + tests) (verificación: termina sin error).

## 2. Verificación del flake

- [x] 2.1 `cargo test --locked --features e2e --all-targets` en paralelo → 0 fallos (verificación: ninguna alta cae con `502`).
- [x] 2.2 `just e2e` (serie, suite oficial `--test-threads=1`) sigue verde (verificación: termina sin error).
- [x] 2.3 `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios (verificación: 0 warnings).

## 3. Cierre

- [ ] 3.1 Marcar las tareas hechas y abrir el PR a `development` con `Closes #141` (verificación: PR creado).
- [x] 3.2 `docs/FEATURE-034-arreglo-flake-e2e-141.md` con qué es, ficheros, decisiones y cómo verificar (verificación: el documento existe).
- [x] 3.3 Renombrar `plans/PLAN-034.md` a `plans/PLAN-034-COMPLETED.md` con `git mv` (verificación: renombrado).
- [x] 3.4 Archivar el change: `openspec archive arreglo-flake-e2e-141` (verificación: el change pasa a `openspec/changes/archive/` y el requisito nuevo entra en `openspec/specs/arnes-de-pruebas/`).
