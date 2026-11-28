# FEATURE-055 — Detector MySQL/MariaDB para deduplicación por manifest

## Qué es

Cierra la Fase 2 del change `dedup-manifest` (plan `plans/PLAN-055.md`, issue #206). La Fase 1 (FEATURE-054) había dejado el manifest de cambios y la deduplicación funcionando solo para PostgreSQL. Esta feature añade el detector equivalente para MariaDB/MySQL y lo conecta con el mismo cable `motor → detector → archivado → retención`, sin tocar la biblioteca de copias.

El resultado: un target MariaDB con `db_name` informado produce un manifest estable; si el volcado siguiente no cambió, se crea una snapshot nueva sobre el mismo árbol con la etiqueta `kesto.change_manifest=<hash>` y no se suben bytes. Si cambió, se sube el volcado completo y se etiqueta con el hash nuevo. Si el detector falla, se archiva sin etiqueta y se hace una subida completa: la copia **no** aborta.

## Ficheros implicados

- `src/engine/extraccion.rs`: helpers compartidos de hash y de degradación, `MysqlChangeDetector` (`con_ejecutor`, `con_configuracion_y_clientes`, `desde_la_orden`) y `calcular_manifest_mysql_con`; el detector PostgreSQL queda intacto en su comportamiento.
- `src/engine/ejecucion.rs`: `detector_de_produccion` elige el detector según el motor de la orden; el aviso de degradación distingue el motor sin detector del detector imposible.
- `Dockerfile`: el runtime instala `postgresql-client` y `mariadb-client`, porque los clientes de detección corren en el contenedor de Kesto.
- `compose.yaml` y `docs/ENTORNO.md`: `MYSQL_HOST`/`MYSQL_PORT`, con la credencial solo en el entorno del `exec`.
- `src/rustic/**`: **sin cambios**. Se reutilizan `archivar_volcado`, el prefijo `kesto.change_manifest`, la snapshot «sin cambios» y la compensación de duplicados de la retención.
- Tests: dieciocho contratos de MySQL en `extraccion.rs` —incluidos los de la remediación post-auditoría— y el cable de orquestación con `DetectorFalso` en `ejecucion.rs`.
- `docs/FEATURE-054-dedup-manifest.md`: feature anterior; define el manifest, el tag, la API y el panel, que aquí no se modifican.

## Cómo funciona

1. `copiar_el_volcado` llama a `detector_de_produccion`. Para `pg_dump` con base simple se construye el detector PostgreSQL; para `mariadb-dump` o `mysqldump` con base simple se construye `MysqlChangeDetector` con los binarios del contenedor de Kesto (`mariadb-dump`/`mariadb`, o `mysqldump`/`mysql`). Un volcado global (`--all-databases`) o sin base no tiene una sola base que checksumar y degrada a subida completa, **con aviso** porque el motor sí prometía dedup; un target sin detector no avisa.
2. `MysqlChangeDetector::desde_la_orden` reutiliza la configuración de la orden de extracción: toma usuario, host, puerto y base del último argumento no bandera —nunca del nombre del binario— y copia `MYSQL_PWD` del entorno del `exec` al entorno de los hijos. `MYSQL_HOST`/`MYSQL_PORT`, leídos del proceso, se pasan como `-h`/`-P` y sustituyen a los de la orden si ésta los trajera. La credencial nunca entra en `argv` ni en el registro. Las banderas que no bloquean la base (`--single-transaction`, `--lock-tables=false`) se guardan **aparte** y van **solo en el volcado de estructura**: el cliente `mariadb`/`mysql` las rechazaría y la dedup no se activaría nunca.
3. El esquema sale de `<volcado> --no-data --skip-dump-date`, invocado por `argv`, sin shell y sin ficheros temporales; `stdout` y `stderr` se leen en hilos distintos con tope de 8 MiB y plazo de 30 s. `--skip-dump-date` evita que la fecha del volcado cambie el esquema en cada copia.
4. El listado se pide con `SELECT CONCAT(table_schema, CHAR(9), table_name) FROM information_schema.tables WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' ORDER BY table_name`. Así no entran vistas, el orden lo fija el servidor y el detector vuelve a ordenar y deduplicar antes de hashear. Una línea con más de un tabulador se rechaza antes de separar los campos.
5. Por cada tabla se ejecuta `CHECKSUM TABLE \`db\`.\`tabla\` EXTENDED;` con identificadores escapados (comillas invertidas duplicadas) y nombres controlados: vacíos, con caracteres de control, contrabarras o `|` se rechazan.
6. El manifest es SHA-256 en hexadecimal minúsculo sobre dominios separados por `\0`: `esquema\0<dump>\0datos\0<db>|<tabla>|<checksum>\n…`. La factorización de T-2.4 comparte con PostgreSQL la apertura del digest, el cierre hexadecimal y el manejo de errores (`manifiesto_o_degradacion`), sin alterar los bytes hasheados.
7. `archivar_volcado` compara el manifest con el de la última snapshot, aplica la deduplicación y escribe el tag `kesto.change_manifest=<hash>`. La API y el panel siguen leyendo el mismo campo `change_manifest`.
8. Fallo al arrancar, código de salida distinto de cero, timeout, salida que alcanza el tope, tabla no representable o checksum ausente/`NULL` → `None` + `warn`; la copia continúa y se archiva sin tag. El `stderr` pasa por la misma red de secretos que el motor antes de entrar en el aviso, así que una contraseña repetida por el cliente se sustituye por la marca de redacción.

## Variables y configuración

- `MYSQL_HOST`/`MYSQL_PORT` son **nuevas** en esta feature: no llevan prefijo, las lee `copiar_el_volcado` y `desde_la_orden` las convierte en `-h`/`-P` para los clientes que corren en el contenedor de Kesto. Sin valor, los clientes prueban el localhost y el detector degrada; la copia sigue funcionando. `MYSQL_PWD` sigue siendo la credencial del cliente MariaDB y viaja por el entorno del `exec`; `PGHOST`/`PGPORT` siguen siendo exclusivas del detector PostgreSQL. Ver `docs/ENTORNO.md`.
- La base se toma de `backup_targets.db_name` a través de la orden de volcado; el detector no la adivina del nombre del binario.
- El runtime de la imagen instala `mariadb-client` (y `postgresql-client`, que el detector de Fase 1 ya necesitaba). El contenedor de la base sigue trayendo `mariadb-dump` para el volcado; el detector usa el cliente del contenedor de Kesto. No hay que cambiar el panel.

## Cómo habilitar y desplegar

No hay interruptor ni migración: basta con desplegar la imagen nueva —con `mariadb-client`— y el motor nuevo, y exportar `MYSQL_HOST`/`MYSQL_PORT` en el `.env` si la base no está en el localhost del contenedor de Kesto; `compose.yaml` ya los pasa al servicio `kesto`. Un target MariaDB existente con `db_name` informado pasa a deduplicar por manifest en su siguiente copia. Un target con `db_name` nulo (`--all-databases`) conserva el comportamiento de siempre: subida completa sin manifest. No hay cambios en API, panel, esquema SQLite ni en el cifrado en reposo, así que no hay rotación de claves ni pasos especiales de arranque.

**Tag de una sola vez:** `--skip-dump-date` y el listado por `information_schema` cambian los bytes hasheados respecto a la primera entrega del detector. Los tags `kesto.change_manifest=<hash>` generados antes de este cambio dejan de ser estables una sola vez: la primera copia tras el despliegue sube los bytes y escribe el hash nuevo; a partir de ahí la deduplicación vuelve a ser estable. No hace falta borrar instantáneas ni purgar tags.

## Decisiones

- **`EXTENDED` y nunca `QUICK`**: `CHECKSUM TABLE ... QUICK` puede devolver una cifra guardada y no detectar cambios reales; `EXTENDED` calcula el checksum de los datos. Es la diferencia entre deduplicar de verdad y omitir una copia con datos nuevos.
- **`--skip-dump-date`**: el volcado de estructura de MariaDB lleva por defecto un comentario con la fecha de ejecución. Sin esta bandera el esquema cambiaría en cada copia y la dedup nunca coincidiría. El coste es una única rotura de estabilidad de los tags ya escritos, documentada arriba.
- **`information_schema` en vez de `SHOW TABLES`**: `SHOW TABLES` mezcla tablas base y vistas, y no garantiza orden. `table_type = 'BASE TABLE'` evita checksumar vistas y `ORDER BY table_name` hace que el manifest no dependa del orden del servidor.
- **Sin binlog position**: queda fuera del alcance del plan. El detector es una lectura puntual de estructura y datos, encaja con el modelo de streaming sin disco y no exige permisos de replicación ni estado adicional por repositorio.
- **Clientes en el runtime, no `exec` en el target**: el detector sigue el patrón de PostgreSQL —los clientes corren en el proceso de Kesto y llegan a la base por la red con `MYSQL_HOST`/`MYSQL_PORT`— en vez de añadir un `exec` por target. El volcado usa `mariadb-dump` dentro del contenedor de la base; el detector usa el cliente del runtime.
- **Reutilización de la retención**: el tag y el algoritmo de agrupación son los de Fase 1; cambiarlos rompería las snapshots ya etiquetadas. Por eso `src/rustic/**` no se toca y el manifest solo se calcula antes de archivar.
- **Dominios separados y tablas ordenadas**: evitan colisiones entre esquema y datos y hacen que el manifest no dependa del orden de las filas devueltas por el servidor.
- **Un solo worker por repositorio**: el detector hereda el plazo de 30 s y el tope de 8 MiB del ejecutor existente; un cliente colgado degrada a subida completa en vez de bloquear la cola.

## Verificación

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
- `cargo test --locked --workspace`: exit 0; 888 tests del binario principal y 64 más entre los otros objetivos del workspace (44 ignorados), 0 fallos.
- `CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 just check-all`: exit 0; incluye `cargo deny`, las pruebas de `scripts/` y los cuatro pasos del panel.
- Tests específicos: dieciocho contratos MySQL, el cable de orquestación MySQL y los siete contratos PostgreSQL por nombre exacto, en verde.

## Follow-ups

- P-055.5: auditoría de `@rust-reviewer` sobre el detector, la integración y los tests.
- P-055.6: marcar T-4.1, archivar el change, renombrar `PLAN-055.md` a `PLAN-055-COMPLETED.md`, PR a `development` y cierre del issue #206.
- Los errores de SQL dentro de una sección de restauración PostgreSQL siguen sin detectarse (#175); es un cabo suelto anterior, ajeno a esta feature.
- Medir el coste real de `CHECKSUM TABLE ... EXTENDED` en bases grandes y valorar un muestreo opcional, sin cambiar el contrato del manifest.
