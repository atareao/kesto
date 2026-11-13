# Proposal

## Why

El `compose.yaml` monta el socket del daemon para que bollard lance los `exec` de copia, pero en un host Docker estándar ese socket es `root:docker` con modo `srw-rw----` y el proceso del contenedor corre como uid/gid `10001`: **no puede abrirlo**. Hoy el fichero solo lleva la línea `group_add` comentada y una nota que remite a `getent group docker`, así que el operador tiene que editar el compose versionado a mano —justo lo que el propio fichero evita para la configuración y los secretos.

Issue de entrada: [#28](https://github.com/atareao/kesto/issues/28).
Follow-up de `docs/FEATURE-008-despliegue-produccion.md` («Permiso del socket del daemon»).
Plan: `plans/PLAN-009.md`.

## What Changes

- En `compose.yaml`, se sustituye el bloque comentado por un `group_add` real alimentado por la variable `DOCKER_GID`, con un defecto inofensivo —el gid del propio proceso (`10001`)— de modo que el arranque en modo de desarrollo (sin daemon) siga sin requerir configuración.
- En `.env.example` se documenta `DOCKER_GID`, con la forma de obtenerla (`getent group docker`).
- Se actualiza el follow-up en `docs/FEATURE-008-despliegue-produccion.md`.
- No se toca código Rust ni el panel: es configuración de despliegue.

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `deploy`: se añade el requisito de que el contenedor acceda al socket del daemon con el grupo del host, configurable por `DOCKER_GID` y con defecto inofensivo.

## Impact

- **Ficheros:** `compose.yaml`, `.env.example`, `docs/FEATURE-008-despliegue-produccion.md`.
- **Entornos:** Podman en local (rootless; el defecto no cambia el comportamiento actual) y Docker en producción (el operador define `DOCKER_GID`).
- **Sin cambios** en la imagen, el binario, la API ni el panel.
