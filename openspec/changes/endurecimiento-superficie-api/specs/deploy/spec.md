# deploy — delta

## Purpose

Cómo se construye, se configura y se verifica el despliegue de Kesto: imagen, compose de referencia y las variables que entran por el entorno. Este delta declara `OIDC_REDIRECT_URL` como la cuarta pieza obligatoria del modo OIDC, en el compose y en la verificación de la imagen publicada.

## MODIFIED Requirements

### Requirement: El compose reproduce el despliegue de referencia del §8

El repositorio SHALL incluir un fichero de compose (`compose.yaml`) con el servicio del §8: nombre `kesto`, `image: ghcr.io/atareao/kesto:latest`, `container_name: kesto-app`, `restart: always` y el puerto `"8080:8080"`.

El servicio SHALL declarar las tres variables de entorno de arranque del §8 —`RUST_LOG=info,kesto=debug`, `DATABASE_URL=sqlite:///app/data/kesto.db` y `BIND_ADDRESS=0.0.0.0:8080`— y SHALL montar los cuatro volúmenes del §8:

- `/var/run/docker.sock:/var/run/docker.sock:ro` — el socket del daemon para el `exec` de bollard, en **solo lectura**;
- `/var/www:/backup_sources/var_www:ro` — un directorio del host a respaldar, en solo lectura;
- `/etc/nginx:/backup_sources/nginx_config:ro` — otro directorio a respaldar, en solo lectura;
- `kesto_data:/app/data` — la persistencia interna de la aplicación.

El servicio SHALL declarar además las cuatro variables del modo OIDC —`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET` y `OIDC_REDIRECT_URL`— y `KESTO_SECRET` con la forma `${VAR:?}`, de modo que `podman compose up` y su validación `compose config` SHALL negarse a levantar el servicio —sin crear el contenedor— cuando falte cualquiera de ellas: la plantilla de producción NO SHALL poder arrancar en abierto, y `BIND_ADDRESS=0.0.0.0:8080` solo es legítimo con proveedor de identidad. La plantilla NO SHALL declarar `KESTO_ALLOW_INSECURE`.

El fichero SHALL declarar el volumen nombrado `kesto_data`. Los tres montajes del host que no son el volumen interno SHALL ser de solo lectura, porque son fuentes de copia y no destinos de escritura.

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
- **THEN** SHALL montar el socket del daemon y los dos directorios a respaldar en solo lectura
- **AND** SHALL montar el volumen nombrado `kesto_data` en `/app/data`
- **AND** el fichero SHALL declarar el volumen `kesto_data`

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

### Requirement: El despliegue se verifica en la forma de producción

La imagen publicada SHALL verificarse **con proveedor de identidad**: el contenedor SHALL arrancar con las tres variables `OIDC_*`, `OIDC_REDIRECT_URL` por `https`, `KESTO_SECRET`, `BIND_ADDRESS=0.0.0.0:8080` y el puerto publicado, y `GET /api/v1/health` —público— SHALL contestar `200` con `auth_mode: "oidc"` consultado desde fuera del contenedor. El descubrimiento del proveedor NO SHALL ser requisito para arrancar: un emisor inalcanzable solo SHALL avisar, así que la verificación NO SHALL necesitar un proveedor real.

El modo de desarrollo NO SHALL verificarse con el puerto publicado: con el proceso atado a loopback —lo único que el modo sin autenticación permite— el reenvío del puerto no alcanza al servicio. Ese modo se verifica donde se usa: el binario del e2e y el arnés de pruebas, que declaran el permiso explícito.

La base SQLite SHALL vivir en el volumen nombrado `kesto_data` montado en `/app/data`, de modo que SHALL sobrevivir a la parada y recreación del contenedor: los datos de un despliegue SHALL persistir aunque el contenedor efímero se reemplace.

#### Scenario: El contenedor arranca con proveedor de identidad
- **GIVEN** un despliegue con `KESTO_MASTER_KEY`, `KESTO_SECRET`, las tres variables `OIDC_*` y `OIDC_REDIRECT_URL` por `https`
- **WHEN** se levanta el servicio con el puerto publicado
- **THEN** el contenedor SHALL arrancar
- **AND** SHALL escuchar en `0.0.0.0:8080`

#### Scenario: La salud declara el modo de producción
- **GIVEN** el contenedor recién arrancado con proveedor de identidad
- **WHEN** se pide `GET /api/v1/health` desde fuera del contenedor
- **THEN** SHALL contestar `200`
- **AND** el cuerpo SHALL declarar `auth_mode: "oidc"`

#### Scenario: El mismo contenedor sirve el panel
- **GIVEN** el contenedor recién arrancado
- **WHEN** se pide `GET /`
- **THEN** SHALL contestar `200`
- **AND** el tipo de contenido SHALL ser `text/html`

#### Scenario: Sin permiso explícito el contenedor no queda abierto
- **GIVEN** un despliegue con `KESTO_MASTER_KEY` y sin ninguna variable `OIDC_*` ni `KESTO_ALLOW_INSECURE`
- **WHEN** se levanta el servicio
- **THEN** SHALL negarse a arrancar
- **AND** NO SHALL quedar la superficie abierta en el puerto publicado

#### Scenario: La base sobrevive a la recreación del contenedor
- **GIVEN** un despliegue con datos ya escritos y el volumen `kesto_data`
- **WHEN** se para y se recrea el contenedor
- **THEN** los datos SHALL seguir presentes al volver a arrancar
- **AND** NO SHALL haberse perdido el fichero de base de datos de `/app/data`
