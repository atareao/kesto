# Entorno de desarrollo y variables de entorno

> Extracto **literal** del `AGENTS.md` de Kesto (poda v2, 2026-10-31). Tabla completa de variables, justificación de cada regla de arranque y versiones del entorno verificado. El `AGENTS.md` conserva la regla imperativa de cada viñeta.

## Variables de entorno

| Variable | Obligatoria | Valor por defecto | Para qué |
|---|---|---|---|
| `RUST_LOG` | no | `info` | Filtro de `tracing-subscriber`. Nombre del §8, sin prefijo |
| `BIND_ADDRESS` | no | `127.0.0.1:8080` | Dirección de escucha. Nombre del §8, sin prefijo. El defecto es de **loopback**, y no el `0.0.0.0:8080` del §8: escuchar en todas las interfaces es una decisión explícita del operador —el §8 la toma en su `compose`, con proveedor de identidad— y en modo de desarrollo es obligatorio que sea de loopback |
| `DATABASE_URL` | no | `sqlite:///app/data/kesto.db` | DSN de SQLite. Nombre del §8, sin prefijo. **Ya se usa**: el arranque abre la base ahí y aplica las migraciones embebidas. Ojo con el defecto: `sqlite:///app/data/kesto.db` es el del volumen del `compose` del §8, así que en una máquina sin `/app/data` **el arranque falla nombrando la ruta** —Kesto no crea el directorio a propósito: crearlo escondería un volumen mal montado—. Para desarrollo, exporta el tuyo (`DATABASE_URL=sqlite:///tmp/kesto/kesto.db`, con el directorio ya creado) |
| `DOCKER_HOST` | no | `unix:///var/run/docker.sock` | Socket con el que el motor habla con Docker. **Sin prefijo `KESTO_`**: es el nombre que ya usa la biblioteca de Docker, y respetarlo deja que un `compose` o un `DOCKER_HOST` del entorno apunten al socket de Podman en desarrollo sin editar la configuración. **Ya se usa**: el motor construye su cliente con esta ruta, y su ausencia, su vacío y su blanco caen al defecto —a diferencia de `BIND_ADDRESS` y `RUST_LOG`, que matan el arranque— |
| `PGHOST` | no | sin valor (libpq decide) | Host al que el **detector PostgreSQL** del dedup llama con `pg_dump`/`psql` desde el proceso de la API (`src/engine/ejecucion.rs`). Nombre estándar de libpq, **sin prefijo** `KESTO_`: es el que esperan los clientes y el que el operador ya exporta para ellos. Sin él los clientes prueban el socket local del contenedor, no alcanzan la base de datos del target y el detector degrada a subida completa —la copia sigue funcionando, pero sin dedup—. En `compose.yaml` se pasa al servicio `kesto`, que es donde corre el detector; el valor lo pone el despliegue (en el ejemplo, el nombre del servicio de la base) |
| `PGPORT` | no | sin valor (libpq decide) | Puerto del mismo detector. Igual que `PGHOST`: nombre estándar de libpq sin prefijo, ausente cae al defecto de libpq y si la base no está ahí el detector degrada a subida completa. Se pasa al servicio `kesto` en `compose.yaml`. La contraseña **no** se toma de aquí: `PGPASSWORD` se inyecta por ejecución desde el entorno del `exec` del contenedor fuente (`PostgresChangeDetector::desde_la_orden`) |
| `MYSQL_HOST` | no | sin valor | Host al que el **detector MySQL/MariaDB** del dedup llama con `mariadb-dump`/`mariadb` (o `mysqldump`/`mysql`) desde el proceso de la API (`src/engine/ejecucion.rs`). Lo lee `copiar_el_volcado` junto a `PGHOST` y `MysqlChangeDetector::desde_la_orden` lo pasa como `-h`. Sin valor los clientes prueban el localhost del contenedor y el detector degrada a subida completa —la copia sigue funcionando, pero sin dedup—. Se pasa al servicio `kesto` en `compose.yaml`; **no** lleva prefijo `KESTO_`, porque es el nombre que esperan los clientes |
| `MYSQL_PORT` | no | sin valor (defecto del cliente) | Puerto del mismo detector; `desde_la_orden` lo pasa como `-P`. Igual que `MYSQL_HOST`: ausente cae al defecto del cliente y si la base no está ahí el detector degrada. La contraseña **no** se toma de aquí: `MYSQL_PWD` se inyecta por ejecución desde el entorno del `exec` del contenedor fuente |
| `KESTO_MOTOR_SOCKET` | no | `/run/kesto/motor.sock` | Ruta del socket IPC que **leen tanto la API como el motor**. La API lo usa para entregar órdenes al proceso privilegiado; el motor lo enlaza y verifica al par por UID. Ambos proceso tienen que ver el mismo fichero: en el `compose` del §8 vive en el volumen compartido `kesto_ipc`. Vacío o en blanco cae al defecto; la API **nunca** lee `DOCKER_HOST` |
| `KESTO_API_UID` | **sí, en `kesto-motor`** | — | UID numérico del proceso de la API, el único que el motor admite como par IPC. La API no lo lee: lo fija el despliegue (en el `compose`, `10001`, el mismo UID con que corre `kesto`). Ausente es error de arranque del motor —no hay modo degradado sin verificación— y presente pero no numérico también |
| `KESTO_HSTS_MAX_AGE` | no | desactivada | Segundos de la cabecera `Strict-Transport-Security` (HSTS). Ausente → sin cabecera (en el despliegue estándar la HSTS la pone el proxy que termina el TLS). Con un entero ≥ 0, **toda** respuesta lleva `Strict-Transport-Security: max-age=<n>` (incluidos los `404` y `405`). Presente pero vacía o no numérica → **error de arranque** |
| `KESTO_BACKUP_ROOT` | no | `/backup_sources` | Raíz bajo la que ha de quedar el `host_path` de un target `directory`: se canonicaliza —resolviendo los enlaces simbólicos de **todos** los componentes— y se rechaza lo que escape de ella, comparando **por componentes** y no por prefijo de cadena. La leen **la API y el motor** con el mismo valor: el motor repite el confinamiento antes del `exec`, y los dos servicios del `compose` montan las mismas fuentes bajo esta raíz. Debe ser una ruta **absoluta**; presente pero relativa o vacía → **error de arranque** que nombra la variable (como `RUST_LOG=""` y `BIND_ADDRESS=""`). Si la raíz existe se guarda **canonicalizada**, y textual si aún no existe |
| `KESTO_MASTER_KEY` | **sí, siempre** | — | Clave maestra del cifrado en reposo: de ella se derivan las claves que cifran los tres secretos del §3 (ver *Cifrado en reposo*). **Mínimo 32 caracteres**; más corta o vacía **impide el arranque** |
| `KESTO_SECRET` | **sí, si el modo es OIDC** | — | Clave de firma del JWT de sesión (`HS256`). Sin ella en modo OIDC el arranque falla nombrándola. **Mínimo 32 caracteres**; más corta o vacía **impide el arranque** |
| `KESTO_ALLOW_INSECURE` | **sí, si el modo es el de desarrollo** | — | Permiso explícito del modo sin autenticación. `1` permite; `0` o ausente lo niega y **mata el arranque**; cualquier otro valor (vacío incluido) es error de arranque. Con el permiso, la escucha tiene que ser de **loopback**. En modo OIDC ni se consulta |
| `KESTO_ALLOW_INSECURE_ENDPOINT` | no | negado | Permiso del borde HTTP para admitir un `endpoint` de repositorio inseguro. `1` concede; `0` o ausente lo niega; cualquier otro valor —vacío incluido— es **error de arranque** que nombra la variable. Con el permiso admite `http` **y** un destino en la propia máquina (loopback o nombre `localhost`/`*.localhost`); **nunca** rebaja el enlace local, la dirección de metadatos, la dirección no especificada ni las credenciales embebidas. Se lee **siempre**, también en modo OIDC |
| `OIDC_ISSUER_URL` | sí, si el modo es OIDC | — | Emisor del proveedor |
| `OIDC_CLIENT_ID` | sí, si el modo es OIDC | — | Identificador del cliente |
| `OIDC_CLIENT_SECRET` | sí, si el modo es OIDC | — | Secreto del cliente |
| `OIDC_REDIRECT_URL` | **sí, si el modo es OIDC** | — | URL de retorno del callback. Obligatoria y **`https`** cuando el modo es OIDC: ausente, o presente con un valor que no sea `https`, **mata el arranque** nombrando la variable —la cookie de sesión se emite siempre con `Secure`, y un retorno en claro haría que el navegador no la devolviera nunca—. Configurada **sola**, sin las otras tres, se ignora en silencio |

