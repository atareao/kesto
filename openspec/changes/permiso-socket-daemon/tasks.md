# Tasks

## 0. Preparación

- [ ] 0.1 Confirmar que el issue [#28](https://github.com/atareao/kesto/issues/28) existe y describe el alcance; si no existe, crearlo con `gh issue create` y asignarlo al usuario. Verificación: `gh issue view 28` devuelve el alcance acordado.
- [ ] 0.2 Medir la línea base: `podman compose config` con solo `KESTO_MASTER_KEY` pasa hoy, pero el `group_add` está comentado, así que el contenedor no puede abrir el socket. Verificación: la salida de `podman compose config` no muestra ningún `group_add`.

## 1. Compose (`group_add` configurable)

- [ ] 1.1 RED: comprobar que `DOCKER_GID=<gid> podman compose config` **no** refleja el gid (la línea está comentada). Verificación: la salida no contiene `<gid>` en `group_add`.
- [ ] 1.2 GREEN: sustituir el bloque comentado por `group_add: ["${DOCKER_GID:-10001}"]`. Verificación: `DOCKER_GID=998 podman compose config` muestra `998`; sin `DOCKER_GID` muestra `10001` y no falla.
- [ ] 1.3 Verificar el caso de desarrollo (sin daemon): `podman compose config` con solo `KESTO_MASTER_KEY` sigue validando. Verificación: exit 0.
- [ ] 1.4 Verificar el caso real (si hay daemon disponible): con el gid del grupo del socket, el proceso puede abrirlo. Verificación: el contenedor arranca y bollard conecta (o, si no hay daemon en la máquina, dejar constancia de que se valida por `compose config`).

## 2. Documentación

- [ ] 2.1 `.env.example`: añadir `DOCKER_GID=` con el comentario de cómo obtenerlo (`getent group docker`). Verificación: `rg 'DOCKER_GID' .env.example` muestra la variable documentada y sin valor.
- [ ] 2.2 `docs/FEATURE-008-despliegue-produccion.md`: actualizar el follow-up del permiso del socket para reflejar que ya es configurable por `DOCKER_GID`. Verificación: el follow-up deja de decir «hay que editar a mano».

## 3. Cierre

- [ ] 3.1 `just check` en verde (no cambia código Rust/UI, pero se confirma que nada se rompe). Verificación: fmt, clippy y test pasan.
- [ ] 3.2 `openspec validate permiso-socket-daemon --strict` en verde. Verificación: sale «valid».
- [ ] 3.3 Archivar el change (`just spec-archive permiso-socket-daemon`) y renombrar `plans/PLAN-009.md` a `plans/PLAN-009-COMPLETED.md`. Verificación: el delta queda fundido en `openspec/specs/deploy/spec.md`.
