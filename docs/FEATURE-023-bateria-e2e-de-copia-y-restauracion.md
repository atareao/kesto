# FEATURE-023 — Batería e2e de copia y restauración

- **Issue**: #86 · **Plan**: `plans/PLAN-023.md` · **Change SDD**: `openspec/changes/bateria-e2e-de-copia-y-restauracion/` (capacidad nueva `verificacion-e2e`: **8 requisitos y 21 escenarios**)
- **Rama**: `feature/bateria-e2e-de-copia-y-restauracion`

## Qué es

Hasta esta entrega, el ciclo completo **copia → S3 → restauración** no estaba verificado de punta a punta: los **27 tests `#[ignore]`** que ejercen el motor de contenedores nunca se ejecutaban en CI —`ci.yml` solo corría `cargo test --locked`—, el almacén de todos los tests era un doble local (`abridor_local`), el único doble remoto simulaba un endpoint caído y no errores 4xx/5xx, la restauración de un directorio no tenía ninguna prueba y el camino S3 → `volcar_a` → `LectorDelVolcado` → `stdin` del `exec` no se ejercía. Verificar con S3 real era una prueba manual con `lab/`.

Esta entrega añade una **batería reproducible** que ejecuta ese ciclo por cada tipo de origen —**mariadb, postgresql, sqlite y directorio**— contra un **S3 real** (SeaweedFS) y un **motor de contenedores real**, y que **compara el contenido restaurado con el sembrado**, no el código de salida. Cubre además los huecos del inventario: restauración hacia contenedor, camino S3→`stdin`, contenedor que no drena su `stdin`, sobrescritura con y sin confirmación, retención y purga contra S3, e higiene de credenciales. El arnés **falla en voz alta**: si falta el motor, el S3 o una imagen, falla nombrando lo que falta, nunca se salta un escenario.

En el camino la batería **destapó y arregló un defecto real** —la restauración de un directorio anidaba la ruta absoluta del origen dentro del destino— (ver *El defecto destapado*).

## Ficheros implicados

| Capa | Fichero | Papel |
|---|---|---|
| Batería | `tests/e2e/main.rs` | Entrada del binario de test: `mod arnes` + `mod escenarios`; documenta por qué `--test-threads=1` y por qué vive tras la feature `e2e` |
| Batería | `tests/e2e/arnes.rs` | El arnés: motor (`bollard` por `DOCKER_HOST`), SeaweedFS (config, arranque, espera con plazo), el **binario** de Kesto arrancado y hablado por HTTP, recogida de restos, y las comparaciones `comparar_arboles` / `comparar_filas` |
| Batería | `tests/e2e/escenarios.rs` | Los **21 escenarios** de `verificacion-e2e`, uno por prueba, con sus ayudantes de siembra y lectura por tipo |
| Build | `Cargo.toml` | Feature `e2e = []` y `[[test]] e2e` con `required-features = ["e2e"]` |
| Receta | `.justfile` | Receta `e2e`: exporta `DOCKER_HOST` (podman por defecto) y lanza la batería |
| CI | `.github/workflows/ci.yml` | Job `e2e`, aislado del job `backend`: motor del runner, imágenes fijadas y pre-descargadas, los `#[ignore]` y la batería |
| Defecto | `src/rustic/repositorio.rs` | `Repositorio::restaurar_snapshot` **descuenta el prefijo** de la ruta declarada; `ruta_declarada`; **poda** por `RestoreOptions::delete(podar)`; test de la raíz del destino y de la poda |
| Defecto | `src/engine/mod.rs` | `Motor::restaurar` recibe `sobrescribir` y lo pasa a `restaurar_en_el_directorio` y de ahí a `restaurar_snapshot` como `podar`; test de directorio corregido |
| Defecto | `src/api/snapshots.rs` | `sobrescribir` viaja **entero** al motor (para un directorio autoriza la poda, no se ignora); test corregido |
| Apoyo | `src/test_support.rs` | Rustdoc de `Siembra`: la ruta del árbol reproduce la de la fuente (la usa `dump`, no la restauración) |

## Cómo funciona

### El arnés: cuatro piezas reales, ningún doble

`Mundo::nuevo()` monta, una vez por escenario, las cuatro piezas:

1. **El motor de contenedores**, por el socket que diga `DOCKER_HOST` (en dev, podman; en CI, Docker). Se conecta con el mismo `bollard` que el motor de Kesto, **fijando la versión de API en la llamada** —`Docker::connect_with_socket(socket, 600, bollard::API_DEFAULT_VERSION)`—, nunca con una deducción.
2. **El servidor S3**: un contenedor SeaweedFS levantado por el propio arnés, con una identidad de credenciales fijas y un **cubo propio del escenario**. Se espera su disponibilidad con plazo (`esperar_el_s3`); un `403` cuenta como «está», y si no llega **no se degrada** a un almacén local.
3. **El binario de Kesto**, arrancado en modo `Disabled` (sin ninguna `OIDC_*`), con su `DATABASE_URL` en un directorio temporal y su puerto libre. Se habla con él por HTTP: alta de repositorio/target/tarea, `run`, listados de ejecuciones, snapshots y restauraciones.
4. **Las comparaciones**, que son la razón de ser de la batería: `comparar_arboles` recorre los dos árboles y cuenta lo que falta, lo que sobra y el primer byte distinto; `comparar_filas` compara los listados de filas. El **escenario 5** (`una_diferencia_de_datos_rompe_la_prueba`) siembra una diferencia deliberada para probar que la comparación **rompe la prueba**.

