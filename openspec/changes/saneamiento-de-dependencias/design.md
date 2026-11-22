# Design — saneamiento de dependencias y vigilancia de la cadena de suministro

## D1 · `sqlx 0.9.1` ahora, y con `sqlite-bundled` en vez de `sqlite`

El `Cargo.toml` documenta por qué se eligió `0.8.6` y no `0.9.0`: en `0.9.0` la feature `sqlite` expande a `sqlite-bundled + sqlite-deserialize + sqlite-load-extension + sqlite-unlock-notify`, abre `libsqlite3-sys` a `>=0.30.1, <0.38.0` —que resuelve a `0.37.0`, **por debajo del `0.39.0` que cierra el CVE**—, sube `flume` a `0.12.0` y exige MSRV `1.94.0`. El razonamiento era correcto para `0.9.0`: el salto no cerraba el advisory que lo motivaba. **En `0.9.1` el techo sube a `<0.41.0`**, así que el CVE pasa a ser cerrable, y el argumento que decidió la versión deja de sostenerse. Lo que sigue en pie es el MSRV —que se **declara** en lugar de dejarlo en un comentario— y la expansión de features, que se evita declarando `sqlite-bundled`: el equivalente exacto de lo que hacía la feature `sqlite` de `0.8.6`, sin encender la carga de extensiones, el aviso de desbloqueo ni la deserialización de tipos, que Kesto no usa.

Alternativas descartadas:

- **Quedarse en `0.8.6` y declarar el CVE como excepción.** Rechazada: de los diez avisos, este es el único que afecta a código que el producto **siempre** ejecuta —SQLite **es** el almacén—, y declararlo dejaría la puerta abierta sin causa externa que lo justifique (a diferencia de `rsa` o `suppaftp`, aquí no hay tercero que retenga la versión).
- **`[patch]` de `libsqlite3-sys` a `0.39` sin mover `sqlx`.** Rechazada: `sqlx-sqlite 0.8.x` fija `^0.30.1`, así que el parche queda fuera del rango declarado y la resolución deja de ser reproducible; y el código de `sqlx-sqlite 0.8` no tiene por qué compilar contra la API de la generación `0.39`.
- **Declarar la feature `sqlite` expandida.** Rechazada: enciende tres capacidades que el producto no usa, y el árbol mínimo es una regla explícita del repositorio.

El riesgo del salto es de **compilación**, no de datos: el algoritmo del checksum (`Sha384` del SQL) es el mismo en las dos versiones, la tabla `_sqlx_migrations` conserva su nombre y su forma por defecto —el renombrado es opt-in por `sqlx.toml`—, y los nombres de `migrations/` (`0001_init.sql` …) ya cumplen la validación nueva. Por eso el cambio de esquema es **ninguno** y no se toca `migrations/`.

## D2 · Las excepciones se declaran con motivo, no se silencian

`cargo deny` admite un `ignore` sin motivo, y también apagar la comprobación entera. Se descartan las dos: la lista de excepciones es el **único** sitio donde un lector futuro puede distinguir «este aviso no nos alcanza» de «este aviso lo silenciamos y ya nadie se acordó de por qué». Son **siete** excepciones, y hay que separar dos tipos, porque el motivo que las justifica no es el mismo:

- **Tres por camino no ejecutado o versión retenida por un tercero** —avisos no alcanzables—: `rsa` (el ataque es de descifrado con la clave privada y `reqsign-core` fija `^0.9.2`), `suppaftp` (el transporte FTP nunca se configura) y `crypto-bigint` (las rutas ECDSA no se usan). Cada entrada nombra **quién retiene la versión** o **qué camino no se ejecuta**, de modo que se puede refutar con un `cargo tree -i` y retirar en cuanto su causa desaparezca.
- **Cuatro por *unmaintained* sin versión que lo cierre** —aquí no hay versión a la que subir—: `async-std` y `pin-utils` caen con la misma cadena FTP sin configurar; `chrono` y `chrono-tz` son dependencia **real** del producto (las fechas y el `tokio-cron-scheduler`) y su salida es una **migración propia a `time`/`jiff`**, fuera del alcance de este change. En estos, el motivo nombra la migración que haría falta y por qué queda fuera.

