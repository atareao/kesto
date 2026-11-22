# Tasks — arreglo-flake-e2e-171

> Change de **infraestructura de pruebas**: no toca producto, contrato HTTP, base de datos ni panel.
> Issue #171 (precedentes de flake dedicados: #141, #142). Causa raíz: la sonda de arranque acierta contra el **servidor temporal** del entrypoint oficial —que ya tiene la base creada y luego se apaga—, así que la siembra se encuentra el socket ausente (`ERROR 2002`, `ENOENT`).
> TDD: primero los tests que hoy fallan, luego el mínimo, luego el refactor. Sin dependencias nuevas (build offline y versiones pinadas).

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Reproducir el flake: `just e2e` en bucle hasta observar el `ERROR 2002` de siembra, o al menos confirmar por lectura que la sonda devuelve en el **primer** acierto (`tests/e2e/arnes.rs:736-760`) (verificación: la carrera queda documentada con una corrida fallida o con el tramo de código señalado).
- [x] 0.3 Confirmar que el plan de la feature cita el issue #171: `plans/PLAN-042.md` —040 y 041 están en vuelo sin fusionar, así que numerar por el mayor de `development` chocaría al fusionarse— (verificación: el plan existe y cita #171).

## 1. La espera con supervivencia entre sondeos

### RED

- [x] 1.1 Rust: test unitario del ayudante puro con el sondeo y la espera **inyectados**: un sondeo que acierta una vez y falla después —el temporal que se apaga— NO da el arranque por bueno, y el ayudante exige un segundo acierto (verificación: en rojo hoy —el ayudante no existe—; se ejecuta con `cargo test --locked --features e2e 'e2e::arnes'`).
- [x] 1.2 Rust: test del caso definitivo: un sondeo que acierta siempre da el arranque por bueno tras **dos** sondeos separados por la espera, y el número de sondeos es exactamente el mínimo (verificación: en rojo hoy).
- [x] 1.3 Rust: test de que la espera entre sondeos es la **inyectada**, no una constante interna: con una espera marcada y un sondeo que falla la primera vez, el ayudante la consume antes del segundo sondeo (verificación: en rojo hoy).
- [x] 1.4 Rust: test de que agotado el plazo el ayudante falla nombrando el **último motivo**, y no devuelve un ok silencioso (verificación: en rojo hoy).

### GREEN

- [x] 1.5 `tests/e2e/arnes.rs`: ayudante puro de espera con supervivencia que recibe el sondeo y la espera como parámetros —con el `tokio::time` solo en el borde— y devuelve `Result<(), String>` con el último motivo (verificación: tests 1.1–1.4 en verde).
- [x] 1.6 `tests/e2e/escenarios.rs`: `mariadb_arrancado` (`:150-175`) usa el ayudante en vez de `esperar_orden` (verificación: el escenario de MariaDB arranca y siembra contra el contenedor real).
- [x] 1.7 `tests/e2e/escenarios.rs`: `postgres_arrancado` (`:230-254`) usa el mismo ayudante, porque su entrypoint tiene el mismo servidor temporal con la base ya creada (verificación: el escenario de PostgreSQL arranca y siembra contra el contenedor real).
- [x] 1.8 `tests/e2e/escenarios.rs`: `alpine_con_sqlite` (`:322-345`) y la espera del S3 no cambian (verificación: el diff no los toca y sus escenarios siguen verdes).

### REFACTOR

- [x] 1.9 Rustdoc del ayudante: por qué un acierto no basta (el temporal tiene la base creada y luego se apaga: `--skip-networking` en MariaDB, `listen_addresses=''` en PostgreSQL) y por qué la espera se inyecta (verificación: `cargo doc` sin warnings; comportamiento intacto).
- [x] 1.10 `tests/e2e/escenarios.rs`: sección de pruebas del requisito nuevo junto a las de la sonda que nombra la base (`:440-500`), con el encabezado del requisito (verificación: los tests del requisito nuevo quedan agrupados bajo su encabezado).
- [x] 1.11 `just check` en verde (fmt + clippy + tests) (verificación: termina sin error).

### Cierre de la revisión (medición y cabos)

