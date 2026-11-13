# FEATURE-009 — Permiso del socket del daemon (`group_add` configurable)

Plan: `plans/PLAN-009-COMPLETED.md` · Issue: [#28](https://github.com/atareao/kesto/issues/28) · Change OpenSpec: `permiso-socket-daemon` (archivado).

## Qué es

Hace que el contenedor de Kesto pueda abrir `/var/run/docker.sock` en un host Docker estándar **sin editar** el `compose.yaml`, mediante la variable `DOCKER_GID`.

## El problema

En un host Docker estándar el socket del daemon es `root:docker` con modo `srw-rw----`. El proceso de Kesto corre **sin privilegios** como uid/gid `10001`, así que no pertenece al grupo dueño del socket y **no puede abrirlo**: bollard no conecta y no puede lanzar los `exec` de copia. El `compose.yaml` solo llevaba la línea `group_add` comentada, obligando al operador a editar el fichero versionado.

## Ficheros implicados

| Fichero | Papel |
|---|---|
| `compose.yaml` | `group_add: ["${DOCKER_GID:-10001}"]` en el servicio `kesto`. |
| `.env.example` | Documenta `DOCKER_GID` (`getent group docker`). |
| `openspec/specs/deploy/spec.md` | Requisito «El contenedor accede al socket del daemon con el grupo del host». |

## Cómo funciona

`group_add` añade grupos suplementarios al proceso del contenedor. El valor sale de `DOCKER_GID`; si no está definida, cae a `10001`, el propio gid del proceso: añadirlo de nuevo es **inofensivo**, y deja intacto el arranque en modo de desarrollo (sin daemon) y en Podman rootless, donde el socket pertenece al usuario.

A diferencia de un `DOCKER_GID` obligatorio, el defecto inofensivo respeta el escenario «el contenedor arranca solo con la clave maestra» del spec `deploy`.

## Configuración

En el `.env` del despliegue:

```bash
DOCKER_GID=$(getent group docker | cut -d: -f3)
```

## Cómo comprobar

```bash
DOCKER_GID=998 podman compose config | grep -A1 group_add   # → 998
podman compose config | grep -A1 group_add                  # → 10001
```

La prueba del socket **real** requiere un host con daemon accesible; en su defecto se valida por `podman compose config`.

## Decisiones

- **Defecto inofensivo (`10001`)** en vez de `DOCKER_GID` obligatorio: conserva el arranque sin daemon y no rompe el escenario de arranque mínimo del spec.
- **Variable de entorno** en vez de editar el fichero: coherente con cómo entran el resto de configuración y secretos.
- El socket se sigue montando en **solo lectura** (`:ro`); no cambia.

## Follow-ups

- TLS/`HSTS` si el TLS no lo termina un proxy delante.
- Backup del volumen `kesto_data`.
