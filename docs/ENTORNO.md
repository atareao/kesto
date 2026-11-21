# Entorno de desarrollo y variables de entorno

> Extracto **literal** del `AGENTS.md` de Kesto (poda v2, 2026-10-31). Tabla completa de variables, justificación de cada regla de arranque y versiones del entorno verificado. El `AGENTS.md` conserva la regla imperativa de cada viñeta.

## Variables de entorno

| Variable | Obligatoria | Valor por defecto | Para qué |
|---|---|---|---|
| `RUST_LOG` | no | `info` | Filtro de `tracing-subscriber`. Nombre del §8, sin prefijo |
| `BIND_ADDRESS` | no | `127.0.0.1:8080` | Dirección de escucha. Nombre del §8, sin prefijo. El defecto es de **loopback**, y no el `0.0.0.0:8080` del §8: escuchar en todas las interfaces es una decisión explícita del operador —el §8 la toma en su `compose`, con proveedor de identidad— y en modo de desarrollo es obligatorio que sea de loopback |
| `DATABASE_URL` | no | `sqlite:///app/data/kesto.db` | DSN de SQLite. Nombre del §8, sin prefijo. **Ya se usa**: el arranque abre la base ahí y aplica las migraciones embebidas. Ojo con el defecto: `sqlite:///app/data/kesto.db` es el del volumen del `compose` del §8, así que en una máquina sin `/app/data` **el arranque falla nombrando la ruta** —Kesto no crea el directorio a propósito: crearlo escondería un volumen mal montado—. Para desarrollo, exporta el tuyo (`DATABASE_URL=sqlite:///tmp/kesto/kesto.db`, con el directorio ya creado) |
| `DOCKER_HOST` | no | `unix:///var/run/docker.sock` | Socket con el que el motor habla con Docker. **Sin prefijo `KESTO_`**: es el nombre que ya usa la biblioteca de Docker, y respetarlo deja que un `compose` o un `DOCKER_HOST` del entorno apunten al socket de Podman en desarrollo sin editar la configuración. **Ya se usa**: el motor construye su cliente con esta ruta, y su ausencia, su vacío y su blanco caen al defecto —a diferencia de `BIND_ADDRESS` y `RUST_LOG`, que matan el arranque— |
| `KESTO_HSTS_MAX_AGE` | no | desactivada | Segundos de la cabecera `Strict-Transport-Security` (HSTS). Ausente → sin cabecera (en el despliegue estándar la HSTS la pone el proxy que termina el TLS). Con un entero ≥ 0, **toda** respuesta lleva `Strict-Transport-Security: max-age=<n>` (incluidos los `404` y `405`). Presente pero vacía o no numérica → **error de arranque** |
| `KESTO_MASTER_KEY` | **sí, siempre** | — | Clave maestra del cifrado en reposo: de ella se derivan las claves que cifran los tres secretos del §3 (ver *Cifrado en reposo*). **Mínimo 32 caracteres**; más corta o vacía **impide el arranque** |
| `KESTO_SECRET` | **sí, si el modo es OIDC** | — | Clave de firma del JWT de sesión (`HS256`). Sin ella en modo OIDC el arranque falla nombrándola. **Mínimo 32 caracteres**; más corta o vacía **impide el arranque** |
| `KESTO_ALLOW_INSECURE` | **sí, si el modo es el de desarrollo** | — | Permiso explícito del modo sin autenticación. `1` permite; `0` o ausente lo niega y **mata el arranque**; cualquier otro valor (vacío incluido) es error de arranque. Con el permiso, la escucha tiene que ser de **loopback**. En modo OIDC ni se consulta |
| `OIDC_ISSUER_URL` | sí, si el modo es OIDC | — | Emisor del proveedor |
| `OIDC_CLIENT_ID` | sí, si el modo es OIDC | — | Identificador del cliente |
| `OIDC_CLIENT_SECRET` | sí, si el modo es OIDC | — | Secreto del cliente |
| `OIDC_REDIRECT_URL` | no | `http://localhost:8080/api/v1/auth/callback` | URL de retorno del callback |

