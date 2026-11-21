# PLAN-042 — Arreglo del flake E2E de la sonda de arranque

- **Issue:** #171 — «Flake E2E: la sonda de MariaDB acierta contra el servidor temporal de arranque».
- **Change de OpenSpec:** `arreglo-flake-e2e-171`.
- **Rama:** `feature/arreglo-sonda-mariadb`, en el worktree `/tmp/opencode/kesto-171`, ramificada de `development`.
- **Alcance:** **solo el arnés de pruebas** (`tests/e2e/`). Cero cambios en `src/`, contrato HTTP, base de datos y panel.
- **Bump esperado:** `patch`. Un merge a `main` es una release.
- **Precedentes:** arreglos de flake dedicados #141 (`arreglo-flake-e2e-141`) y #142 (`arreglo-flake-engine-142`).

## 1. Causa raíz

Las imágenes oficiales de MariaDB y de PostgreSQL levantan un **servidor temporal** para inicializarse —que **ya tiene la base creada**, porque la crea él— y lo apagan antes de arrancar el definitivo.

El arnés ya lo sabía y por eso sus sondas **nombran la base** (`sonda_de_mariadb`, `tests/e2e/arnes.rs:212-230`; `sonda_de_postgres`, `:197-210`), lo que las protege del temporal *antes* de que la base exista. Pero queda un hueco: la base existe en el temporal durante su tramo final, así que la sonda puede acertar contra él.

Y la espera se conforma con el **primer** acierto: `Motor::esperar_orden` (`tests/e2e/arnes.rs:736-760`) devuelve `Ok(())` en cuanto una invocación sale con código 0. Cuando el temporal se apaga, la siembra —que corre justo después— se encuentra el socket ausente:

```
ERROR 2002 (HY000): Can't connect to local server through socket '/run/mysqld/mysqld.sock' (2)
```

El código `2` es `ENOENT`. No es una regresión de #170: la carrera es previa y está en el arnés.

## 2. Decisión

**Opción A: la espera exige que el servidor que contesta sobreviva a una espera corta entre dos sondeos.** El temporal —que se apaga— no la da por buena; el definitivo sí.

| Opción | Veredicto |
|---|---|
| **A** — dos sondeos con supervivencia entre ellos | **Elegida.** No asume nada de la imagen; si el servidor tarda de más, solo se retrasa el acierto. |
| B — señalar una exclusiva del definitivo (su escucha TCP: el temporal de MariaDB va con `--skip-networking`, el de PostgreSQL con `listen_addresses=''`) | Descartada. Ataría la sonda al `bind-address`, a la autenticación (`root@localhost` vs `root@%`, `pg_hba`) y al `listen_addresses` de cada imagen. Si esa suposición se rompe, la espera no acierta **nunca** y el fallo pasa de flake ocasional a plazo agotado en cada corrida. |
| C — reintentar la siembra ante un fallo de conexión | Descartada. Trata el síntoma donde no está —lo que miente es la sonda— y tendría que distinguir un fallo de conexión de un error real para no duplicar filas. |

**El ayudante es puro y testeable:** recibe el **sondeo** y la **espera** inyectados —el `tokio::time` solo en el borde—, igual que el `reintentar` del #141 (`tests/e2e/arnes.rs:1408-1428`) recibe el «intento» como clausura. Así una prueba simula la secuencia temporal → definitivo sin contenedores y sin esperar la espera de producción.

**`esperar_orden` no se cambia para todos:** SQLite (contenedor `alpine`, sin servidor temporal) no debe alterarse; el cambio va por el ayudante nuevo o por un parámetro, nunca por la función compartida a lo bruto.

## 3. Alcance

