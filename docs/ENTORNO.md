# Entorno de desarrollo y variables de entorno

> Extracto **literal** del `AGENTS.md` de Kesto (poda v2, 2026-10-31). Tabla completa de variables, justificación de cada regla de arranque y versiones del entorno verificado. El `AGENTS.md` conserva la regla imperativa de cada viñeta.

## Variables de entorno

| Variable | Obligatoria | Valor por defecto | Para qué |
|---|---|---|---|
| `RUST_LOG` | no | `info` | Filtro de `tracing-subscriber`. Nombre del §8, sin prefijo |
| `BIND_ADDRESS` | no | `0.0.0.0:8080` | Dirección de escucha. Nombre del §8, sin prefijo |
| `DATABASE_URL` | no | `sqlite:///app/data/kesto.db` | DSN de SQLite. Nombre del §8, sin prefijo. **Ya se usa**: el arranque abre la base ahí y aplica las migraciones embebidas. Ojo con el defecto: `sqlite:///app/data/kesto.db` es el del volumen del `compose` del §8, así que en una máquina sin `/app/data` **el arranque falla nombrando la ruta** —Kesto no crea el directorio a propósito: crearlo escondería un volumen mal montado—. Para desarrollo, exporta el tuyo (`DATABASE_URL=sqlite:///tmp/kesto/kesto.db`, con el directorio ya creado) |
| `DOCKER_HOST` | no | `unix:///var/run/docker.sock` | Socket con el que el motor habla con Docker. **Sin prefijo `KESTO_`**: es el nombre que ya usa la biblioteca de Docker, y respetarlo deja que un `compose` o un `DOCKER_HOST` del entorno apunten al socket de Podman en desarrollo sin editar la configuración. **Ya se usa**: el motor construye su cliente con esta ruta, y su ausencia, su vacío y su blanco caen al defecto —a diferencia de `BIND_ADDRESS` y `RUST_LOG`, que matan el arranque— |
| `KESTO_MASTER_KEY` | **sí, siempre** | — | Clave maestra del cifrado en reposo: de ella se derivan las claves que cifran los tres secretos del §3 (ver *Cifrado en reposo*) |
| `KESTO_SECRET` | **sí, si el modo es OIDC** | — | Clave de firma del JWT de sesión (`HS256`). Sin ella en modo OIDC el arranque falla nombrándola |
| `OIDC_ISSUER_URL` | sí, si el modo es OIDC | — | Emisor del proveedor |
| `OIDC_CLIENT_ID` | sí, si el modo es OIDC | — | Identificador del cliente |
| `OIDC_CLIENT_SECRET` | sí, si el modo es OIDC | — | Secreto del cliente |
| `OIDC_REDIRECT_URL` | no | `http://localhost:8080/api/v1/auth/callback` | URL de retorno del callback |

