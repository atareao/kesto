# Tasks — recogida-de-temporales-de-prueba

> Change de **infraestructura de pruebas**: no toca producto, contrato HTTP, base de datos ni panel.
> Issue #94. La causa está medida (guardas retenidas en un `static` + muerte abrupta), y el patrón a
> reutilizar —etiqueta de pid + barrido por `/proc`— ya está probado en el arnés e2e.
>
> TDD: primero los tests que hoy fallan, luego el mínimo, luego el refactor. Sin dependencias nuevas
> (build offline y versiones pinadas): solo `std`.

## 0. Preparación

- [ ] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [ ] 0.2 Medir y anotar el estado de partida: `ls -d /tmp/kesto-prueba-* 2>/dev/null | wc -l` y `df -i /tmp` (verificación: queda anotado el número de restos y los inodos libres de partida; hoy hay 884 restos).
- [ ] 0.3 Crear `plans/PLAN-026.md` y referenciar el issue #94 (verificación: el plan cita #94).

## 1. Raíz única por ejecución y barrido de restos

### RED

- [ ] 1.1 Rust: un test del barrido sobre una **base inyectada** —un directorio temporal de prueba con dos entradas, `kesto-prueba-<pid-propio>` y `kesto-prueba-<pid-muerto>`— exige que borre **solo** la del pid muerto y conserve la del vivo (verificación: `cargo test --locked` en rojo hoy —la función no existe—; la función recibe la base por parámetro para no tocar el `/tmp` real).
- [ ] 1.2 Rust: un test exige que `directorio_de_prueba()` devuelva una ruta **bajo la raíz de la ejecución** (`kesto-prueba-<pid>`), no una entrada suelta (verificación: en rojo hoy —crea `/tmp/kesto-prueba-<uuid>` en el primer nivel—).
- [ ] 1.3 Rust: un test exige que el barrido **no borre** ninguna entrada ajena a `kesto-prueba-*` de la base inyectada (verificación: en rojo hoy).

### GREEN

- [ ] 1.4 `src/test_support.rs`: `proceso_vivo(pid)` por `/proc/<pid>` y `recoger_los_restos_de_prueba(base: &Path)` que borra solo `kesto-prueba-*` de pids muertos (verificación: tests 1.1 y 1.3 en verde).
- [ ] 1.5 `src/test_support.rs`: una raíz por ejecución (`kesto-prueba-<pid>`) creada perezosamente, y `directorio_de_prueba()` creando su subdirectorio bajo ella (verificación: test 1.2 en verde; las pruebas existentes siguen pasando).
- [ ] 1.6 `src/test_support.rs`: invocar el barrido **una sola vez por proceso** al primer uso del arnés (verificación: `BARRIDO` como `OnceLock`, igual que en el e2e; los restos de un pid muerto desaparecen al arrancar la batería).

### REFACTOR

- [ ] 1.7 Rustdoc: explicar por qué la limpieza no se apoya en el `Drop` (un `static` no lo ejecuta; una muerte abrupta tampoco) y qué acota el barrido. Sin cambiar comportamiento (verificación: `cargo test --locked` en verde).

## 2. Aviso de inodos y limpieza documentada

### RED

- [ ] 2.1 Rust: un test del aviso con un **umbral inyectable** exige que, por debajo del umbral, el arnés deje constancia y **no** falle; y que por encima no diga nada (verificación: en rojo hoy —no existe—).
- [ ] 2.2 Shell: un test de la receta exige que `just clean-tmp` borre los restos `kesto-prueba-*` (verificación: en rojo hoy —la receta no existe—, siguiendo el patrón de los tests de recetas que ya hay en `scripts/`).

### GREEN

- [ ] 2.3 `src/test_support.rs`: el aviso no bloqueante de inodos bajos, con umbral inyectable (verificación: test 2.1 en verde).
- [ ] 2.4 `.justfile`: receta `clean-tmp` que borra los restos, con su descripción (verificación: test 2.2 en verde).
- [ ] 2.5 `docs/COMANDOS.md`: documentar `clean-tmp` —qué borra y cuándo usarla— (verificación: la entrada existe y describe la receta).

## 3. Verificación

- [ ] 3.1 `just check` en verde (fmt + clippy + tests) (verificación: termina sin error).
- [ ] 3.2 Prueba manual del criterio del issue: matar una corrida a mitad (`timeout 20 cargo test --locked || true`), contar los restos antes y después de una corrida nueva, y comprobar que **no crecen** entre ejecuciones consecutivas; `df -i /tmp` no crece (verificación: los números anotados en 0.2 no aumentan).
- [ ] 3.3 `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios (verificación: 0 warnings).

## 4. Cierre

- [ ] 4.1 Marcar las tareas hechas y `just spec-archive recogida-de-temporales-de-prueba` (verificación: el change pasa a `openspec/changes/archive/` y la capacidad `arnes-de-pruebas` nace en `openspec/specs/`).
- [ ] 4.2 `docs/FEATURE-026-recogida-de-temporales-de-prueba.md` con qué es, ficheros, decisiones y cómo verificar (verificación: el documento existe).
- [ ] 4.3 Renombrar `plans/PLAN-026.md` a `PLAN-026-COMPLETED.md` y fusionar el PR a `development` con `Closes #94` (verificación: `git mv` hecho y PR fusionado).