Cada excepción se puede refutar con un `cargo tree -i` y se puede retirar en cuanto su causa desaparezca.

**Coste aceptado**: una excepción puede quedarse obsoleta —el tercero publica la versión que la cierra y el CI no lo recuerda—, porque `cargo deny` solo comprueba que el aviso siga estando. Lo mitiga Dependabot: la actualización llega como PR y el `ignore` se retira en el mismo PR que sube la dependencia.

## D3 · `multiple-versions` avisa; `wildcards` deniega

Hoy hay **126 grupos** de versiones duplicadas, y todos vienen de árboles de terceros: `rustic_core`, `opendal`, `suppaftp`, `wiremock`. Denegar las duplicidades obligaría a un fichero de `skip` con 126 entradas —una lista que nadie lee, que caduca sola y que convierte cualquier bump en una pelea de configuración—. Se queda en `warn`: el aviso sale en el registro del trabajo y se puede leer cuando interesa.

Los comodines son otra cosa: una versión `*` en un `Cargo.toml` es un defecto real —hace que dos máquinas resuelvan árboles distintos— y hoy **no hay ninguno**, así que denegarlos no cuesta nada y cierra la puerta a que aparezca.

## D4 · La puerta es `cargo deny`, no `cargo audit`

`cargo audit` solo mira advisories; `cargo deny` mira cuatro familias —advisories, licencias, prohibiciones y fuentes— con la configuración **versionada en el repositorio**, que es lo que permite revisar las excepciones en el PR. Las licencias y las fuentes no son un adorno: son la comprobación que atrapa a la dependencia nueva que entra con una licencia inesperada o desde un registro ajeno, que es exactamente el tipo de cambio que nadie revisa a mano en un PR de bumps. Se descarta correr las dos: duplicaría el tiempo del trabajo y las dos leen la misma base de avisos, con lo que el segundo par de ojos no añade señal.

## D5 · Dependabot, no Renovate

Dependabot es nativo de GitHub: no exige instalar ninguna aplicación, cubre los tres ecosistemas del repositorio —`cargo` (una entrada en la raíz cubre el workspace entero, `kesto-mcp` incluido), `github-actions` y el `npm` del panel— y su configuración vive en el árbol, junto al resto de la automatización. Renovate agrupa mejor y admite ventanas de fusión más finas, pero necesita la aplicación instalada en el repositorio: una dependencia externa para un proyecto que mantiene sus flujos en ficheros. La agrupación que aquí hace falta —separar los parches de los saltos mayores— se consigue con los `groups` de Dependabot.

## D6 · La prueba del cableado es estructural, y tiene un límite declarado

`scripts/dependencias.test.sh` lee `deny.toml`, `ci.yml` y `.github/dependabot.yml` **como texto** y comprueba la forma: que el `deny.toml` tenga las cuatro secciones y una allowlist no vacía, que cada `ignore` lleve motivo, que el CI ejecute `cargo deny` con `--locked` y que sus pasos de clippy y test lleven `--workspace`, y que Dependabot declare los tres ecosistemas. Sigue el precedente de `scripts/release-image-workflow.test.sh`, que valida la estructura del flujo de publicación.

Lo que **no** prueba, y conviene decirlo: que el trabajo llegue a ejecutarse —eso lo demuestra el CI— ni que la allowlist de licencias esté completa —eso lo demuestra `cargo deny check` corriendo de verdad, en local con `just deny` y en el trabajo—. Lo que atrapa es el modo de fallo que motivó SEC-11: que alguien borre el cableado y la vigilancia desaparezca **sin que nada lo delate**.

