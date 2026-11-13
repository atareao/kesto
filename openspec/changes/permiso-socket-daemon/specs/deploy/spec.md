# Spec Delta

## Purpose

El empaquetado y el despliegue de Kesto: la imagen de contenedor que reúne el binario y el panel compilado sin toolchain y sin privilegios, el fichero de compose del §8 que la pone en marcha, la forma en que la configuración y los secretos entran en el contenedor, y la verificación de que el artefacto arranca y sirve de verdad. Fija el contrato entre lo que se construye, lo que se despliega y lo que el operador tiene que proveer.

## ADDED Requirements

### Requirement: El contenedor accede al socket del daemon con el grupo del host

El `compose.yaml` SHALL declarar el grupo suplementario del socket del daemon mediante `group_add`, tomando el gid de la variable de entorno `DOCKER_GID`, porque en un host Docker estándar el socket es `root:docker` con modo `srw-rw----` y el proceso del contenedor corre como uid/gid `10001`: sin el grupo del daemon el proceso no puede abrirlo.

`DOCKER_GID` SHALL tener un valor por defecto inofensivo —el propio gid del proceso (`10001`)—, de modo que un despliegue sin daemon (modo de desarrollo, o Podman rootless donde el socket pertenece al usuario) siga arrancando sin definir la variable. La variable SHALL documentarse en `.env.example`, con la forma de obtenerla (`getent group docker`).

#### Scenario: Con DOCKER_GID el servicio entra en el grupo del daemon
- **GIVEN** un despliegue con `DOCKER_GID` definida al gid del grupo del socket del host
- **WHEN** se valida el fichero con `podman compose config`
- **THEN** el servicio `kesto` SHALL declarar ese gid en `group_add`
- **AND** el proceso del contenedor SHALL poder abrir `/var/run/docker.sock`

#### Scenario: Sin DOCKER_GID el arranque en desarrollo no se rompe
- **GIVEN** un despliegue con `KESTO_MASTER_KEY` y sin `DOCKER_GID`
- **WHEN** se valida el fichero con `podman compose config`
- **THEN** la validación SHALL pasar
- **AND** `group_add` SHALL quedar en el gid del propio proceso (`10001`), sin error