El arnés usa el **binario** y no la biblioteca porque el crate es solo binario (`src/main.rs`, sin `[lib]`) y su arnés interno es `#[cfg(test)]`.

### Ejecución en serie y aislamiento

- **`--test-threads=1`**: cada escenario levanta su propio S3, su binario y sus contenedores; son mundos independientes, pero un motor que arranca cuatro bases en paralelo mide la máquina, no el motor de copias. En serie, un fallo se lee entero.
- **Aislamiento entre ejecuciones**: el S3 de cada escenario tiene cubo propio (`kesto-e2e-<marca>`), así que dos ciclos no comparten snapshots.
- **Recogida de restos acotada al propio run**: cada contenedor se crea con la etiqueta `kesto-e2e-pid=<pid del proceso que corre la batería>`. `Motor::conectar` barre, **una vez por proceso**, los contenedores de la batería cuyo `pid` **ya no existe** —los de una ejecución muerta— y no toca los de una ejecución viva. Se descartó barrer por el prefijo del nombre: dos ejecuciones en paralelo —otra terminal o el CI— se matarían entre sí.

### El defecto destapado

La restauración de un **directorio** anidaba la **ruta absoluta** del origen dentro del destino —`<destino>/tmp/x/origen/a.txt` en vez de `<destino>/a.txt`—. La instantánea guarda el contenido con la ruta con la que se **montó** (`backup_sources/x`), y `restaurar_snapshot` empezaba el recorrido en la raíz **sintética**, que recreaba esa ruta entera dentro del destino; el contrato del motor para un target `directory` es escribir al `host_path` con el contenido en su raíz.

El arreglo es en la **restauración**, no en la copia: así no se rompe el contrato de `extraction` y se reparan los snapshots **ya guardados**. `ruta_declarada` decide el prefijo —la ruta de la que cuelga el árbol cuando la instantánea declara **una sola**; con varias no hay prefijo único que descontar, y descontar uno dejaría fuera del destino al resto— y se pasa a `node_from_snapshot_and_path`. **Dos pruebas existentes** que aseveraban la colocación anidada se corrigieron: `restaurar_una_instantanea_de_un_target_de_directorio_devuelve_su_ejecucion_y_su_target` (`src/api/snapshots.rs`) y la de la rama de directorio del motor (`src/engine/mod.rs`). Se añadió una prueba nueva sin contenedores ni S3 en `src/rustic/repositorio.rs` que mide las dos mitades —que la instantánea declara la ruta montada y que la restauración la descuenta— sorteando la ruta del origen con un UUID para que el anidado no se confunda con el contenido.

### La poda la autoriza la sobrescritura

`sobrescribir: true` no es solo la puerta del `428` para un target de base: para un target de **directorio** es lo que **autoriza la poda** del destino —`RestoreOptions::delete(podar)`, retirar de él lo que no está en la instantánea—. Sin la bandera, un directorio **solo recibe** (escribe encima de lo que coincide y deja el resto). No se poda por defecto porque `delete: true` borra del host datos que pueden no tener copia, y una restauración lanzada por error sobre el directorio equivocado se los llevaría por delante. La poda es de **mejor esfuerzo** —es de la biblioteca—: retirar un fichero que no se deja se registra y la restauración sigue.

## Cómo se ejecuta

### En local

```text
just e2e
```

La receta exporta `DOCKER_HOST` —si no está puesto, al socket de podman de esta máquina— y ejecuta:

```text
cargo test --features e2e --test e2e -- --test-threads=1
```

Cuesta **minutos** (~1150 s): levanta un servidor y varios contenedores por escenario, y uno de ellos espera al **plazo de inactividad** del motor (5 min). Sin un `DOCKER_HOST` válido, la ejecución **falla** nombrando la variable y el socket esperado, y no se salta ningún escenario.

### En CI (job `e2e`)

El job `e2e` corre en `ubuntu-26.04`, **aislado del job `backend`** (sin `needs:` y con su Docker propio, para que un fallo del runner de contenedores no arrastre al pipeline principal; `timeout-minutes: 30`), y ejecuta **dos pasos**:

| Paso | Comando | Qué corre |
|---|---|---|
| Integration tests (ignored) | `cargo test --locked -- --ignored --test-threads=1` | los **27** `#[ignore]` |
| E2E tests | `cargo test --locked --features e2e --test e2e -- --test-threads=1` | los **21** escenarios |

