# Proposal

## Why

El motor del §4, la API del §5, el panel del §6 y el planificador ya están entregados y probados, pero Kesto todavía no se puede **desplegar**: no existe `Dockerfile` ni fichero de compose, así que el único modo de ejecutarlo es `cargo run` en la máquina de desarrollo. La publicación de la imagen de contenedor está además dormida a propósito —`preflight` de `release.yml` se salta el job `image` mientras no existan `Dockerfile` y `ui/package.json`—, con lo que una release hoy no produce ningún entregable desplegable.

Este change cierra ese hueco: empaqueta el binario y el panel compilado en una única imagen y describe el despliegue de referencia del §8 con un `compose.yaml`. Con él, un operador puede levantar Kesto en un host con Docker sin toolchain de Rust, sin Node y sin copiar el árbol de fuentes.

Es el último tramo de producto que queda de la definición: el panel ya deja el hueco del servido estático preparado (`api-shell`), y el `Dockerfile` multi-stage estaba expresamente **fuera** del change `ui-panel` (issue aparte).

Issue de entrada: [#17](https://github.com/atareao/kesto/issues/17).
SRS de referencia: §8 «Despliegue en Producción (Docker Compose)» (`docs/DEFINITION.md:267`) y la estructura del §7 (`docs/DEFINITION.md`, `Dockerfile # Multi-stage build (UI build + Rust binary)`).
Plan: `plans/PLAN-008.md`.

## What Changes

- Se añade un **`Dockerfile` multi-stage** con tres etapas en el orden del §7 —UI build → Rust build → runtime—: la primera compila el panel (`ui/` con `pnpm`), la segunda compila el binario de release de Rust, y la tercera es una imagen de runtime mínima que **no** lleva toolchain ni Node ni el árbol de fuentes, corre **sin root** (uid/gid `10001`, el mismo que ya asume `just run`) y expone el panel y la API **en el mismo origen** por el puerto `8080`.
- Se ajusta el **`.dockerignore`** (ya existe) para que el contexto de build no arrastre `target/`, `ui/node_modules/`, `ui/dist/`, `data/`, `.env`, ni el propio `Dockerfile`; y para que el árbol de fuentes del panel sí entre en la etapa de UI.
- Se crea el **`compose.yaml`** del §8, literal: servicio `kesto`, `image: ghcr.io/atareao/kesto:latest`, `container_name: kesto-app`, `restart: always`, `ports: "8080:8080"`, las tres variables de entorno de arranque y los cuatro montajes, con el volumen nombrado `kesto_data` para persistir la base SQLite.

  > **Cabo suelto del §8:** el ejemplo del SRS usa `image: kesto:latest` (etiqueta local sin registro). Aquí se fija `ghcr.io/atareao/kesto:latest`, que es la referencia real que produce `just build` y la que publica el job `image` de `release.yml`; así `podman compose up` no depende de un tag manual previo.
- Se fija la **inyección de secretos**: `KESTO_MASTER_KEY` es obligatoria siempre y viaja al contenedor por `.env`/secrets, nunca en claro dentro de la imagen ni del compose versionado; en modo OIDC se exige además `KESTO_SECRET` y las tres `OIDC_*` como un todo o nada.
- Se verifica el **arranque real** en la configuración de desarrollo (modo `disabled`): el contenedor levanta solo con `KESTO_MASTER_KEY`, `GET /api/v1/health` contesta con `auth_mode: "disabled"` y `GET /` sirve el panel, con la base persistida en `kesto_data` entre recreaciones.
- Se escribe la documentación de la feature en `docs/FEATURE-008-despliegue-produccion.md`.

## Capabilities

### New Capabilities

- `deploy`: el empaquetado y el despliegue de Kesto —la imagen multi-stage sin root y sin toolchain, el compose del §8, la inyección de la configuración y los secretos, y la verificación de arranque en el modo de desarrollo—.

### Modified Capabilities

Ninguna. La imagen consume la superficie ya fijada por `api-shell` (servido del panel y `/api/v1/health`) sin cambiar su contrato; este change la empaqueta, no la redefine.

## Impact

- **Ficheros nuevos:** `Dockerfile`, `compose.yaml`, `docs/FEATURE-008-despliegue-produccion.md`.
- **Ficheros modificados:** `.dockerignore` (ya existe; se revisa y completa), `HANDOFF.md` y el renombrado de `plans/PLAN-008.md` al cerrar.
- **`justfile`:** pasa a operativas las recetas que hoy abortan por falta de artefacto —`build`/`build-nocache` (exigen `Dockerfile`), `deploy` (exige compose) y `health`—; no se cambia su contrato, solo dejan de fallar.
- **CI / release:** el job `image` de `release.yml` se enciende **solo** cuando existen `Dockerfile` y `ui/package.json`; no se edita el workflow. Un merge a `main` pasa a ser una release con imagen publicable en `ghcr.io/atareao/kesto`.
- **Entornos:** se construye y se prueba con **Podman** en local (`BUILDAH_FORMAT=docker` para conservar el `HEALTHCHECK`), y el despliegue de referencia es **Docker** en producción, con el socket del daemon montado en `/var/run/docker.sock` y `DOCKER_HOST` configurable.
- **Secretos:** ni `KESTO_MASTER_KEY` ni `KESTO_SECRET` ni las `OIDC_*` quedan escritos en el `Dockerfile`, en el `compose.yaml` versionado ni en una capa de la imagen.
- **Fuera de alcance:** orquestación distinta de compose (Kubernetes, Swarm), proxy inverso/TLS y terminación HTTPS en el host, gestión de usuarios y permisos, copias de seguridad del propio volumen `kesto_data` y migración de datos desde un despliegue anterior.