- [x] 1.12 `tests/e2e/arnes.rs`: `ESPERA_ENTRE_SONDEOS` = **2 s**, con la medición de la ventana del temporal como motivo en su doc —31 ms (MariaDB) y 148 ms (PostgreSQL), 5 corridas cada uno, ~13× el máximo— (verificación: la constante y su doc citan la medición; los unitarios del ayudante en verde).
- [x] 1.13 `tests/e2e/arnes.rs`: el plazo acota el **primer** acierto y la confirmación tiene su holgura —límite interno `plazo + espera`, no `plazo`—, con el porqué en la doc del ayudante y el contrato de error intacto (nombra el plazo visible) (verificación: el test del plazo sigue verde y el borde queda documentado).
- [x] 1.14 `tests/e2e/arnes.rs`: el test de la espera inyectada **discrimina** —espera inyectada de 20 ms, cotas `>= 20 ms` y `< 500 ms`—, de modo que una constante interna de 2 s lo delate (verificación: en verde, con la cota superior activa).
- [x] 1.15 `design.md` y `proposal.md`: no dan por especificado el «la sonda nombra la base» del #170 —que solo vive en los tests y el `CHANGELOG`— y dicen «sin esperar la espera de producción» en vez de «sin depender del reloj real» (verificación: `rg` no encuentra la cita como escenario de spec).
- [x] 1.16 `tests/e2e/escenarios.rs`: el test se llama por lo que cubre —`las_sondas_que_nombran_la_base_exigen_la_supervivencia_del_definitivo`—, porque itera las dos sondas (verificación: el nombre dice la verdad y el test sigue verde).
- [x] 1.17 `design.md`: decisión **D6** con la tabla de la medición, los hitos del entrypoint, la garantía real y el residuo acotado (verificación: D6 existe con la tabla).
- [x] 1.18 `specs/arnes-de-pruebas/spec.md`: la promesa se rebaja a la garantía real —el temporal que se apaga **antes** de la espera—, con el residuo declarado y sin renombrar escenarios (verificación: `openspec validate --strict` → valid).

## 2. Verificación del flake

- [x] 2.1 `just e2e` repetido (al menos 5 corridas) sin el `ERROR 2002` de siembra (verificación: ninguna corrida falla en la siembra de MariaDB ni de PostgreSQL).
- [x] 2.2 La suite en paralelo sigue verde: `cargo test --locked --features e2e --all-targets` (verificación: 0 fallos).
- [x] 2.3 Las pruebas existentes de las sondas siguen verdes —`la_sonda_de_mariadb_consulta_la_base_sembrable` (`escenarios.rs:486`) y `la_sonda_de_postgres_consulta_la_base_sembrable` (`:449`)— y las del #141 (`e2e::arnes`, `reintentar`) también (verificación: `cargo test --locked --features e2e` sin fallos).
- [x] 2.4 `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios (verificación: 0 warnings).
- [x] 2.5 `just e2e` una vez con `W` = 2 s (33/33, 1197 s) y repeticiones dirigidas de `mariadb`/`postgres` con `--test-threads=1` (3 + 4 + 5 corridas del escenario de PostgreSQL; todas verdes salvo **una** vez en `copia_y_restauracion_de_postgresql_conserva_las_filas`, no reproducida en 10 repeticiones posteriores) (verificación: la siembra de MariaDB/PostgreSQL no vuelve a dar `ERROR 2002`).
- [x] 2.6 `openspec validate arreglo-flake-e2e-171 --strict` → valid (verificación: valid).
- [x] 2.7 Cabo abierto: un fallo único y **no reproducido** en `copia_y_restauracion_de_postgresql_conserva_las_filas`. Se intentó reproducir a conciencia (12/12 corridas dirigidas verdes con salida completa) sin reaparecer: acumulado **23 corridas, 1 fallo**. Es un escenario de **restauración** cuyo cuerpo **no** toca #171 (en esta rama corre con el código de restauración anterior a #164), así que **no se atribuye** a #171: queda como **residuo documentado y vigilado en el E2E del CI** (si reaparece, capturar el motivo completo) (verificación: hecha).

## 3. Cierre

- [x] 3.1 Marcar las tareas hechas y abrir el PR a `development` con `Closes #171` (verificación: PR creado).
- [x] 3.2 `docs/FEATURE-042-<slug>.md` con qué es, ficheros, decisiones —la opción elegida y las descartadas— y cómo verificar (verificación: el documento existe).
- [x] 3.3 Renombrar `plans/PLAN-042.md` a `plans/PLAN-042-COMPLETED.md` con `git mv` (verificación: renombrado).
- [x] 3.4 Archivar el change: `openspec archive arreglo-flake-e2e-171` (verificación: el change pasa a `openspec/changes/archive/` y el requisito nuevo entra **al final** de `openspec/specs/arnes-de-pruebas/spec.md`, sin dejar huérfanos los escenarios de los requisitos anteriores).