Los dos van por un tubo (`set -o pipefail` + `tee`) para dejar el registro en un artefacto, y el desenlace del tubo es el del **último** comando: sin `pipefail`, un fallo de la batería saldría con el cero del `tee` y el job quedaría verde. Antes de ejecutar, el job pre-descarga las imágenes.

El job `backend` **no** cambia: sigue corriendo `cargo fmt`, `clippy` y `cargo test --locked` sin motor de contenedores ni S3. La batería vive tras la **feature** `e2e` con `required-features`, así que `cargo test --locked` no la compila ni la ejecuta.

## Variables y configuración

La batería no añade variables de entorno al producto. Lo que consume:

| Variable | Dónde | Para qué |
|---|---|---|
| `DOCKER_HOST` | local y CI | Socket del motor. En local lo pone la receta (podman por defecto); en CI apunta a `unix:///var/run/docker.sock` |
| `MARIA_IMAGE` | CI | Imagen MariaDB de origen (fijada y pre-descargada) |
| `POSTGRES_IMAGE` | CI | Imagen PostgreSQL de origen |
| `ALPINE_IMAGE` | CI | Contenedor de apoyo (ficheros SQLite, contenedor que no drena `stdin`) |
| `S3_IMAGE` | CI | Imagen del servidor SeaweedFS |
| `KESTO_MASTER_KEY` | CI | Clave de relleno para los `#[ignore]`; el arnés arranca el binario con su propia clave constante |

El arnés respeta las imágenes del CI: `imagen(variable, por_defecto)` toma la variable si está y, si no, la de por defecto (las mismas versiones), para que las imágenes «fijadas por versión» no vivan en un fichero que nadie lee.

## Decisiones

- **D1 — S3 real: SeaweedFS.** MinIO quedó descartado por no ser descargable sin credenciales, `moto` ya se usó en backup-engine pero no sirve como destino S3 real de Rustic, y `rclone serve s3` mapea marcadores de directorio a ficheros de 0 bytes. El arnés levanta su propio SeaweedFS y **no degrada** a un almacén local.
- **D2 — Sin dependencias nuevas en `Cargo.toml`.** Los contenedores se orquestan con el `bollard` que ya existe y HTTP con el `reqwest` de los tests. SeaweedFS y las bases son **imágenes externas**, no crates.
- **D3 — Los `#[ignore]` se conservan y se ejecutan.** La batería los corre tal cual en el job `e2e`; no se reescribe su marcado.
- **D4 — La batería no se salta nunca.** Un entorno incompleto es un **fallo** nombrando lo que falta, no un `skip`.
- **D5 — `--test-threads=1`.** Un motor que arranca cuatro bases en paralelo mide la máquina; en serie, un fallo se lee entero.
- **D6 — El arreglo del anidado va en la restauración, no en la copia.** Preserva el contrato de `extraction` y repara los snapshots ya guardados.
- **D7 — La poda la autoriza `sobrescribir`.** Para un directorio, sin la bandera solo se recibe; no se poda por defecto porque el borrado de datos sin copia es irreversible.
- **D8 — La limpieza de restos se acota al propio run** (etiqueta `kesto-e2e-pid=<pid>`), para que dos ejecuciones en paralelo no se maten entre sí.
- **D9 — La batería es una feature de Cargo**, no un simple fichero de `tests/`, para que el job `backend` siga sin motor de contenedores.

## Verificación

- **Batería**: **21/21 en verde** con `cargo test --features e2e --test e2e -- --test-threads=1` (~1150 s), contra podman en local y Docker en CI.
- **Suite del backend**: `cargo test --locked` → **531 passed / 0 failed / 27 ignored**. Los **27** `#[ignore]` se ejecutan en el job `e2e`.
- **El defecto, medido**: el escenario de directorio destapó el anidado; se corrigió en `restaurar_snapshot`, con prueba nueva en `src/rustic/repositorio.rs` y dos pruebas existentes corregidas.
- **SDD**: `openspec validate bateria-e2e-de-copia-y-restauracion --strict` en verde; `just check-spec` con change activo.
- **Puertas**: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` y `cargo test --locked` limpios (`just check`).

## Follow-ups

- **Tiempo de ejecución**: el job `e2e` es más lento que `backend`; se mantiene separado a propósito para no penalizar el ciclo rápido. Si el CI se encarece, reconsiderar la separación de escenarios.
- **Descarga de imágenes en CI**: se mitiga fijando versiones y con un plazo explícito; un fallo de red se ve como fallo del job, nunca como éxito.
- **Sin dependencias nuevas**: si algún día hiciera falta algo más que `bollard`/`reqwest`, se abre ticket aparte en vez de engordar el `Cargo.lock`.
- **Restauración parcial por rutas, copia previa automática y progreso incremental real**: siguen siendo follow-ups de #67 (son features, no pruebas); esta entrega ya cierra el follow-up «e2e de los `#[ignore]` en CI».
- **`INACTIVIDAD_MAXIMA` no es inyectable**: es una constante del módulo, y el escenario del contenedor que no drena su `stdin` lo espera entero (5 min). Hacerlo ajustable acortaría el escenario, pero es deuda del motor, no de la batería.