- Los nombres que se leen **tal cual**, sin prefijo `KESTO_`, son los tres del §8 (`RUST_LOG`, `BIND_ADDRESS`, `DATABASE_URL`), `DOCKER_HOST` —este último no lo trae el §8: es el nombre con el que la biblioteca de Docker, y el propio entorno, designa el socket—, `PGHOST`/`PGPORT` —nombres estándar de libpq, que los clientes `pg_dump`/`psql` del detector esperan tal cual— y `MYSQL_HOST`/`MYSQL_PORT` —los pasa el detector a `mariadb-dump`/`mariadb` como `-h`/`-P`—. Estos cuatro últimos son opcionales y sin valor dejan que cada cliente elija el suyo; el resto lleva prefijo `KESTO_`, salvo `OIDC_*`, que conserva el nombre que ya usa shuul.
- Las tres variables `OIDC_*` que gobiernan el modo (`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID` y `OIDC_CLIENT_SECRET`) son **un todo o nada**: las tres presentes encienden la autenticación; ninguna presente la apaga y lo anuncia con un aviso; unas sí y otras no es **error de arranque**, no un modo degradado. Degradar a «apagada» dejaría el panel abierto sin que nadie se entere.
- `KESTO_MASTER_KEY` es obligatoria **siempre**: es la raíz de la que se derivan las claves de cifrado en reposo de los tres secretos del §3.
- `KESTO_SECRET` es obligatoria **en modo OIDC** —es la clave con la que se firma la sesión— y no hace falta en modo `Disabled`. **Rotarla invalida todas las sesiones vivas**: no hay tabla de sesiones ni estado en el servidor, cada petición se verifica por firma contra la clave que tenga el proceso en memoria.
- Una credencial —`KESTO_MASTER_KEY` o, en modo OIDC, `KESTO_SECRET`— de menos de **32 caracteres**, **vacía incluida**, **impide el arranque** nombrando la variable y el mínimo. Antes solo avisaba: con una clave maestra corta la del cifrado en reposo es forzable, y con un `KESTO_SECRET` corto la sesión es forjable, así que el aviso dejaba el sistema en producción con los secretos al alcance de quien leyera el fichero.
- El modo de desarrollo —sin ninguna `OIDC_*`— exige el permiso explícito `KESTO_ALLOW_INSECURE=1` **y** una dirección de escucha de loopback: sin el permiso el proceso se niega a arrancar en abierto, y con él `BIND_ADDRESS=0.0.0.0:8080` sigue siendo error de arranque. El permiso no se consulta en modo OIDC.
- `BIND_ADDRESS` sin puerto es error de arranque, y **el valor vacío no cae al defecto**: `BIND_ADDRESS=` mata el arranque. Si usas `compose` con interpolación de una variable que no definiste, acuérdate de esto.
- Lo mismo vale para `RUST_LOG`: presente pero vacía es **error de arranque**, no «sin registro». Si dejas `RUST_LOG=` en un `compose` con la variable sin definir, el proceso no arranca —que es mucho mejor que un servicio ciego—. Quitarla del todo sí cae al valor por defecto, `info`. Los espacios sobrantes se recortan: `RUST_LOG=" info "` funciona igual que `RUST_LOG=info`.
- Un `OIDC_REDIRECT_URL` configurado **solo**, sin las otras tres, se ignora en silencio: los tres desenlaces se deciden sobre las tres variables de la autenticación. Quien configure solo el retorno creerá que encendió algo.
- `KESTO_BACKUP_ROOT` es la raíz de las copias de **directorio**, y su defecto —`/backup_sources`— es exactamente donde el `compose` del §8 monta los directorios a respaldar **en los dos servicios**: `kesto` valida la ruta y `kesto-motor` la vuelve a confinar antes de ejecutar. Si cambias la raíz, declárala en ambos y monta las fuentes en el mismo sitio en los dos; si no, el motor rechazará rutas que la API aceptó. Presente pero relativa o vacía **mata el arranque**, en la línea de `RUST_LOG=` y `BIND_ADDRESS=`: un valor presente pero inutilizable no se degrada en silencio. La frontera que gobierna va en el *Aviso de actualización* de abajo.
- `KESTO_MOTOR_SOCKET` y `KESTO_API_UID` son el canal y la credencial del par entre los dos procesos: la API solo conoce el socket IPC —nunca `DOCKER_HOST`— y el motor solo abre el socket del daemon. El `compose` declara la misma ruta en ambos servicios sobre `kesto_ipc` y fija `KESTO_API_UID` al UID con que corre `kesto`; sin UID, o con un UID no numérico, el motor no arranca, porque no tiene forma de identificar a su par. En modo `Disabled` la gestión de la API queda abierta, pero ese modo no se publica: exige permiso explícito y loopback.
- ⚠️ El `compose` del §8 y el `compose.yaml` del repositorio **exigen** `KESTO_MASTER_KEY`, las cuatro `OIDC_*` y `KESTO_SECRET` con la forma `${VAR:?}`: un `.env` al que le falte cualquiera de ellas **no levanta el servicio, sin crear el contenedor**. Es lo que impide que un despliegue que pierde sus `OIDC_*` arranque `Disabled` —sin capa de autenticación y con todo `/api/v1/*` abierto en el 8080—: la plantilla no puede degradar a ese modo, que exige permiso explícito y loopback, así que `BIND_ADDRESS=0.0.0.0:8080` solo es legítimo con proveedor.

