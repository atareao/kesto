# deploy — delta

## Purpose

El despliegue de referencia de Kesto. Este delta separa el socket del daemon del proceso que sirve la API y añade el volumen compartido del socket IPC.

## MODIFIED Requirements

### Requirement: El compose reproduce el despliegue de referencia del §8

El repositorio SHALL incluir un fichero de compose (`compose.yaml`) con el servicio del §8: nombre `kesto`, `image: ghcr.io/atareao/kesto:latest`, `container_name: kesto-app`, `restart: always` y el puerto `"8080:8080"`, más un segundo servicio `kesto-motor` con la misma imagen y el binario `kesto-motor` como entrada.

El servicio de la API SHALL declarar las tres variables de entorno de arranque del §8 —`RUST_LOG=info,kesto=debug`, `DATABASE_URL=sqlite:///app/data/kesto.db` y `BIND_ADDRESS=0.0.0.0:8080`— y SHALL montar tres volúmenes: los dos directorios a respaldar, `/var/www:/backup_sources/var_www:ro` y `/etc/nginx:/backup_sources/nginx_config:ro`, y la persistencia `kesto_data:/app/data`. El montaje de `/var/run/docker.sock` NO SHALL declararse en este servicio. Ambos servicios SHALL compartir un volumen para el socket IPC configurado con `KESTO_MOTOR_SOCKET`.

El servicio motor SHALL declarar `KESTO_API_UID` con el UID del proceso de la API, SHALL montar `/var/run/docker.sock` en solo lectura y SHALL correr con usuario no root. El fichero SHALL documentar que el socket `:ro` sigue siendo equivalente a root y que la rebaja real es la separación de procesos.

El servicio de la API SHALL declarar además las cuatro variables del modo OIDC —`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET` y `OIDC_REDIRECT_URL`— y `KESTO_SECRET` con la forma `${VAR:?}`, de modo que `podman compose up` y su validación `compose config` SHALL negarse a levantar el servicio —sin crear el contenedor— cuando falte cualquiera de ellas: la plantilla de producción NO SHALL poder arrancar en abierto, y `BIND_ADDRESS=0.0.0.0:8080` solo es legítimo con proveedor de identidad. La plantilla NO SHALL declarar `KESTO_ALLOW_INSECURE`.

El fichero SHALL declarar el volumen nombrado `kesto_data` y el volumen compartido del socket IPC. Los tres montajes del host que no son el volumen interno SHALL ser de solo lectura, porque son fuentes de copia y no destinos de escritura.

#### Scenario: El servicio del §8 está declarado
- **GIVEN** el fichero `compose.yaml`
- **WHEN** se lee la definición del servicio `kesto`
- **THEN** SHALL declarar `image: ghcr.io/atareao/kesto:latest`, `container_name: kesto-app` y `restart: always`
- **AND** SHALL publicar el puerto `"8080:8080"`

#### Scenario: Las variables de entorno del §8 están presentes
- **GIVEN** el servicio `kesto` del `compose.yaml`
- **WHEN** se leen sus variables de entorno
- **THEN** SHALL declarar `RUST_LOG=info,kesto=debug`, `DATABASE_URL=sqlite:///app/data/kesto.db` y `BIND_ADDRESS=0.0.0.0:8080`

#### Scenario: Los volúmenes del §8 están montados
- **GIVEN** el servicio `kesto` del `compose.yaml`
- **WHEN** se leen sus montajes
- **THEN** SHALL montar los dos directorios a respaldar en solo lectura
- **AND** SHALL montar el volumen nombrado `kesto_data` en `/app/data`
- **AND** el fichero SHALL declarar el volumen `kesto_data`
- **AND** el socket del daemon SHALL montarse solo en el servicio `kesto-motor`

#### Scenario: La plantilla de producción no arranca sin proveedor de identidad
- **GIVEN** el fichero `compose.yaml` y un entorno sin las variables `OIDC_*` ni `KESTO_SECRET`
- **WHEN** se valida con `compose config`
- **THEN** SHALL negarse a levantar el servicio
- **AND** NO SHALL crear el contenedor

#### Scenario: La plantilla no arranca sin la URL de retorno
- **GIVEN** el fichero `compose.yaml` y un entorno con las tres `OIDC_*` y `KESTO_SECRET` pero sin `OIDC_REDIRECT_URL`
- **WHEN** se valida con `compose config`
- **THEN** SHALL negarse a levantar el servicio
- **AND** el motivo SHALL nombrar `OIDC_REDIRECT_URL`

### Requirement: El contenedor accede al socket del daemon con el grupo del host

El `compose.yaml` SHALL declarar el grupo suplementario del socket del daemon mediante `group_add`, tomando el gid de la variable de entorno `DOCKER_GID`, en el **servicio `kesto-motor`** —el único que monta el socket—, porque en un host Docker estándar el socket es `root:docker` con modo `srw-rw----` y el proceso del contenedor corre como uid/gid `10001`: sin el grupo del daemon el proceso no puede abrirlo.

`DOCKER_GID` SHALL tener un valor por defecto inofensivo —el propio gid del proceso (`10001`)—, de modo que un despliegue sin daemon (modo de desarrollo, o Podman rootless donde el socket pertenece al usuario) siga arrancando sin definir la variable. La variable SHALL documentarse en `.env.example`, con la forma de obtenerla (`getent group docker`).

#### Scenario: Con DOCKER_GID el servicio entra en el grupo del daemon
- **GIVEN** un despliegue con `DOCKER_GID` definida al gid del grupo del socket del host
- **WHEN** se valida el fichero con `podman compose config`
- **THEN** el servicio `kesto-motor` SHALL declarar ese gid en `group_add`
- **AND** el proceso del contenedor motor SHALL poder abrir `/var/run/docker.sock`

#### Scenario: Sin DOCKER_GID el arranque en desarrollo no se rompe
- **GIVEN** un despliegue con `KESTO_MASTER_KEY` y sin `DOCKER_GID`
- **WHEN** se valida el fichero con `podman compose config`
- **THEN** la validación SHALL pasar
- **AND** `group_add` SHALL quedar en el gid del propio proceso (`10001`), sin error

## ADDED Requirements

### Requirement: El socket del daemon solo está al alcance del proceso motor

En el despliegue de referencia, el servicio `kesto-app` SHALL NOT montar `/var/run/docker.sock` ni declarar `DOCKER_HOST`. El servicio `kesto-motor` SHALL montarlo con la opción `:ro` y SHALL ejecutarse con un usuario no root, y SHALL declarar `KESTO_API_UID` para verificar al par IPC. Un error, `panic` o inyección en la API SHALL NOT poder abrir el socket del daemon: la posesión del socket vive en otro proceso.

#### Scenario: Inspeccionar el compose muestra el socket solo en el motor
- **GIVEN** el `compose.yaml` de referencia
- **WHEN** se inspeccionan los montajes de los dos servicios
- **THEN** `kesto-app` SHALL NOT montar `/var/run/docker.sock`
- **AND** `kesto-motor` SHALL montarlo con `:ro` y con usuario no root
- **AND** `kesto-app` SHALL NOT declarar `DOCKER_HOST`

#### Scenario: API y motor comparten el socket IPC
- **GIVEN** el `compose.yaml` de referencia
- **WHEN** se inspeccionan los volúmenes compartidos
- **THEN** ambos servicios SHALL montar el volumen que contiene `KESTO_MOTOR_SOCKET`
- **AND** el servicio motor SHALL declarar `KESTO_API_UID` con el UID del proceso de la API
