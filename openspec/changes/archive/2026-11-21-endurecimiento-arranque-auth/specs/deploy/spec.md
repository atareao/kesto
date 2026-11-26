# deploy — delta

## Purpose

Cómo se construye, se configura y se verifica el despliegue de Kesto: imagen, compose de referencia y las variables que entran por el entorno. Este delta hace que la plantilla de producción no pueda arrancar sin proveedor de identidad y mueve la verificación de la imagen publicada a la forma que de verdad se publica.

## MODIFIED Requirements

### Requirement: El compose reproduce el despliegue de referencia del §8

El repositorio SHALL incluir un fichero de compose (`compose.yaml`) con el servicio del §8: nombre `kesto`, `image: ghcr.io/atareao/kesto:latest`, `container_name: kesto-app`, `restart: always` y el puerto `"8080:8080"`.

El servicio SHALL declarar las tres variables de entorno de arranque del §8 —`RUST_LOG=info,kesto=debug`, `DATABASE_URL=sqlite:///app/data/kesto.db` y `BIND_ADDRESS=0.0.0.0:8080`— y SHALL montar los cuatro volúmenes del §8:

- `/var/run/docker.sock:/var/run/docker.sock:ro` — el socket del daemon para el `exec` de bollard, en **solo lectura**;
- `/var/www:/backup_sources/var_www:ro` — un directorio del host a respaldar, en solo lectura;
- `/etc/nginx:/backup_sources/nginx_config:ro` — otro directorio a respaldar, en solo lectura;
- `kesto_data:/app/data` — la persistencia interna de la aplicación.

El servicio SHALL declarar además las tres variables del modo OIDC —`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID` y `OIDC_CLIENT_SECRET`— y `KESTO_SECRET` con la forma `${VAR:?}`, de modo que `podman compose up` y su validación `compose config` SHALL negarse a levantar el servicio —sin crear el contenedor— cuando falte cualquiera de ellas: la plantilla de producción NO SHALL poder arrancar en abierto, y `BIND_ADDRESS=0.0.0.0:8080` solo es legítimo con proveedor de identidad. La plantilla NO SHALL declarar `KESTO_ALLOW_INSECURE`.

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

## REMOVED Requirements

### Requirement: El despliegue arranca y se verifica en modo de desarrollo

**Reason**: el modo de desarrollo ya no puede verificar un puerto publicado: exige el permiso explícito y un bind de loopback, y con el proceso atado a loopback el reenvío del puerto no alcanza al servicio. La verificación se mueve a la forma de producción, que es la que se publica.

**Migration**: la verificación de la imagen publicada pasa a `El despliegue se verifica en la forma de producción`, que conserva la comprobación de la persistencia del volumen `kesto_data`. El modo de desarrollo sigue verificado por el binario: el e2e y el arnés de pruebas declaran `KESTO_ALLOW_INSECURE=1`.

### Requirement: La configuración y los secretos entran por el entorno y nunca en claro

**Reason**: el requisito fijaba que una credencial de menos de 32 caracteres «avisa y arranca igualmente». Con `KESTO_MASTER_KEY` corta —o vacía— la clave del cifrado en reposo es forzable, y con `KESTO_SECRET` corta la sesión es forjable: el aviso deja el sistema en producción con los secretos al alcance de quien lea el fichero. El contrato completo, con la credencial débil como puerta de arranque, se reescribe en el requisito añadido «La configuración y los secretos entran por el entorno, y una credencial débil no arranca».

**Migration**: el requisito se sustituye por uno nuevo que conserva lo que sigue vigente —el `compose` exigiendo `${KESTO_MASTER_KEY:?}`, el OIDC como un todo o nada, `KESTO_SECRET` obligatoria en modo OIDC, la rotación y el vacío que no cae al defecto— y añade el permiso explícito del modo de desarrollo. Un despliegue con credencial corta dejará de arrancar al actualizarse: ver `docs/ENTORNO.md`.

## ADDED Requirements
### Requirement: La configuración y los secretos entran por el entorno, y una credencial débil no arranca

`KESTO_MASTER_KEY` SHALL ser obligatoria **siempre** y SHALL llegar al contenedor por el entorno —el `.env` del despliegue o un gestor de secretos del host—, **nunca** escrita en la imagen ni en el `compose.yaml` versionado: una clave en una capa de la imagen o en un fichero que se sube al repositorio es una clave que hay que tratar como filtrada. El `compose.yaml` SHALL exigir la variable con la forma `${KESTO_MASTER_KEY:?...}`, de modo que `podman compose up` —y su validación `compose config`— SHALL negarse a levantar el servicio y NO SHALL crear el contenedor cuando falte; el binario, además, SHALL fallar si la variable no está definida en su propio proceso.

`KESTO_MASTER_KEY` es la clave de la que se deriva la del cifrado en reposo (`repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`), así que SHALL tratarse como un secreto de larga vida: **rotarla deja ilegibles los secretos ya guardados**.

