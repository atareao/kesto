# PLAN-009 — Permiso del socket del daemon (`group_add` configurable)

Issue: [#28](https://github.com/atareao/kesto/issues/28).

Change OpenSpec: `openspec/changes/permiso-socket-daemon/`.

## Objetivo

Que el contenedor pueda abrir `/var/run/docker.sock` en un host Docker estándar
(socket `root:docker`, modo `srw-rw----`; proceso uid/gid `10001`) **sin editar**
el `compose.yaml`, mediante una variable `DOCKER_GID` con defecto inofensivo.

## Alcance

- `compose.yaml`: `group_add: ["${DOCKER_GID:-10001}"]` en lugar del bloque comentado.
- `.env.example`: documentar `DOCKER_GID` (obtener con `getent group docker`).
- `docs/FEATURE-008-despliegue-produccion.md`: actualizar el follow-up.

Fuera de alcance: permisos del socket más allá del grupo, TLS/`HSTS`, backup del volumen.

## Tareas

Ver `openspec/changes/permiso-socket-daemon/tasks.md`.

## Criterio de cierre

- `DOCKER_GID=<gid> podman compose config` declara ese gid en `group_add`.
- Sin `DOCKER_GID`, `podman compose config` no falla y `group_add` queda en `10001`.
- `just check` en verde (no cambia código Rust/UI).
- Change archivado (`openspec archive`) y follow-up actualizado.
