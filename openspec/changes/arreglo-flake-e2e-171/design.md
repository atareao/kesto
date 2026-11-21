# Design — la espera de arranque no se conforma con el servidor temporal

## D1 · El defecto está en la espera, no en la siembra

`Motor::esperar_orden` (`tests/e2e/arnes.rs:736-760`) devuelve `Ok(())` con el **primer** acierto de la sonda. Eso basta para un servicio que solo tiene un arranque, pero no para las imágenes oficiales de MariaDB y PostgreSQL, que levantan un **servidor temporal** —con la base ya creada, porque la crea él— y lo apagan antes del definitivo. Quien siembra justo después del apagado se encuentra el socket ausente (`ERROR 2002`, `ENOENT`), que es el fallo del issue #171.

Por eso el arreglo va en la espera: es ella la que afirma «arrancado» cuando el servidor que contesta es el que se va a apagar.

Se descarta la opción C del issue —reintentar la siembra ante un fallo de conexión—: trataría el síntoma donde no está, obligaría a distinguir un fallo de conexión de un error real de SQL para no reintentar lo que no se debe, y un reintento tras un fallo ambiguo podría duplicar filas. La sonda es la que miente; se corrige la sonda.

## D2 · Supervivencia entre dos sondeos (A), y no una señal exclusiva del definitivo (B)

Se elige A: dos aciertos separados por una espera corta, de modo que el servidor que contesta **sobreviva** entre ambos. El temporal, que se apaga, no la da por buena; el definitivo sí.

Se descarta B —sondear una señal que solo emita el definitivo, como su escucha TCP, que el temporal no tiene: MariaDB arranca el temporal con `--skip-networking` y PostgreSQL con `listen_addresses=''`—. Es determinista, y por eso tienta, pero ata la sonda a la configuración de red y de autenticación de cada imagen: el `bind-address`, el `listen_addresses` y el reparto de permisos (`root@localhost` frente a `root@%`, las líneas de `pg_hba`). Si cualquiera de esas suposiciones se rompe —una versión nueva de la imagen, un `MARIADB_ROOT_HOST` distinto—, la espera **no acierta nunca** y el fallo deja de ser un flake ocasional para convertirse en un `PLAZO_DE_ARRANQUE` agotado en cada corrida: peor que el problema que se arregla.

Con A no hay suposiciones sobre la imagen, y el peor caso es benigno: si el servidor definitivo tarda de más, el acierto se retrasa una espera corta. La sonda sigue siendo la misma —nombra la base, `arnes.rs:212-230` y `:197-210`—, así que el arreglo del #170 para el tramo anterior del temporal se conserva. Ojo con la cita: ese «la sonda nombra la base» del #170 **no está en ningún spec** —`rg` no encuentra el escenario en `openspec/`; vive en las pruebas de `escenarios.rs` y en el `CHANGELOG`—, así que este change no lo da por especificado.

## D3 · El sondeo y la espera se inyectan, y `esperar_orden` no cambia para todos

El ayudante nuevo recibe el **sondeo** y la **espera** como parámetros —con `tokio::time` solo en el borde—, igual que el `reintentar` del #141 (`tests/e2e/arnes.rs:1408-1428`) recibe el «intento» como clausura. Así una prueba unitaria simula la secuencia entera —acierto durante el temporal, fallo después, acierto estable al final— sin contenedores y sin esperar la espera de producción, y la espera entre sondeos se puede marcar desde el test en vez de esperarla de verdad.

No se cambia `esperar_orden` para todo el mundo: SQLite (contenedor `alpine`, `escenarios.rs:322-345`) no tiene servidor temporal y su espera —`sqlite3 -version`— no debe pagar una espera extra ni ganar un modo de fallo que no le corresponde. El cambio va por el ayudante nuevo, que usan los dos arranques que sí tienen servidor temporal; `esperar_orden` queda intacto para lo demás.

Se conserva el contrato de error: agotado el plazo, se falla en voz alta nombrando contenedor, orden y plazo con el último motivo. Con un matiz que es un arreglo de borde: el plazo acota la espera del **primer** acierto, y la confirmación —la espera y el segundo sondeo— dispone de su propia holgura acotada por la espera entre sondeos (el límite interno del bucle es `plazo + espera`, no `plazo`). Así un servidor que aparece al final del plazo **se confirma** en vez de fallar por unos milisegundos, que es lo que haría un plazo contado como presupuesto de reloj puro.