| Entra | Sale (con motivo) |
|---|---|
| MariaDB (`escenarios.rs:150-175`) y PostgreSQL (`escenarios.rs:230-254`): mismo mecanismo, verificado en el entrypoint oficial (`docker_temp_server_start` con `listen_addresses=''` → `docker_setup_db` → `docker_temp_server_stop`), y la sonda de PostgreSQL nombra la base igual que la de MariaDB. | SQLite (`escenarios.rs:322-345`): contenedor `alpine` con el cliente, **sin servidor**. La espera del S3 (`esperar_el_s3`, `arnes.rs:870`): sin servidor temporal. |
| El ayudante puro de espera con supervivencia, en `tests/e2e/arnes.rs`. | El producto, el contrato HTTP, la base de datos y el panel: **cero cambios en `src/`**. |
| Las constantes del #141 (`INTENTOS_DEL_ALTA`, `ESPERA_ENTRE_ALTA`, `reintentar`): **no se tocan**. | `PLAZO_DE_ARRANQUE` (`arnes.rs:147`): no se sube. Ninguna aserción se relaja y ningún escenario se retira. |

Sin dependencias nuevas (build offline y versiones pinadas).

## 4. Tareas

### 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Confirmar por lectura que la sonda devuelve en el **primer** acierto (`tests/e2e/arnes.rs:736-760`) (verificación: la carrera queda documentada con el tramo de código señalado).
- [x] 0.3 Confirmar que este plan cita el issue #171 (verificación: existe y lo cita).

### 1. La espera con supervivencia entre sondeos

**RED**

- [x] 1.1 Test unitario del ayudante con el sondeo y la espera **inyectados**: un sondeo que acierta una vez y falla después —el temporal que se apaga— NO da el arranque por bueno (verificación: en rojo hoy; `cargo test --locked --features e2e 'e2e::arnes'`).
- [x] 1.2 Test del caso definitivo: un sondeo que acierta siempre da el arranque por bueno tras **dos** sondeos separados por la espera (verificación: en rojo hoy).
- [x] 1.3 Test de que la espera entre sondeos es la **inyectada**, no una constante interna (verificación: en rojo hoy).
- [x] 1.4 Test de que agotado el plazo el ayudante falla nombrando el **último motivo**, sin un ok silencioso (verificación: en rojo hoy).

**GREEN**

- [x] 1.5 `tests/e2e/arnes.rs`: ayudante puro de espera con supervivencia —sondeo y espera como parámetros, `tokio::time` solo en el borde, `Result<(), String>` con el último motivo (verificación: tests 1.1–1.4 en verde).
- [x] 1.6 `tests/e2e/escenarios.rs`: `mariadb_arrancado` usa el ayudante (verificación: arranca y siembra contra el contenedor real).
- [x] 1.7 `tests/e2e/escenarios.rs`: `postgres_arrancado` usa el mismo ayudante (verificación: arranca y siembra contra el contenedor real).
- [x] 1.8 `alpine_con_sqlite` y la espera del S3 no cambian (verificación: el diff no los toca y sus escenarios siguen verdes).

**REFACTOR**

- [x] 1.9 Rustdoc del ayudante: por qué un acierto no basta y por qué la espera se inyecta (verificación: `cargo doc` sin warnings).
- [x] 1.10 Pruebas del requisito nuevo agrupadas bajo su encabezado, junto a las de la sonda que nombra la base (`escenarios.rs:440-500`) (verificación: quedan agrupadas).
- [x] 1.11 `just check` en verde (verificación: termina sin error).

**CIERRE DE LA REVISIÓN — MEDICIÓN Y CABOS**

La ventana del servidor temporal se midió con el sondeo dentro del contenedor y, sobre todo, con los hitos del entrypoint (`podman logs -t`), 5 corridas por motor en máquina ociosa: **31 ms** en MariaDB (`mariadb:11`) y **148 ms** en PostgreSQL (`postgres:18-alpine`) de máximo. La espera `W` = `ESPERA_ENTRE_SONDEOS` = **2 s** es ~13× ese máximo: holgura deliberada frente a la carga, pagada **una vez por arranque de contenedor**. Detalle en `design.md` D6.