- Los nombres que se leen **tal cual**, sin prefijo `KESTO_`, son los tres del §8 (`RUST_LOG`, `BIND_ADDRESS`, `DATABASE_URL`) y `DOCKER_HOST` —este último no lo trae el §8: es el nombre con el que la biblioteca de Docker, y el propio entorno, designa el socket—, para que un `compose` copiado de `docs/DEFINITION.md` o un despliegue que ya exporte `DOCKER_HOST` funcionen sin editar una línea. El resto lleva prefijo `KESTO_`, salvo `OIDC_*`, que conserva el nombre que ya usa shuul.
- Las tres variables `OIDC_*` que gobiernan el modo (`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID` y `OIDC_CLIENT_SECRET`) son **un todo o nada**: las tres presentes encienden la autenticación; ninguna presente la apaga y lo anuncia con un aviso; unas sí y otras no es **error de arranque**, no un modo degradado. Degradar a «apagada» dejaría el panel abierto sin que nadie se entere.
- `KESTO_MASTER_KEY` es obligatoria **siempre**: es la raíz de la que se derivan las claves de cifrado en reposo de los tres secretos del §3.
- `KESTO_SECRET` es obligatoria **en modo OIDC** —es la clave con la que se firma la sesión— y no hace falta en modo `Disabled`. **Rotarla invalida todas las sesiones vivas**: no hay tabla de sesiones ni estado en el servidor, cada petición se verifica por firma contra la clave que tenga el proceso en memoria.
- Una credencial de menos de 32 caracteres **avisa y arranca igualmente**: Kesto es el orquestador de las copias, y negarse a arrancar por una variable floja convertiría un error de configuración en ausencia de copias.
- `BIND_ADDRESS` sin puerto es error de arranque, y **el valor vacío no cae al defecto**: `BIND_ADDRESS=` mata el arranque. Si usas `compose` con interpolación de una variable que no definiste, acuérdate de esto.
- Lo mismo vale para `RUST_LOG`: presente pero vacía es **error de arranque**, no «sin registro». Si dejas `RUST_LOG=` en un `compose` con la variable sin definir, el proceso no arranca —que es mucho mejor que un servicio ciego—. Quitarla del todo sí cae al valor por defecto, `info`. Los espacios sobrantes se recortan: `RUST_LOG=" info "` funciona igual que `RUST_LOG=info`.
- Un `OIDC_REDIRECT_URL` configurado **solo**, sin las otras tres, se ignora en silencio: los tres desenlaces se deciden sobre las tres variables de la autenticación. Quien configure solo el retorno creerá que encendió algo.
- ⚠️ Y el `compose` del §8 **tal cual está publicado no arranca**: su bloque `environment` trae `RUST_LOG`, `DATABASE_URL` y `BIND_ADDRESS`, pero **falta `KESTO_MASTER_KEY`**, que es obligatoria siempre, así que el contenedor muere en el arranque nombrando la variable —Kesto es el orquestador de las copias y no arranca con una configuración incompleta—. Y aun añadiéndola queda decidir el modo: ese mismo `compose` no trae ninguna de las tres `OIDC_*`, de modo que arrancaría `Disabled`, **sin capa de autenticación y con todo `/api/v1/*` abierto en el 8080** —el modo de desarrollo de más abajo, que no es para un despliegue expuesto—. El §8 es un punto de partida, no un despliegue listo.


## Entorno de desarrollo (verificado en esta máquina)

| Herramienta | Estado |
|---|---|
| Rust / Cargo | 1.97 · `edition = "2024"` (exige ≥ 1.85) |
| Node / pnpm | v24.22.1 · pnpm 12.15.0 (npm 12.2.1) |
| Contenedores | **no hay `docker` ni `/var/run/docker.sock`**; Podman 6.1.4 con API Docker-compatible en `unix:///run/user/1000/podman/podman.sock` (`/_ping` → 200, `/containers/json` responde) |
| CLI | `just` 1.59.0 · `openspec` 1.13.2 · `codegraph` 0.7.9 · `gh` 2.102.1 (autenticado como `atareao`) · `vampus` 0.4.6 · `git-cliff` 2.14.2 |

- **Podman en local, Docker en producción** (decisión tomada el 2026-10-24). En dev, bollard apunta al socket de podman con la ruta configurable (`DOCKER_HOST`); nunca al de Docker. El compose de producción (§8) sigue montando `/var/run/docker.sock`.
- Cualquier comando `docker ...` falla en esta máquina: usa `podman ...`.
- **La versión de API de bollard se fija en la llamada, no se deja deducir**: `Cliente::conectar` construye el motor con `Docker::connect_with_socket(socket, segundos, bollard::API_DEFAULT_VERSION)` —`src/docker/cliente.rs:68`— y nunca con `connect_with_defaults()`, porque una deducción contra un motor que no es Docker (podman) o contra una versión distinta es exactamente el sitio donde un cliente se atasca en la negociación, y esos fallos aparecen más tarde y más lejos de su causa; el rustdoc de `Cliente::conectar` guarda el resto, incluido que hoy ese prefijo no llega al cable y que si algún día hace falta otra versión lo que toca es negociar, no subir la constante.