## D4 · El alcance llega a PostgreSQL, y no por analogía

La sonda de PostgreSQL (`arnes.rs:197-210`) nombra la base igual que la de MariaDB, y su entrypoint oficial tiene el mismo mecanismo, verificado sobre la imagen: `docker_temp_server_start` arranca el servidor temporal con `-c listen_addresses=''`, `docker_setup_db` crea `POSTGRES_DB` en él y `docker_temp_server_stop` lo para antes del definitivo. El hueco es el mismo —la base existe en el temporal—, así que la carrera es la misma y el arreglo también: sería incoherente corregir un motor y dejar el otro con el flake a sabiendas.

Quedan fuera, con motivo: SQLite, que no tiene servidor (es un contenedor `alpine` con el cliente y un fichero), y el servidor S3, cuya espera (`esperar_el_s3`, `arnes.rs:870`) sondea el almacén y no un motor con arranque en dos fases.

## D5 · Lo que no cambia

Cero cambios en `src/`: el arreglo es del arnés, sin tocar producto, contrato HTTP, base de datos ni panel. No se tocan las constantes ni el bucle del #141 (`INTENTOS_DEL_ALTA`, `ESPERA_ENTRE_ALTA`, `reintentar`), que son del alta de repositorio y no tienen relación con el arranque de un motor. No se sube `PLAZO_DE_ARRANQUE` (`arnes.rs:147`), no se relaja ninguna aserción y no se retira cobertura: las pruebas de las sondas del #170 siguen tal cual. Sin dependencias nuevas (build offline y versiones pinadas).

## D6 · La espera entre sondeos vale 2 s, y la garantía es la que es

La espera entre sondeos (`ESPERA_ENTRE_SONDEOS`) vale **2 s**. No es el valor de la ventana del servidor temporal, sino una **holgura deliberada** sobre ella. La ventana es lo que tarda el entrypoint desde que le crea la base al temporal hasta que lo apaga; 5 corridas por motor en máquina ociosa:

| Motor | Ventanas medidas (ms) | **Máximo** |
|---|---|---|
| MariaDB (`mariadb:11`) | 31 · 26 · 25 · 28 · 25 | **31** |
| PostgreSQL (`postgres:18-alpine`) | 134 · 137 · 133 · 148 · 131 | **148** |

De dónde sale cada ventana (hitos de los logs del contenedor, `podman logs -t`):

- **MariaDB**: de `Creating database pruebas` / `Securing system users` a `Stopping temporary server` —28 ms en la corrida citada—; el definitivo anuncia `ready for connections` 1,07 s después. Contra el temporal el sondeo recibe en bucle `Access denied for user 'root'@'localhost'`, así que **la ventana se abre exactamente en `Securing system users`**.
- **PostgreSQL**: del `database system is ready to accept connections` del temporal a su `shutting down` —15.304 → 15.433—; el definitivo anuncia su `ready` en 15.554.

Cómo se midió, para que sea reproducible: un sondeo **dentro** del contenedor (un `podman exec` con un bucle a ~10 ms, porque el `podman exec` por muestra cuesta ~200 ms y se salta el hueco) y, sobre todo, los **hitos del entrypoint** de `podman logs -t`, que son deterministas. Los dos segundos son **~13× el máximo medido**: el margen cubre la carga —la suite levanta contenedores en paralelo— y se paga **una vez por arranque de contenedor** (la espera va entre los dos sondeos de la confirmación, no antes de cada sondeo).

**La garantía real**, la que el spec rebaja a propósito, es: «el temporal que se apaga **antes** de `W` no confirma el arranque». **El residuo no queda cerrado**: un temporal que sobreviviera **más** de `W` tras el primer acierto se confirmaría dos veces. Cerrarlo exigiría más de 2 s entre crear la base y apagar el temporal —**~13× el máximo medido**—, y la alternativa determinista —una señal exclusiva del definitivo— se descartó en D2 por frágil: atada al `bind-address`/`listen_addresses` de cada imagen.