- [x] 1.12 `ESPERA_ENTRE_SONDEOS` = **2 s**, con la medición como motivo en su doc (verificación: la constante y su doc citan la medición).
- [x] 1.13 El plazo acota el **primer** acierto; la confirmación tiene su holgura (límite interno `plazo + espera`) (verificación: el test del plazo sigue verde).
- [x] 1.14 El test de la espera inyectada **discrimina**: espera de 20 ms y cotas `>= 20 ms` y `< 500 ms` (verificación: la cota superior delata una constante interna de 2 s).
- [x] 1.15 `design.md`/`proposal.md`: no dan por especificado el «la sonda nombra la base» del #170 y dicen «sin esperar la espera de producción» (verificación: `rg` no encuentra la cita como escenario de spec).
- [x] 1.16 El test de `escenarios.rs` se renombra por lo que cubre: `las_sondas_que_nombran_la_base_exigen_la_supervivencia_del_definitivo` (verificación: el nombre dice la verdad).
- [x] 1.17 `design.md` D6 con la tabla de la medición, los hitos, la garantía real y el residuo acotado (verificación: D6 existe).
- [x] 1.18 El spec rebaja la promesa a la garantía real, con el residuo declarado y sin renombrar escenarios (verificación: `openspec validate --strict` → valid).

### 2. Verificación del flake

- [x] 2.1 `just e2e` repetido (al menos 5 corridas) sin el `ERROR 2002` de siembra (verificación: ninguna corrida falla en la siembra de MariaDB ni de PostgreSQL).
- [x] 2.2 La suite en paralelo sigue verde: `cargo test --locked --features e2e --all-targets` (verificación: 0 fallos).
- [x] 2.3 Las pruebas existentes de las sondas siguen verdes —`la_sonda_de_mariadb_consulta_la_base_sembrable` (`escenarios.rs:486`), `la_sonda_de_postgres_consulta_la_base_sembrable` (`:449`)— y las del #141 también (verificación: sin fallos).
- [x] 2.4 `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` limpios (verificación: 0 warnings).
- [x] 2.5 `just e2e` una vez con `W` = 2 s (33/33, 1197 s) y repeticiones dirigidas de `mariadb`/`postgres` con `--test-threads=1` (3 + 4 + 5 corridas de PostgreSQL; todas verdes salvo **una** en `copia_y_restauracion_de_postgresql_conserva_las_filas`, no reproducida en 10 repeticiones posteriores) (verificación: la siembra no vuelve a dar `ERROR 2002`).
- [x] 2.6 `openspec validate arreglo-flake-e2e-171 --strict` → valid (verificación: valid).
- [ ] 2.7 Cabo abierto: un fallo único y **no reproducido** en `copia_y_restauracion_de_postgresql_conserva_las_filas` (1 de 4 corridas dirigidas; mensaje no conservado). Vigilar en CI (verificación: pendiente).

### 3. Cierre

- [x] 3.1 Marcar las tareas hechas y abrir el PR a `development` con `Closes #171` (verificación: PR creado).
- [x] 3.2 `docs/FEATURE-042-arreglo-flake-e2e-171.md` con qué es, ficheros, decisiones —la opción elegida y las descartadas— y cómo verificar (verificación: el documento existe).
- [ ] 3.3 Renombrar este plan a `plans/PLAN-042-COMPLETED.md` con `git mv` (verificación: renombrado).
- [ ] 3.4 Archivar el change: `openspec archive arreglo-flake-e2e-171` (verificación: el change pasa a `openspec/changes/archive/` y el requisito nuevo entra **al final** de `openspec/specs/arnes-de-pruebas/spec.md`, sin dejar huérfanos los escenarios de los requisitos anteriores).

## 5. Riesgos

| Riesgo | Mitigación |
|---|---|
| La supervivencia añade latencia al arranque de cada contenedor. | La espera entre sondeos vale 2 s (~13× la ventana medida: 31 ms en MariaDB, 148 ms en PostgreSQL) y se paga **una vez por contenedor**, entre los dos sondeos de la confirmación. |
| Un servidor que tarde más que la espera entre sondeos no se detecta antes de tiempo. | El plazo (`PLAZO_DE_ARRANQUE`) no cambia: el acierto solo se retrasa, nunca se adelanta. |
| El ayudante nuevo se cuela también en SQLite. | El spec lo prohíbe expresamente; el ayudante va por parámetro o es propio, y la tarea 1.8 lo comprueba. |
