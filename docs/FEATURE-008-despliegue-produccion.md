# FEATURE-008 — despliegue-produccion

> Implementación del change proposal `despliegue-produccion`, en `openspec/changes/despliegue-produccion/`. Issue [#17](https://github.com/atareao/kesto/issues/17). Plan: `plans/PLAN-008.md`. Rama: `feature/despliegue-produccion`, desde `development`. Especificación de origen: `docs/DEFINITION.md` §8 (compose de producción), §2 (componentes) y §7 (estructura).
> Estado: **implementado, verde y verificado** — `Dockerfile`, `compose.yaml` y `.env.example` construidos con Podman; imagen de **200 MB**, contenedor `(healthy)`, health/panel servidos y volumen persistente. `openspec validate despliegue-produccion --strict` OK.

**Kesto ya se despliega como un contenedor.** Hasta este change el producto se ejecutaba desde las fuentes —o con la receta `just run`— y `Dockerfile` era, junto con `ui/`, la pieza que el §7 declaraba y nadie había escrito. Ahora el repositorio empaqueta el binario de release y el panel compilado en una imagen multi-stage sin toolchain y sin privilegios, y la pone en marcha con el compose de producción del §8, cerrando los cabos que el ejemplo del SRS deja sueltos (el principal: `KESTO_MASTER_KEY`, que es obligatoria siempre y el §8 omite).

## Qué es

Empaquetado y despliegue de Kesto en producción (§8): una imagen de contenedor reproducible que reúne el binario y el panel compilado, un `compose.yaml` que la levanta con el socket del daemon, las fuentes de copia y el volumen de datos, y un contrato explícito de cómo entran la configuración y los secretos. La imagen se construye en **tres etapas** (panel → binario → runtime mínimo) y el runtime corre **sin privilegios** y sirve el panel y la API en el **mismo origen**.

## Ficheros implicados

| Ruta | Qué es |
|---|---|
| `Dockerfile` | Multi-stage: `ui-builder` (`node:24-bookworm-slim` + **pnpm 12** por corepack → `ui/dist`) → `rust-builder` (`rust:1.97-bookworm`, `cargo build --release --locked`) → `runtime` (`debian:bookworm-slim`). |
| `compose.yaml` | Servicio `kesto` del §8: `image: ghcr.io/atareao/kesto:latest`, `container_name: kesto-app`, `restart: always`, `"8080:8080"`, entorno de arranque, cuatro volúmenes y el volumen nombrado `kesto_data`. |
| `.env.example` | Plantilla de variables del despliegue (la clave maestra y las del modo OIDC) que se copia a `.env`, nunca versionado. |
| `.dockerignore` | Exclusiones del contexto de build (`target/`, `ui/node_modules/`, `ui/dist/`, `.git/`, `docs/`, `plans/`, `.env*`…). Ya existía; se mantiene. |

## Cómo funciona

El panel se sirve desde **`/app/ui/dist`**: `PANEL_POR_DEFECTO="ui/dist"` es una ruta **relativa al CWD** y el proceso arranca con `WORKDIR /app`, así que el binario encuentra el panel donde lo espera. La etapa de runtime copia el binario a **`/app/kesto`** (el `CMD` es `/app/kesto`) y el panel compilado a `/app/ui/dist`, y crea **`/app/data`** propiedad del usuario sin privilegios.

El proceso corre como **`USER 10001:10001`** —el mismo uid/gid que asume `just run`—, nunca como `root`. La imagen declara **`EXPOSE 8080`** y un **`HEALTHCHECK`** con `curl` contra `/api/v1/health`, de modo que el daemon distingue «arrancado» de «en pie» sin ayuda externa.

`migrations/` se copia en la etapa `rust-builder` **antes** del build porque `sqlx::migrate!()` lo lee en **tiempo de compilación** y lo incrusta en el binario: sin ese directorio el binario no compila.

El volumen nombrado **`kesto_data`** se monta en `/app/data` (la SQLite vive ahí), de modo que los datos sobreviven a la parada y recreación del contenedor.

## Variables/config

- **`KESTO_MASTER_KEY`** — obligatoria **siempre**; es la clave de la que se deriva la del cifrado en reposo. Llega por el `.env` del despliegue o un gestor de secretos, **nunca en claro** en la imagen ni en el `compose.yaml` versionado.
- **`KESTO_SECRET`** + las tres **`OIDC_*`** (`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`) — «todo o nada»: con las tres presentes el modo es **OIDC** (y entonces `KESTO_SECRET` es obligatoria, la clave de firma de sesión); con ninguna, el modo es **`disabled`** (desarrollo); con unas sí y otras no, el arranque falla.
- **`RUST_LOG`**, **`BIND_ADDRESS`**, **`DATABASE_URL`** — se leen **sin prefijo `KESTO_`**, tal cual. Una variable **presente pero vacía** (`BIND_ADDRESS=`, `RUST_LOG=`) es **error de arranque**, no un retorno al defecto.
- El `compose.yaml` usa **`${KESTO_MASTER_KEY:?...}`**: sin la clave maestra, `podman compose up` (o `compose config`) **falla antes de crear el contenedor**. Dentro del proceso, en cambio, una credencial de **menos de 32 caracteres avisa y arranca igualmente** (higiene, no puerta).

## Cómo construir y desplegar

```bash
# Construcción local de la imagen (Podman, formato docker forzado en el .justfile)
just build

# Despliegue de producción (lee .env del host; exige KESTO_MASTER_KEY)
podman compose up -d        # o: just deploy
just health                 # comprueba /api/v1/health

# Desarrollo (sin contenedor): backend y panel por separado
just dev
just ui-dev
```

`just build` fuerza **`BUILDAH_FORMAT=docker`**: el formato **OCI descarta el `HEALTHCHECK` en silencio**.

## Decisiones

- **Imagen `ghcr.io/atareao/kesto:latest`** — el §8 decía `kesto:latest`; se corrige a la referencia real del registro del repositorio (cabo suelto del SRS cerrado).
- **Usuario `10001`** — el mismo que asume la receta `just run`, para que lo construido y lo desplegado coincidan.
- **`BUILDAH_FORMAT=docker`** — el formato OCI descarta el `HEALTHCHECK` sin avisar; el formato docker lo conserva.
- **Secretos por entorno** — nunca en la imagen ni en el compose versionado; la clave maestra llega por `.env`/secrets del host.

## Verificación (medida)

- Build `ghcr.io/atareao/kesto:latest` / `:0.2.0` → **200 MB**.
- `cargo`, `rustc`, `node` y `pnpm` **no** están en la imagen final; `id` → `uid=10001`.
- `HEALTHCHECK` presente y el contenedor alcanza `(healthy)`.
- `GET /api/v1/health` → **`200`** con `auth_mode:"disabled"` (version `0.2.0`); `GET /` → **`200`** `text/html`.
- El volumen `kesto_data` **persiste** entre contenedores.
- `podman compose config` válido con la master key. Rechazos: sin master key falla `compose config` (exit 1); OIDC parcial → `la configuración de OIDC está incompleta: falta OIDC_CLIENT_SECRET`; `BIND_ADDRESS=` → `tiene un valor con formato inválido`; credencial corta → `WARN KESTO_MASTER_KEY mide menos de 32 caracteres` y arranca.

## Follow-ups

- Las rutas de respaldo del §8 (`/var/www`, `/etc/nginx`) son **ejemplo del host**: el operador debe ajustarlas a su entorno.
- **Permiso del socket del daemon:** en un host Docker estándar el socket es `root:docker` con modo `srw-rw----` y el proceso corre como uid/gid `10001`, así que necesita el grupo del daemon. Ya **no se edita el fichero**: `compose.yaml` declara `group_add: ["${DOCKER_GID:-10001}"]` y basta definir `DOCKER_GID` (`getent group docker`) en el `.env`. Sin definirla, el defecto (el propio gid `10001`) mantiene el arranque en modo de desarrollo. En local lo cubre `just run` con `--userns=keep-id`.
- Considerar `Strict-Transport-Security` si el TLS no lo termina un proxy.
- El job `image` de `release.yml` se **enciende solo** al existir `Dockerfile` + `ui/package.json`: publica multi-arquitectura a ghcr.
- Sin gestión de usuarios ni backup del volumen: fuera del alcance de este change.
