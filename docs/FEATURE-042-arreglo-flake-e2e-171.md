# FEATURE-042 — Arreglo del flake e2e: la sonda de arranque no se conforma con el servidor temporal (#171)

- **Issue:** #171. **Change OpenSpec:** `arreglo-flake-e2e-171`. **Plan:** `plans/PLAN-042.md`.
- **Tipo:** infraestructura de pruebas. **Cero cambios en `src/`**: es un arreglo del arnés e2e (`tests/e2e/`), sin tocar producto, contrato HTTP, base de datos ni panel.
- **Capability:** `arnes-de-pruebas`. Precedentes de flake dedicados: #141 (`FEATURE-034`) y #142 (`FEATURE-035`).

## Qué es

La espera de arranque de un motor daba la base por lista con el **primer** acierto de la sonda. Para un servicio de un solo arranque eso basta, pero las imágenes oficiales de MariaDB y de PostgreSQL levantan un **servidor temporal** para inicializarse —que **ya tiene la base creada**, porque la crea él— y lo apagan antes de arrancar el definitivo. Si la sonda acierta dentro de esa ventana, el escenario siembra contra un servidor que está a punto de morir y se encuentra el socket ausente:

```
ERROR 2002 (HY000): Can't connect to local server through socket '/run/mysqld/mysqld.sock' (2)
```

El código `2` es `ENOENT`. No era una regresión de #170: la carrera está en el arnés y es previa. La ventana es **real pero corta** —decenas de milisegundos—, y por eso el flake era raro.

El arreglo es que la espera exija **dos aciertos separados por una espera**, de modo que un temporal que se apaga antes de esa espera no confirme el arranque. La garantía es acotada y se declara como es (ver «Decisiones» y «Follow-ups»).

## Ficheros implicados

- `tests/e2e/arnes.rs` — **único código tocado** (312 líneas en el diff):
  - `ESPERA_ENTRE_SONDEOS` (`:865`), la espera entre los dos sondeos, **2 s**, con la medición como motivo en su doc.
  - `esperar_arranque(contenedor, cmd, plazo, espera, sonda)` (`:910-960`), el ayudante nuevo: bucle **puro** que recibe el sondeo como clausura y la espera como parámetro, con `tokio::time` solo en el borde.
  - Cuatro `#[tokio::test]` del ayudante: `el_servidor_temporal_no_da_el_arranque_por_bueno` (`:2017`), `el_servidor_definitivo_da_el_arranque_por_bueno_tras_dos_sondeos` (`:2062`), `la_espera_entre_sondeos_es_la_inyectada` (`:2100`) y `agotado_el_plazo_falla_con_el_ultimo_motivo` (`:2152`).
  - **Intactos**: `Motor::esperar_orden` (`:738-762`), las sondas `sonda_de_postgres` (`:197-212`) y `sonda_de_mariadb` (`:214-232`), `PLAZO_DE_ARRANQUE` (`:147`), `esperar_el_s3` (`:990`) y las constantes del #141 (`INTENTOS_DEL_ALTA`, `ESPERA_ENTRE_ALTA`, `reintentar`).
- `tests/e2e/escenarios.rs` — los dos puntos de llamada que cambian de `esperar_orden` a `esperar_arranque`: `mariadb_arrancado` (`:177-191`) y `postgres_arrancado` (`:277-291`). El test del requisito nuevo, `las_sondas_que_nombran_la_base_exigen_la_supervivencia_del_definitivo` (`:557-595`), agrupado bajo su encabezado junto a los de la sonda que nombra la base (`:488` y `:525`). **No** se toca `alpine_con_sqlite` (`:382`, sigue con `esperar_orden`) ni el contenedor «sordo» del escenario de restauración (`:1274`, también `esperar_orden`).
- `openspec/changes/arreglo-flake-e2e-171/` — `proposal.md`, `design.md` (D1–D6), el delta de `arnes-de-pruebas` y `tasks.md`.
- `plans/PLAN-042.md` y este documento.
- `ui/`: **sin cambios**.

