# Proposal

## Why

El escenario `escenarios::una_credencial_incorrecta_no_se_publica_en_el_motivo` (`tests/e2e/escenarios.rs:819`) falla de forma intermitente **en la siembra**, antes de tocar la aplicación: `la siembra del MariaDB tenía que terminar con código 0 y terminó con 1` con `ERROR 2002 (HY000): Can't connect to local server through socket '/run/mysqld/mysqld.sock' (2)` — el código `2` es `ENOENT`, el socket **no existía** en ese instante (issue #171, detectado en el E2E de #170). La espera de arranque (`mariadb_arrancado` → `Motor::esperar_orden`, `tests/e2e/escenarios.rs:150-175` y `tests/e2e/arnes.rs:736-760`) había dado el arranque por bueno un momento antes.

No es una regresión de #170: la carrera está en el arnés y es previa. La imagen oficial de MariaDB levanta un **servidor temporal** para inicializar, y lo apaga antes de arrancar el definitivo. El arnés ya lo sabe y por eso la sonda nombra la base —`tests/e2e/arnes.rs:212-230`, porque un cliente sin base contesta que sí contra el temporal antes de que la base exista—. Ese «la sonda nombra la base» **no está en ningún spec**: `rg` no encuentra el escenario en `openspec/`, vive en las pruebas de `escenarios.rs` y en el `CHANGELOG`, así que no se da por especificado. Pero queda un hueco: **la base la crea el propio servidor temporal**, así que la sonda puede acertar en su tramo final, y entonces el temporal se apaga y la siembra se encuentra el socket ausente. La espera, hoy, se conforma con el **primer** acierto.

El mismo hueco está en PostgreSQL, y no por analogía sino por el mismo mecanismo: el entrypoint oficial arranca un servidor temporal con `listen_addresses=''` (`docker_temp_server_start`), crea `POSTGRES_DB` en él (`docker_setup_db`) y lo para (`docker_temp_server_stop`) antes de arrancar el definitivo; la sonda de `tests/e2e/arnes.rs:197-210` nombra la base y por eso puede acertar contra el temporal. SQLite (contenedor `alpine` con el cliente, `escenarios.rs:322-345`) y el servidor S3 no tienen servidor temporal: su espera no cambia.

## What Changes

- **Solo el arnés e2e** (`tests/e2e/arnes.rs` y `tests/e2e/escenarios.rs`): la espera de arranque de un motor deja de conformarse con el primer acierto de la sonda y pasa a exigir que el servidor que contesta **sobreviva** a una espera corta entre dos sondeos. El servidor temporal que se apaga **antes** de esa espera no la da por buena; el definitivo sí. La garantía es acotada: un temporal que sobreviviera más que la espera no queda cubierto (ver `design.md`, D6).
- **Opción elegida (A: dos sondeos con supervivencia), y por qué.** Se descarta sondear una señal exclusiva del definitivo (B, p. ej. su escucha TCP, que el temporal no tiene: MariaDB arranca el temporal con `--skip-networking` y PostgreSQL con `listen_addresses=''`): es determinista pero ata la sonda a la configuración de red, autenticación y `bind-address` de cada imagen —si esa suposición se rompe, la espera no acierta nunca y el fallo pasa de un flake ocasional a un plazo de 180 s agotado en cada corrida—. Se descarta reintentar la siembra (C): trata el síntoma en el sitio equivocado —lo que miente es la sonda— y tendría que distinguir un fallo de conexión de un error real para no duplicar filas. La opción A no depende de ninguna suposición sobre la imagen y, si el servidor tardara de más, solo retrasa el acierto.
- El mecanismo se implementa como un ayudante **puro y testeable**: recibe el **sondeo** y la **espera** inyectados —igual que el `reintentar` del #141 (`tests/e2e/arnes.rs:1408-1428`) recibe el «intento» como clausura—, de modo que una prueba unitaria simule la secuencia temporal —acierto durante el temporal, fallo después, acierto estable al final— sin contenedores y sin esperar la espera de producción.
- Se aplica a MariaDB (`escenarios.rs:150-175`) y a PostgreSQL (`escenarios.rs:230-254`), que comparten la carrera y el mismo ayudante.
- La espera conserva su plazo (`PLAZO_DE_ARRANQUE`, `tests/e2e/arnes.rs:147`) y sigue fallando **en voz alta** con el último motivo —contenedor, orden y plazo— cuando vence. El plazo acota la espera del **primer** acierto; la confirmación tiene su propia holgura, de modo que un servidor que aparece al final del plazo se confirma en vez de fallar.

## Non-goals

- NO se toca el producto, el contrato HTTP, la base de datos ni el panel: **cero cambios en `src/`**. El arreglo es del arnés de pruebas.
- NO se tocan las constantes ni el bucle del #141 (`INTENTOS_DEL_ALTA`, `ESPERA_ENTRE_ALTA`, `reintentar`): son del alta de repositorio y no tienen relación con el arranque de un motor.
- NO se reintenta la siembra de ningún motor (opción C descartada arriba).
- NO se sube `PLAZO_DE_ARRANQUE`, no se relaja ninguna aserción y no se retira ningún escenario existente: se conserva la cobertura actual.
- NO se toca el arranque de SQLite ni la espera del servidor S3 (`esperar_el_s3`, `arnes.rs:870`): no tienen servidor temporal.
- NO se introduce ninguna dependencia nueva (build offline y versiones pinadas).

## Capabilities

### New Capabilities

_(ninguna)_

### Modified Capabilities

- `arnes-de-pruebas`: la espera de arranque de un motor deja de conformarse con el primer acierto de la sonda y pasa a exigir que el servidor que contesta sobreviva a una espera corta entre dos sondeos, de modo que el servidor temporal del arranque —que contesta con la base ya creada y luego se apaga— no se dé por definitivo. El requisito nuevo se añade **después** de los existentes de la capability.