- Los nombres que se leen **tal cual**, sin prefijo `KESTO_`, son los tres del §8 (`RUST_LOG`, `BIND_ADDRESS`, `DATABASE_URL`) y `DOCKER_HOST` —este último no lo trae el §8: es el nombre con el que la biblioteca de Docker, y el propio entorno, designa el socket—, para que un `compose` copiado de `docs/DEFINITION.md` o un despliegue que ya exporte `DOCKER_HOST` funcionen sin editar una línea. El resto lleva prefijo `KESTO_`, salvo `OIDC_*`, que conserva el nombre que ya usa shuul.
- Las tres variables `OIDC_*` que gobiernan el modo (`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID` y `OIDC_CLIENT_SECRET`) son **un todo o nada**: las tres presentes encienden la autenticación; ninguna presente la apaga y lo anuncia con un aviso; unas sí y otras no es **error de arranque**, no un modo degradado. Degradar a «apagada» dejaría el panel abierto sin que nadie se entere.
- `KESTO_MASTER_KEY` es obligatoria **siempre**: es la raíz de la que se derivan las claves de cifrado en reposo de los tres secretos del §3.
- `KESTO_SECRET` es obligatoria **en modo OIDC** —es la clave con la que se firma la sesión— y no hace falta en modo `Disabled`. **Rotarla invalida todas las sesiones vivas**: no hay tabla de sesiones ni estado en el servidor, cada petición se verifica por firma contra la clave que tenga el proceso en memoria.
- Una credencial —`KESTO_MASTER_KEY` o, en modo OIDC, `KESTO_SECRET`— de menos de **32 caracteres**, **vacía incluida**, **impide el arranque** nombrando la variable y el mínimo. Antes solo avisaba: con una clave maestra corta la del cifrado en reposo es forzable, y con un `KESTO_SECRET` corto la sesión es forjable, así que el aviso dejaba el sistema en producción con los secretos al alcance de quien leyera el fichero.
- El modo de desarrollo —sin ninguna `OIDC_*`— exige el permiso explícito `KESTO_ALLOW_INSECURE=1` **y** una dirección de escucha de loopback: sin el permiso el proceso se niega a arrancar en abierto, y con él `BIND_ADDRESS=0.0.0.0:8080` sigue siendo error de arranque. El permiso no se consulta en modo OIDC.
- `BIND_ADDRESS` sin puerto es error de arranque, y **el valor vacío no cae al defecto**: `BIND_ADDRESS=` mata el arranque. Si usas `compose` con interpolación de una variable que no definiste, acuérdate de esto.
- Lo mismo vale para `RUST_LOG`: presente pero vacía es **error de arranque**, no «sin registro». Si dejas `RUST_LOG=` en un `compose` con la variable sin definir, el proceso no arranca —que es mucho mejor que un servicio ciego—. Quitarla del todo sí cae al valor por defecto, `info`. Los espacios sobrantes se recortan: `RUST_LOG=" info "` funciona igual que `RUST_LOG=info`.
- Un `OIDC_REDIRECT_URL` configurado **solo**, sin las otras tres, se ignora en silencio: los tres desenlaces se deciden sobre las tres variables de la autenticación. Quien configure solo el retorno creerá que encendió algo.
- ⚠️ El `compose` del §8 y el `compose.yaml` del repositorio **exigen** `KESTO_MASTER_KEY`, las tres `OIDC_*` y `KESTO_SECRET` con la forma `${VAR:?}`: un `.env` al que le falte cualquiera de ellas **no levanta el servicio, sin crear el contenedor**. Es lo que impide que un despliegue que pierde sus `OIDC_*` arranque `Disabled` —sin capa de autenticación y con todo `/api/v1/*` abierto en el 8080—: la plantilla no puede degradar a ese modo, que exige permiso explícito y loopback, así que `BIND_ADDRESS=0.0.0.0:8080` solo es legítimo con proveedor.

> ⚠️ **Nota de actualización (endurecimiento del arranque).** Un despliegue que ya esté en marcha con una `KESTO_MASTER_KEY` —o, en modo OIDC, un `KESTO_SECRET`— de menos de 32 caracteres **dejará de arrancar** al actualizarse: la variable pasa de aviso a puerta de arranque. La reparación es poner una credencial de 32 caracteres o más. Ojo con cuál se toca: **rotar la clave maestra deja ilegibles los secretos ya guardados** (los tres del §3), así que no es un cambio de variable sino una operación con plan —hay que volver a dar de alta esos secretos—; rotar `KESTO_SECRET`, en cambio, solo invalida las sesiones vivas.


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
| Rust / Cargo | 1.97 · `edition = "2024"` (exige ≥ 1.85) |
| Node / pnpm | v24.22.1 · pnpm 12.15.0 (npm 12.2.1) |
| Contenedores | **no hay `docker` ni `/var/run/docker.sock`**; Podman 6.1.4 con API Docker-compatible en `unix:///run/user/1000/podman/podman.sock` (`/_ping` → 200, `/containers/json` responde) |
| CLI | `just` 1.59.0 · `openspec` 1.13.2 · `codegraph` 0.7.9 · `gh` 2.102.1 (autenticado como `atareao`) · `vampus` 0.4.8 · `git-cliff` 2.14.2 |

- **Podman en local, Docker en producción** (decisión tomada el 2026-10-24). En dev, bollard apunta al socket de podman con la ruta configurable (`DOCKER_HOST`); nunca al de Docker. El compose de producción (§8) sigue montando `/var/run/docker.sock`.
- Cualquier comando `docker ...` falla en esta máquina: usa `podman ...`.
- **La versión de API de bollard se fija en la llamada, no se deja deducir**: `Cliente::conectar` construye el motor con `Docker::connect_with_socket(socket, segundos, bollard::API_DEFAULT_VERSION)` —`src/docker/cliente.rs:68`— y nunca con `connect_with_defaults()`, porque una deducción contra un motor que no es Docker (podman) o contra una versión distinta es exactamente el sitio donde un cliente se atasca en la negociación, y esos fallos aparecen más tarde y más lejos de su causa; el rustdoc de `Cliente::conectar` guarda el resto, incluido que hoy ese prefijo no llega al cable y que si algún día hace falta otra versión lo que toca es negociar, no subir la constante.

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