## D7 · El checksum de migración se fija en una prueba de caracterización

La afirmación «el salto no invalida las bases ya desplegadas» se apoya en una propiedad de un tercero —que `sqlx` siga calculando el checksum igual—, y una propiedad de un tercero merece una prueba, no un comentario. La prueba fija el checksum hexadecimal de una migración conocida: si una versión futura cambia el algoritmo, la prueba se pone roja **antes** de que un operador vea un `VersionMismatch` en producción. Se escribe antes del salto y está verde a los dos lados: es un guardián, no un rojo.

## D8 · La receta local de las pruebas de scripts enmascaraba el fallo

La receta `scripts-test` corre la batería en un bucle `for` sin `set -e`, y el código de salida de un bucle es el de su **último** comando: un script en rojo quedaba tapado por el verde de los que vinieran después por orden alfabético, de modo que `just check-all` —el análogo local del CI— podía devolver éxito con una prueba en rojo. Lo destapó la propia fase RED de este change: `dependencias.test.sh` fallaba y la receta salía con `0`. El CI **no** lo sufría, porque su paso de pruebas corre con `bash -e` —el `-e` que GitHub Actions pone por defecto— y ahí el primer fallo corta el paso. La lección es la que recoge el requisito nuevo del delta: **una puerta que devuelve éxito con una prueba en rojo es un adorno**. La receta lleva `set -e` y la comprobación **D** de `scripts/dependencias.test.sh` existe para que nadie lo quite sin que nada lo delate.

## D9 · El grafo de `cargo deny` no incluye `kesto-mcp` (cerrado por D11)

`cargo deny` construye su grafo desde los **miembros por defecto** del workspace (`cargo metadata` → `workspace_default_members`), y este repositorio declara el paquete raíz `kesto` y el miembro `kesto-mcp` **sin** `default-members`, así que el defecto es solo el paquete raíz: `cargo deny --locked check` **no ve** `kesto-mcp` ni, por tanto, `rmcp` —justo el crate cuyos tres advisories motivan la tarea 3.2—. Verificado en la fase GREEN: `cargo metadata --locked | jq .workspace_default_members` devuelve solo `kesto`, y `cargo deny --locked --manifest-path kesto-mcp/Cargo.toml check advisories` sí reporta RUSTSEC-2026-0457, -0461 y -0462.

Quedó **abierto** entonces, porque contradecía el requisito del delta («el CI SHALL ejecutar `cargo deny check` sobre el workspace»): las dos salidas eran declarar `default-members = ["kesto", "kesto-mcp"]` en el `[workspace]` del `Cargo.toml` —una línea, y es donde el resto de las herramientas ya mira el workspace entero— o correr `cargo deny` una segunda vez con `--manifest-path kesto-mcp/Cargo.toml`. Ninguna cabía en aquella fase: la primera toca `Cargo.toml` (tarea 4) y la segunda, el paso del CI. **Lo cerró D11**, y por una tercera vía que entonces no se consideró: `--workspace` en la propia invocación.

## D10 · La caracterización del planificador es de la versión de SQLite

El salto arrastra el SQLite empaquetado de **3.46.0** (`libsqlite3-sys 0.30.1`) a **3.54.1** (`0.40.0`), y con él se movió el planificador. Medido: sin estadísticas, la consulta de una instantánea de la `0004` sigue entrando en `e` por `idx_job_executions_snapshot_id`, y el filtro suelto también; lo que cambia es **qué puerta** elige el planificador cuando las dos están disponibles **y** hay estadísticas —bajo `ANALYZE` el join arranca por `idx_backup_jobs_repository_id` y alcanza `e` por `idx_job_executions_job_id`, donde 3.46.0 nombraba el índice nuevo—.