## Cómo funciona

### El problema: la ventana del servidor temporal

El entrypoint oficial levanta primero un **servidor temporal**, inicializa en él y lo apaga antes del definitivo. La sonda ya **nombra la base** (`sonda_de_mariadb`, `sonda_de_postgres`), y eso la protege del temporal *antes* de que la base exista, pero no de su tramo final: la base la crea el propio temporal, así que la sonda puede acertar contra él. Quien se conforme con ese acierto siembra justo cuando el temporal se apaga.

Detalle que explica por qué el flake era raro: contra el temporal de MariaDB el sondeo recibe en bucle `Access denied for user 'root'@'localhost' (using password: YES)` hasta que el entrypoint «asegura» el root, así que la ventana se abre justo ahí.

### El arreglo: dos aciertos con supervivencia

`esperar_arranque` (`arnes.rs:910`) no devuelve `Ok(())` con el primer acierto: cuando la sonda acierta, **duerme `espera`** y vuelve a sondear; solo si el segundo sondeo también acierta da el arranque por bueno. Un servidor temporal que se apaga entre los dos sondeos no confirma; el definitivo, que sigue en pie, sí. `ESPERA_ENTRE_SONDEOS` = **2 s** (`arnes.rs:865`).

El ayudante es **puro y testeable**: recibe el sondeo como clausura `FnMut() -> Future` y la espera como parámetro —el `tokio::time` vive solo en el borde que duerme de verdad—, igual que el `reintentar` del #141 recibe el «intento». Así una prueba unitaria simula la secuencia temporal → definitivo sin contenedores y sin esperar la espera de producción. `Motor::esperar_orden` **no cambia** para lo demás: SQLite (`alpine`, sin servidor temporal) no paga una latencia ni gana un modo de fallo que no le corresponden.

### La medición

La ventana del temporal se midió **5 corridas por motor** con el sondeo *dentro* del contenedor y, sobre todo, **desde los hitos del entrypoint** (`podman logs -t`), que es determinista. Los valores y la tabla están en `design.md` (D6):

| Motor | Ventanas medidas (ms) | **Máximo** |
|---|---|---|
| MariaDB (`mariadb:11`) | 31 · 26 · 25 · 28 · 25 | **31** |
| PostgreSQL (`postgres:18-alpine`) | 134 · 137 · 133 · 148 · 131 | **148** |

- **MariaDB**: de `Creating database` / `Securing system users` a `Stopping temporary server`.
- **PostgreSQL**: del `database system is ready to accept connections` del temporal a su `LOG: shutting down`.

`W` = 2 s es **~13× el máximo medido**: no es el valor de la ventana, es **holgura deliberada** frente a la carga —la suite levanta contenedores en paralelo y la máquina puede estar ocupada—. El coste se paga **una vez por arranque de contenedor**: la espera va entre los dos sondeos de la confirmación, no antes de cada sondeo.

### El borde del plazo

El plazo (`PLAZO_DE_ARRANQUE`, `arnes.rs:147`) acota la espera del **primer** acierto; la confirmación —la espera y el segundo sondeo— dispone de su propia holgura. Por eso el límite interno del bucle es `plazo + espera` y no `plazo` (`arnes.rs:924`): un servidor que aparece al final del plazo **se confirma** en vez de fallar por unos milisegundos. El contrato de error no cambia: agotado el límite se falla **en voz alta** nombrando contenedor, orden y el **plazo visible**, con el último motivo (`arnes.rs:953-955`).

## Variables y configuración

Ninguna nueva. `ESPERA_ENTRE_SONDEOS` es una constante interna del arnés; no hay variables de entorno ni cambios de despliegue. El efecto observable es que el e2e deja de ser intermitente en la siembra de MariaDB y PostgreSQL.

## Cómo verificar