> ⚠️ **Nota de actualización (endurecimiento del arranque).** Un despliegue que ya esté en marcha con una `KESTO_MASTER_KEY` —o, en modo OIDC, un `KESTO_SECRET`— de menos de 32 caracteres **dejará de arrancar** al actualizarse: la variable pasa de aviso a puerta de arranque. La reparación es poner una credencial de 32 caracteres o más. Ojo con cuál se toca: **rotar la clave maestra deja ilegibles los secretos ya guardados** (los tres del §3), así que no es un cambio de variable sino una operación con plan —hay que volver a dar de alta esos secretos—; rotar `KESTO_SECRET`, en cambio, solo invalida las sesiones vivas.

> ⚠️ **Aviso de actualización (confinamiento de la frontera «operador → motor», issues #160, #161 y #162).** Tres campos de un target dejan de aceptar lo que aceptaban: la frontera pasa de comprobar solo su **forma** a comprobar su **alcance**. Son **cambios incompatibles** y **no se migran las filas**: el motor rechaza la fila **al usarla**, nombrando el campo, y la copia o la restauración deja de hacerse en vez de hacerse mal. Lo que deja de funcionar y su reparación:
>
> | Qué deja de funcionar | Reparación |
> |---|---|
> | Target `directory` cuyo `host_path` queda **fuera** de `KESTO_BACKUP_ROOT` (por defecto `/backup_sources`) | **Mover** el directorio bajo la raíz, o **declarar la raíz** que lo contenga |
> | Target de base cuyo contenedor **no lleva** la etiqueta `io.kesto.backup-source=true` | **Recrear el contenedor** con `--label io.kesto.backup-source=true` |
> | Target `sqlite` cuyo `db_path` **no es su forma canónica** (vive bajo un directorio enlazado, o la cadena lleva `..`) | **Declarar la ruta canónica** |
>
> Además, un contenedor del que se copia un `sqlite` ha de traer el binario **`readlink`** —busybox y coreutils lo traen— además de `sqlite3`: sin él la comprobación de la ruta falla en voz alta, con el error del contenedor, en vez de silenciarse.


## Servidor MCP (`kesto-mcp`)

> Variables del **binario `kesto-mcp`**, que es un proceso aparte del servidor Kesto: no las mezcles con las de arriba. Todas se leen al arrancar y ninguna se escribe en el código ni en un fichero versionado. Detalle en `docs/FEATURE-030-mcp.md`.

| Variable | Obligatoria | Valor por defecto | Para qué |
|---|---|---|---|
| `KESTO_URL` | **sí** | — | URL base de la API de Kesto. Ausente o vacía → **error de arranque** que nombra la variable. Se recorta y pierde la barra final |
| `KESTO_API_TOKEN` | no | — | Token por defecto cuando la petición no trae `Authorization: Bearer`. Un valor vacío o en blanco se trata como **ausente**, no como error |
| `KESTO_MCP_BIND_ADDRESS` | no | `127.0.0.1:8931` | Dirección del transporte HTTP (solo con `--transport http`). El defecto es bucle local a propósito |
| `KESTO_MCP_ALLOWED_HOSTS` | no | los del SDK (bucle local) | Hosts admitidos por HTTP, separados por comas (solo con `--transport http`) |
| `RUST_LOG` | no | `info` | Nivel de registro (va a **stderr**). A diferencia del servidor Kesto, un `RUST_LOG` ilegible **no** impide arrancar: cae al nivel por defecto |

- El transporte se elige con argumentos, no con variables: `--transport <stdio|http>` (defecto `stdio`) y `--bind <dirección>` —que **sustituye** a `KESTO_MCP_BIND_ADDRESS` y **solo** vale con `--transport http`—.
- El token nunca aparece en el registro, en los errores ni en la salida del servidor. En `stdio` no hay cabecera, así que manda `KESTO_API_TOKEN`; en HTTP, la cabecera `Authorization: Bearer` de la petición manda sobre el entorno.

## Entorno de desarrollo (verificado en esta máquina)

| Herramienta | Estado |
|---|---|
| Rust / Cargo | 1.97 · `edition = "2024"` · MSRV declarado `1.94` en `Cargo.toml` (`rust-version`, lo exige `sqlx 0.9.1`); el CI usa `stable` |
| Node / pnpm | v24.22.1 · pnpm 12.15.0 (npm 12.2.1) |
| Contenedores | **no hay `docker` ni `/var/run/docker.sock`**; Podman 6.1.4 con API Docker-compatible en `unix:///run/user/1000/podman/podman.sock` (`/_ping` → 200, `/containers/json` responde) |
| CLI | `just` 1.59.0 · `openspec` 1.13.2 · `codegraph` 0.7.9 · `gh` 2.102.1 (autenticado como `atareao`) · `vampus` 0.4.8 · `git-cliff` 2.14.2 |

- **Podman en local, Docker en producción** (decisión tomada el 2026-10-24). En dev, bollard apunta al socket de podman con la ruta configurable (`DOCKER_HOST`); nunca al de Docker. El compose de producción (§8) sigue montando `/var/run/docker.sock`.
- Cualquier comando `docker ...` falla en esta máquina: usa `podman ...`.
- **La versión de API de bollard se fija en la llamada, no se deja deducir**: `Cliente::conectar` construye el motor con `Docker::connect_with_socket(socket, segundos, bollard::API_DEFAULT_VERSION)` —`src/docker/cliente.rs:68`— y nunca con `connect_with_defaults()`, porque una deducción contra un motor que no es Docker (podman) o contra una versión distinta es exactamente el sitio donde un cliente se atasca en la negociación, y esos fallos aparecen más tarde y más lejos de su causa; el rustdoc de `Cliente::conectar` guarda el resto, incluido que hoy ese prefijo no llega al cable y que si algún día hace falta otra versión lo que toca es negociar, no subir la constante.

### Dependencias del workspace (fijadas por `Cargo.lock`)

Las que cambian de versión con el saneamiento de dependencias (issue #167, SEC-11; detalle en `docs/FEATURE-044-saneamiento-de-dependencias.md`). El resto de versiones vive en `Cargo.toml`/`Cargo.lock`, que son la fuente de verdad:

| Crate | Versión | Por qué |
|---|---|---|
| `sqlx` | `0.9.1` | Salto desde `0.8.6`: en `0.9.1` el techo de `libsqlite3-sys` sube a `<0.41.0` y el CVE de SQLite pasa a ser cerrable. La feature es `sqlite-bundled` (no `sqlite`, que en `0.9` enciende extensiones, deserialización y aviso de desbloqueo). Exige el MSRV declarado `1.94` |
| `libsqlite3-sys` | `0.40.0` | El SQLite **empaquetado y compilado en el binario**, dentro del rango que abre `sqlx-sqlite 0.9.1`. Sube el motor de **SQLite 3.46.0 a 3.54.1** y cierra RUSTSEC-2026-0459 (CVE-2026-119375) |
| `rmcp` | `3.11` | SDK del servidor MCP (`kesto-mcp`), de `3.5.1` a `3.11.0`: cierra RUSTSEC-2026-0457/0461/0462 |

- El **MSRV declarado es `1.94`** (`rust-version` en `Cargo.toml`): `sqlx 0.9.1` lo exige. El CI usa `stable`; el entorno local, `1.97`. Un toolchain por debajo de `1.94` falla al resolver, con el número en la mano.
- `zstd 0.13.4` / `zstd-safe 7.4.1` y `async-executor 1.14.1` son parches del mismo change (cierre de avisos y salida del *yanked*).

## Batería e2e — entorno que necesita

`just e2e` (issue #86) **no** corre donde falte motor de contenedores, y no degrada: sin motor o sin S3 **falla** nombrando lo que falta. Necesita:

- **Un motor de contenedores** al que apuntar por `DOCKER_HOST`. En local, el podman de esta máquina (`unix:///run/user/1000/podman/podman.sock`); en el CI, el Docker del runner (`unix:///var/run/docker.sock`). El arnés habla por `bollard` con la API de Docker, que podman implementa compatible.
- **Un servidor S3 real**: el arnés levanta su propio **SeaweedFS** en un contenedor (puerto S3 publicado en `127.0.0.1`, credenciales fijas, cubo por escenario). No se usa un doble local.
- **Las imágenes de origen y de apoyo**, que el CI fija por versión, pre-descarga y pasa por variable; en local el arnés usa las mismas como valor por defecto:

| Pieza | Variable (CI) | Imagen por defecto |
|---|---|---|
| Servidor S3 | `S3_IMAGE` | `docker.io/chrislusf/seaweedfs:latest` |
| Origen MariaDB | `MARIA_IMAGE` | `docker.io/library/mariadb:11` |
| Origen PostgreSQL | `POSTGRES_IMAGE` | `docker.io/library/postgres:18-alpine` |
| Contenedor de apoyo (`sqlite`, `stdin` que no drena) | `ALPINE_IMAGE` | `docker.io/library/alpine:3.23` |

- **`KESTO_MASTER_KEY`**: los `#[ignore]` que el job `e2e` ejecuta necesitan una clave; el CI pasa una de relleno (nunca un secreto real) y el arnés arranca el binario con su propia clave constante.

No añade ninguna dependencia a `Cargo.toml`: SeaweedFS y las bases son imágenes externas, y la orquestación usa el `bollard` y el `reqwest` que ya están.