No es una regresión, y por eso no se toca la migración: las dos puertas son **indexadas**, la consulta no pierde ningún acceso indexado y `t` se recorre igual que antes (`SCAN t`, que ya estaba en la medición previa). Lo que estaba mal era la aserción, que fijaba **una** puerta concreta donde la propiedad que importa —y que no depende de la versión— es que el índice nuevo sea la puerta **cuando es la única**. Por eso el bloque «con estadísticas» pasa a afirmar lo que es cierto hoy: que `e` se alcanza por un índice —la aserción es que el plan **no** contenga `SCAN e`, el mismo idioma que ya usaba el filtro suelto, y un `SCAN e` bajo estadísticas sí sería el hallazgo que la prueba tiene que cazar— y que la puerta que el planificador elige es la que la medición nombra. Las aserciones del bloque sin estadísticas y las del filtro suelto **no se tocan**: son las que siguen demostrando que el índice de la `0004` es la puerta cuando es la única. Y el bloque sigue **mordiendo**: con un `DROP INDEX idx_job_executions_snapshot_id` después del migrador, la aserción del bloque sin estadísticas se pone roja; sin eso, una caracterización que no se inmuta cuando le quitas lo que caracteriza es un adorno (la lección de D8).

El comentario `Medido:` del bloque reescrito nombra las **dos** versiones de SQLite, para que el siguiente que lo lea sepa que la medida es de una versión concreta y qué hay que rehacer si vuelve a moverse: la medición, no la migración. Lo que esto **no** es: no es un cambio de esquema ni de datos, no se toca `migrations/` —el nombre y la forma por defecto de `_sqlx_migrations` no cambian y el checksum sigue verde, D7— y el salto de SQLite no altera el formato del fichero de la base.

## D11 · La puerta no veía `kesto-mcp` sin `--workspace`

Cierra el cabo que D9 dejó abierto, y lo destapó la revisión del bloque, no una prueba en rojo: `cargo deny --locked check` salía en verde **sin haber mirado nunca `rmcp`**, que es justo el crate cuyos tres avisos motivan la tarea 3.2.

La causa es de forma del manifiesto, y la dice el propio `cargo deny --help`: *«if you specify a manifest path that is a member of a workspace, that crate will be the sole root of the crate graph»*, y `--workspace` *«overrides that behavior to include all workspace members»*. El manifiesto raíz de este repositorio es un **paquete** —`[package] name = "kesto"` junto a `[workspace] members = ["kesto-mcp"]`—, no un manifiesto **virtual**, así que `kesto` es la **única raíz** del grafo; y `kesto-mcp` **no es dependencia de `kesto`**, de modo que queda fuera. Con el flag entran los tres: `kesto-mcp@0.1.0`, `rmcp@3.11.0` y `rmcp-macros@3.11.0`. Medido, no supuesto: `cargo deny --locked list | rg rmcp` daba **cero** apariciones, y `cargo deny --workspace --locked list | rg 'rmcp|kesto-mcp'` da las tres; y `cargo deny --workspace --locked check` termina con `advisories ok, bans ok, licenses ok, sources ok`, así que cubrir el workspace de verdad no deja nada rojo y no hay excusa para no hacerlo.

**La lección es la de D8**: una comprobación que no mira lo que dice mirar devuelve verde sin haber comprobado nada, y aquí el modo de fallo era peor, porque la puerta **parecía** puerta —el trabajo del CI existía, corría y salía en verde— y no lo era. La salida no es declarar `default-members` ni correr `cargo deny` dos veces, sino añadir `--workspace` a la invocación: una palabra, y es exactamente la semántica que el trabajo promete. La prueba estructural lo vigila desde ahora en los dos sitios: la comprobación **B** de `scripts/dependencias.test.sh` exige `--workspace` **y** `--locked` en la línea del CI, y la **E** exige `--workspace` en la receta `deny` del `.justfile`, que es el espejo local y podía quedarse corta por el mismo motivo. El escenario nuevo del delta fija la propiedad que el flag compra: un aviso sobre un crate que **no es dependencia del crate raíz** hace fallar el trabajo.