Las tres variables que gobiernan el modo OIDC —`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID` y `OIDC_CLIENT_SECRET`— SHALL ser **un todo o nada**: con las tres presentes el modo es OIDC, con ninguna el modo es el de desarrollo, y con **unas sí y otras no** el arranque SHALL fallar con un error claro en vez de degradarse en silencio. En modo OIDC, `KESTO_SECRET` SHALL ser obligatoria —es la clave con la que se firma la sesión— y rotarla SHALL invalidar las sesiones vivas. `OIDC_REDIRECT_URL` configurada sola, sin las otras tres, SHALL ignorarse.

Una credencial del despliegue con menos de **32 caracteres** —vacía incluida— SHALL impedir el arranque: es la clave del cifrado en reposo y de la firma de sesión, y una débil deja los secretos guardados al alcance de quien lea el fichero. La ausencia de `KESTO_SECRET` NO SHALL confundirse con una credencial débil: sin proveedor de identidad no hace falta. Las variables que el binario lee **sin** prefijo —`RUST_LOG`, `BIND_ADDRESS` y `DATABASE_URL`— conservan su nombre tal cual, y una `BIND_ADDRESS` o un `RUST_LOG` **presentes pero vacíos** SHALL ser error de arranque, **no** un retorno al valor por defecto.

El modo de desarrollo —sin proveedor de identidad— SHALL exigir el permiso explícito de `KESTO_ALLOW_INSECURE` y una dirección de escucha de loopback, así que NO SHALL poder publicar el puerto: un despliegue que pierde las `OIDC_*` SHALL negarse a arrancar en abierto.

#### Scenario: Sin la clave maestra no hay arranque
- **GIVEN** una puesta en marcha del servicio sin `KESTO_MASTER_KEY`
- **WHEN** se ejecuta `podman compose up` (o su validación `compose config`)
- **THEN** SHALL negarse a levantar el servicio por falta de la clave maestra
- **AND** NO SHALL crearse el contenedor

#### Scenario: El OIDC es un todo o nada
- **GIVEN** un despliegue con `OIDC_ISSUER_URL` y `OIDC_CLIENT_ID` pero sin `OIDC_CLIENT_SECRET`
- **WHEN** el proceso arranca
- **THEN** SHALL fallar por configuración OIDC incompleta
- **AND** NO SHALL arrancar en modo de desarrollo degradado

#### Scenario: En modo OIDC el secreto de sesión es obligatorio
- **GIVEN** un despliegue con las tres variables `OIDC_*` y sin `KESTO_SECRET`
- **WHEN** el proceso arranca
- **THEN** SHALL fallar por falta de la clave de firma de sesión

#### Scenario: Una credencial corta no arranca
- **GIVEN** un despliegue cuya clave maestra tiene menos de 32 caracteres
- **WHEN** el proceso arranca
- **THEN** SHALL fallar nombrando la variable y el mínimo
- **AND** NO SHALL arrancar

#### Scenario: Sin proveedor de identidad no se publica el puerto
- **GIVEN** un despliegue con `KESTO_MASTER_KEY` y sin ninguna variable `OIDC_*`
- **WHEN** el proceso arranca sin el permiso explícito
- **THEN** SHALL fallar en alto
- **AND** NO SHALL escuchar en una dirección alcanzable desde otra interfaz

#### Scenario: Una variable vacía no cae al defecto
- **GIVEN** un despliegue con `BIND_ADDRESS=` presente pero vacía
- **WHEN** el proceso arranca
- **THEN** SHALL fallar por configuración inválida
- **AND** NO SHALL escuchar en una dirección por defecto

#### Scenario: Ningún secreto queda escrito en la imagen ni en el compose
- **GIVEN** el `Dockerfile` y el `compose.yaml` versionados
- **WHEN** se busca el valor de la clave maestra, de `KESTO_SECRET` o de las `OIDC_*`
- **THEN** NO SHALL aparecer ningún valor de secreto
- **AND** SHALL entrar al contenedor solo por el entorno

### Requirement: El despliegue se verifica en la forma de producción

La imagen publicada SHALL verificarse **con proveedor de identidad**: el contenedor SHALL arrancar con las tres variables `OIDC_*`, `KESTO_SECRET`, `BIND_ADDRESS=0.0.0.0:8080` y el puerto publicado, y `GET /api/v1/health` —público— SHALL contestar `200` con `auth_mode: "oidc"` consultado desde fuera del contenedor. El descubrimiento del proveedor NO SHALL ser requisito para arrancar: un emisor inalcanzable solo SHALL avisar, así que la verificación NO SHALL necesitar un proveedor real.

El modo de desarrollo NO SHALL verificarse con el puerto publicado: con el proceso atado a loopback —lo único que el modo sin autenticación permite— el reenvío del puerto no alcanza al servicio. Ese modo se verifica donde se usa: el binario del e2e y el arnés de pruebas, que declaran el permiso explícito.

La base SQLite SHALL vivir en el volumen nombrado `kesto_data` montado en `/app/data`, de modo que SHALL sobrevivir a la parada y recreación del contenedor: los datos de un despliegue SHALL persistir aunque el contenedor efímero se reemplace.

#### Scenario: El contenedor arranca con proveedor de identidad
- **GIVEN** un despliegue con `KESTO_MASTER_KEY`, `KESTO_SECRET` y las tres variables `OIDC_*`
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