- Tests del ayudante (rápidos, sin contenedores): `cargo test --locked --features e2e 'e2e::arnes'` → **9 passed** (los 4 nuevos y los 5 del #141).
- Suite en paralelo: `cargo test --locked --features e2e --all-targets` → **0 fallos**.
- Batería oficial (serie): `just e2e` → **33 passed / 0 failed en 1197 s**.
- Puerta habitual: `just check` (fmt + clippy + tests) → **exit 0**.
- Verificación de la entrega (resultados reales): `cargo fmt --all -- --check` ok; `cargo clippy --all-targets -- -D warnings` y con `--features e2e`, **0 warnings**; `cargo test --locked` → **641 passed / 0 failed / 27 ignored**, más 5 passed; `openspec validate arreglo-flake-e2e-171 --strict` → **valid**.
- Reproducción dirigida: repeticiones de `mariadb`/`postgres` y del escenario de PostgreSQL aislado con `--test-threads=1` → **11 de 12 verdes** (el fallo restante es el cabo abierto de abajo, no la siembra: la siembra no vuelve a dar `ERROR 2002`).

## Decisiones

- **Opción A: dos sondeos con supervivencia entre ellos.** Elegida. No asume nada de la imagen; si el servidor definitivo tarda de más, solo se retrasa el acierto.
- **Descartada B: sondear una señal exclusiva del definitivo** (su escucha TCP, que el temporal no tiene: MariaDB arranca el temporal con `--skip-networking`, PostgreSQL con `listen_addresses=''`). Es determinista y por eso tienta, pero ata la sonda al `bind-address`, al `listen_addresses` y al reparto de permisos (`root@localhost` vs `root@%`, `pg_hba`) de cada imagen. Si esa suposición se rompe, la espera **no acierta nunca** y el fallo pasa de flake ocasional a plazo agotado en cada corrida: peor que el problema que se arregla.
- **Descartada C: reintentar la siembra ante un fallo de conexión.** Trata el síntoma donde no está —lo que miente es la sonda— y tendría que distinguir un fallo de conexión de un error real para no duplicar filas.
- **El alcance llega a PostgreSQL, y no por analogía.** Su entrypoint tiene el mismo servidor temporal (`docker_temp_server_start` con `listen_addresses=''` → `docker_setup_db` → `docker_temp_server_stop`), así que la carrera es la misma; corregir un motor y dejar el otro con el flake a sabiendas sería incoherente.
- **`esperar_orden` no se cambia para todos.** SQLite no tiene servidor temporal; el cambio va por el ayudante nuevo, que usan los dos arranques que sí lo tienen. Las constantes del #141 no se tocan (son del alta de repositorio, ajenas al arranque de un motor).
- **El plazo acota el primer acierto, no la confirmación** (límite interno `plazo + espera`): un servidor que aparece al final del plazo se confirma en vez de fallar por unos milisegundos.
- **Garantía real y residuo, sin adornos.** Lo garantizado es que **el temporal que se apaga antes de `W` no confirma el arranque**. Un temporal que sobreviviera **más de `W`** tras el primer acierto seguiría colándose; cerrarlo del todo pediría una señal exclusiva del definitivo, descartada por frágil (atada al `bind-address`/`listen_addresses` de cada imagen). Con los números medidos, haría falta que el entrypoint tardase **más de 2 s** entre crear la base y apagar el temporal.

## Follow-ups

- **Cabo abierto, vigilado.** En una de las corridas dirigidas falló **una vez** y **no se reprodujo** `copia_y_restauracion_de_postgresql_conserva_las_filas` (escenario de **restauración**, ajeno a la sonda; su cuerpo no lo toca #171). Se está reproduciendo aparte; si no reaparece, queda como residuo vigilado en el E2E del CI, y si vuelve hay que capturar el motivo completo (tarea 2.7 del plan).
- **Alcance del arreglo.** Es del arnés de pruebas: no cambia el producto, el contrato HTTP, la base de datos ni el panel. Un merge a `main` es una release (`patch`).
