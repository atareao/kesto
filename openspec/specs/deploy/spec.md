# deploy Specification

## Purpose
El empaquetado y el despliegue de Kesto: la imagen de contenedor que reúne el binario y el panel compilado sin toolchain y sin privilegios, el fichero de compose del §8 que la pone en marcha, la forma en que la configuración y los secretos entran en el contenedor, y la verificación de que el artefacto arranca y sirve de verdad. Fija el contrato entre lo que se construye, lo que se despliega y lo que el operador tiene que proveer.

## Requirements

### Requirement: La imagen se construye en etapas y el runtime no lleva toolchain

El `Dockerfile` SHALL construirse en **etapas** en el orden del §7 —compilar el panel de `ui/`, compilar el binario de release de Rust, y una etapa final de runtime—. La imagen final SHALL contener el binario compilado y el panel compilado (`ui/dist`), y NO SHALL contener el toolchain de Rust (`cargo`, `rustc`), ni `node`, ni `pnpm`, ni el árbol de fuentes del backend (`src/`), ni las dependencias del panel (`node_modules`), ni sus artefactos intermedios.

La razón es doble. El tamaño: arrastrar el toolchain de Rust y `node_modules` a la imagen que se ejecuta multiplica su peso sin aportar nada en tiempo de ejecución. La superficie de ataque: una imagen que lleva un compilador y un gestor de paquetes dentro pone herramientas de compilación al alcance de cualquiera que consiga ejecutar código en ella, y aquí no hacen ninguna falta.

El binario SHALL encontrar el panel compilado en la ruta desde la que el servidor lo sirve, de modo que la imagen entregue el producto completo y no un backend sin interfaz.

#### Scenario: La imagen entrega el producto completo
- **GIVEN** la imagen construida con `just build`
- **WHEN** se inspecciona su sistema de ficheros
- **THEN** SHALL contener el binario de Kesto
- **AND** SHALL contener el panel compilado en la ruta de `ui/dist`

#### Scenario: El runtime no arrastra el toolchain
- **GIVEN** la imagen construida
- **WHEN** se busca un compilador y un gestor de paquetes en la etapa final
- **THEN** `cargo`, `rustc`, `node` y `pnpm` NO SHALL estar disponibles
- **AND** NO SHALL existir el árbol de fuentes ni `node_modules`

### Requirement: El runtime corre sin privilegios y sirve panel y API en el mismo origen

La etapa final SHALL ejecutar el proceso con un usuario **sin privilegios** —uid/gid `10001`, el mismo que asume la receta `just run`—, NO SHALL correr como `root`, y SHALL escuchar en un único puerto (`8080`) por el que sirve **el panel y la API en el mismo origen**: el documento del panel y sus assets en `/`, y la superficie de la API bajo `/api/v1`.

Un solo origen es lo que hace que la cookie de sesión y el cliente del panel funcionen sin configurar CORS, y es lo que `api-shell` ya fija para el servido estático. Que el proceso no sea `root` es lo que impide que un fallo en el binario —que va a hablar con el socket del daemon de contenedores del host— se convierta en un fallo con privilegios de administrador.

#### Scenario: El proceso no corre como root
- **GIVEN** el contenedor arrancado
- **WHEN** se consulta el usuario efectivo del proceso
- **THEN** SHALL ser el usuario sin privilegios `10001`
- **AND** NO SHALL ser `root`

#### Scenario: Panel y API comparten origen
- **GIVEN** el contenedor arrancado y escuchando en el puerto `8080`
- **WHEN** se pide `GET /` y `GET /api/v1/health` contra el mismo host y puerto
- **THEN** SHALL contestar `200` a las dos
- **AND** la primera SHALL ser el documento del panel y la segunda la envoltura de la API

### Requirement: El compose reproduce el despliegue de referencia del §8

El repositorio SHALL incluir un fichero de compose (`compose.yaml`) con el servicio del §8: nombre `kesto`, `image: ghcr.io/atareao/kesto:latest`, `container_name: kesto-app`, `restart: always` y el puerto `"8080:8080"`.

El servicio SHALL declarar las tres variables de entorno de arranque del §8 —`RUST_LOG=info,kesto=debug`, `DATABASE_URL=sqlite:///app/data/kesto.db` y `BIND_ADDRESS=0.0.0.0:8080`— y SHALL montar los cuatro volúmenes del §8:

- `/var/run/docker.sock:/var/run/docker.sock:ro` — el socket del daemon para el `exec` de bollard, en **solo lectura**;
- `/var/www:/backup_sources/var_www:ro` — un directorio del host a respaldar, en solo lectura;
- `/etc/nginx:/backup_sources/nginx_config:ro` — otro directorio a respaldar, en solo lectura;
- `kesto_data:/app/data` — la persistencia interna de la aplicación.

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

### Requirement: La configuración y los secretos entran por el entorno y nunca en claro

`KESTO_MASTER_KEY` SHALL ser obligatoria **siempre** y SHALL llegar al contenedor por el entorno —el `.env` del despliegue o un gestor de secretos del host—, **nunca** escrita en la imagen ni en el `compose.yaml` versionado: una clave en una capa de la imagen o en un fichero que se sube al repositorio es una clave que hay que tratar como filtrada. El `compose.yaml` SHALL exigir la variable con la forma `${KESTO_MASTER_KEY:?...}`, de modo que `podman compose up` —y su validación `compose config`— SHALL negarse a levantar el servicio y NO SHALL crear el contenedor cuando falte; el binario, además, SHALL fallar si la variable no está definida en su propio proceso.

`KESTO_MASTER_KEY` es la clave de la que se deriva la del cifrado en reposo (`repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`), así que SHALL tratarse como un secreto de larga vida: **rotarla deja ilegibles los secretos ya guardados**.

Las tres variables que gobiernan el modo OIDC —`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID` y `OIDC_CLIENT_SECRET`— SHALL ser **un todo o nada**: con las tres presentes el modo es OIDC, con ninguna el modo es el de desarrollo, y con **unas sí y otras no** el arranque SHALL fallar con un error claro en vez de degradarse en silencio. En modo OIDC, `KESTO_SECRET` SHALL ser obligatoria —es la clave con la que se firma la sesión— y rotarla SHALL invalidar las sesiones vivas. `OIDC_REDIRECT_URL` configurada sola, sin las otras tres, SHALL ignorarse.

Una credencial del despliegue con menos de **32 caracteres** SHALL avisar y **arrancar igualmente**: es un aviso de higiene, no una puerta de arranque. Las variables que el binario lee **sin** prefijo —`RUST_LOG`, `BIND_ADDRESS` y `DATABASE_URL`— conservan su nombre tal cual, y una `BIND_ADDRESS` o un `RUST_LOG` **presentes pero vacíos** SHALL ser error de arranque, **no** un retorno al valor por defecto.

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

#### Scenario: Una credencial corta avisa pero no detiene el arranque
- **GIVEN** un despliegue cuya clave maestra tiene menos de 32 caracteres
- **WHEN** el proceso arranca
- **THEN** SHALL registrar un aviso sobre la longitud de la credencial
- **AND** SHALL arrancar igualmente

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

### Requirement: El despliegue arranca y se verifica en modo de desarrollo

Con solo `KESTO_MASTER_KEY` y sin ninguna variable `OIDC_*`, el contenedor SHALL arrancar en el modo de **desarrollo** (`Disabled`): `GET /api/v1/health` SHALL contestar `200` con `auth_mode: "disabled"` y `GET /` SHALL servir el documento del panel con `200` y `text/html`. Es la verificación de que la imagen y el compose funcionan de verdad, y no solo de que compilan.

La base SQLite SHALL vivir en el volumen nombrado `kesto_data` montado en `/app/data`, de modo que SHALL sobrevivir a la parada y recreación del contenedor: los datos de un despliegue SHALL persistir aunque el contenedor efímero se reemplace.

#### Scenario: El contenedor arranca solo con la clave maestra
- **GIVEN** un despliegue con `KESTO_MASTER_KEY` y sin ninguna `OIDC_*`
- **WHEN** se levanta el servicio con compose
- **THEN** el contenedor SHALL arrancar
- **AND** SHALL escuchar en el puerto `8080`

#### Scenario: La salud declara el modo de desarrollo
- **GIVEN** el contenedor recién arrancado en modo de desarrollo
- **WHEN** se pide `GET /api/v1/health`
- **THEN** SHALL contestar `200`
- **AND** el cuerpo SHALL declarar `auth_mode: "disabled"`

#### Scenario: El mismo contenedor sirve el panel
- **GIVEN** el contenedor recién arrancado
- **WHEN** se pide `GET /`
- **THEN** SHALL contestar `200`
- **AND** el tipo de contenido SHALL ser `text/html`

#### Scenario: La base sobrevive a la recreación del contenedor
- **GIVEN** un despliegue con datos ya escritos y el volumen `kesto_data`
- **WHEN** se para y se recrea el contenedor
- **THEN** los datos SHALL seguir presentes al volver a arrancar
- **AND** NO SHALL haberse perdido el fichero de base de datos de `/app/data`

### Requirement: El contenedor declara su sonda de salud

La imagen SHALL declarar un `HEALTHCHECK` que consulte `GET /api/v1/health` en la propia imagen, de modo que el daemon pueda distinguir «arrancado» de «en pie» sin ayuda externa. La construcción local con Podman SHALL forzar el formato de imagen `docker` para que la sonda no se descarte en silencio.

#### Scenario: La imagen lleva su sonda de salud
- **GIVEN** la imagen construida en local con `just build`
- **WHEN** se inspecciona su configuración
- **THEN** SHALL declarar un `HEALTHCHECK` sobre `/api/v1/health`
- **AND** el contenedor SHALL llegar al estado `healthy` cuando el servicio responde

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

### Requirement: La etiqueta `latest` de la imagen apunta siempre a la release más alta

La publicación de la imagen en GHCR SHALL mantener `latest` resolviendo **siempre** al manifiesto de la versión semver más alta publicada, con independencia del orden en que terminen las construcciones. El workflow de release NO SHALL publicar `latest` cuando el tag que se está publicando no sea esa versión más alta, porque el `compose.yaml` del §8 usa `image: ghcr.io/atareao/kesto:latest` y un despliegue que siga esa etiqueta recibiría una versión anterior.

El workflow SHALL serializar las construcciones de imagen entre releases, de modo que dos releases no compitan en paralelo por la misma etiqueta y el resultado no dependa de cuál acabe antes.

#### Scenario: `latest` apunta a la versión más alta
- **GIVEN** las releases `0.3.3` y `0.4.0` publicadas y sus construcciones terminadas
- **WHEN** se inspecciona `ghcr.io/atareao/kesto:latest`
- **THEN** SHALL resolver al mismo digest que `ghcr3.io/atareao/kesto:0.4.0`
- **AND** NO SHALL resolver al digest de `0.3.3`

#### Scenario: Una release no-máxima no degrada `latest`
- **GIVEN** `latest` publicado en 0.4.0
- **WHEN** termina la construcción de una release anterior (por ejemplo `0.3.3`)
- **THEN** el workflow NO SHALL publicar `latest` desde esa release
- **AND** `latest` SHALL seguir resolviendo al digest de 0.4.0

#### Scenario: Builds solapados no dejan `latest` en una versión antigua
- **GIVEN** dos construcciones de release solapadas en el tiempo
- **WHEN** ambas terminan
- **THEN** `latest` SHALL quedar en la versión más alta de las dos
- **AND** NO SHALL quedar en la que terminó última por azar

### Requirement: La imagen multi-arquitectura se construye nativamente y se publica como manifest list

El workflow de release SHALL construir cada arquitectura de la imagen (`linux/amd64` y `linux/arm64`) en un runner de la **misma** arquitectura —amd64 en `ubuntu-26.04` y arm64 en `ubuntu-26.04-arm`—, de modo que la capa `arm64` NO SHALL compilarse por emulación. Cada construcción SHALL publicar su imagen por digest y un paso posterior SHALL ensamblar ambos digests en un único manifest list.

#### Scenario: amd64 y arm64 se construyen sin emulación
- **GIVEN** una release `vX.Y.Z` publicada
- **WHEN** arranca el job de imagen
- **THEN** la arquitectura amd64 SHALL construirse en un runner x64
- **AND** la arquitectura arm64 SHALL construirse en el runner nativo `ubuntu-26.04-arm`
- **AND** la construcción arm64 NO SHALL depender de emulación

#### Scenario: el manifest publicado contiene las dos arquitecturas
- **GIVEN** la release `vX.Y.Z` terminada
- **WHEN** se inspecciona `ghcr.io/atareao/kesto:X.Y.Z`
- **THEN** el manifest SHALL contener `linux/amd64` y `linux/arm64`
- **AND** `X.Y.Z`, `X.Y` y `latest` SHALL resolver al mismo manifest list

#### Scenario: el contrato de tags no cambia
- **GIVEN** una release `vX.Y.Z` que es la más alta publicada
- **WHEN** termina la publicación
- **THEN** los tags SHALL ser `X.Y.Z`, `X.Y` y `latest`, igual que antes
- **AND** NO SHALL publicarse etiquetas nuevas ni retirarse las existentes

### Requirement: La compilación Rust se cachea entre ejecuciones

El workflow SHALL reutilizar los artefactos de compilación Rust entre ejecuciones mediante caché persistente: una capa de dependencias en el `Dockerfile` y **`sccache`** con backend de caché de GitHub Actions. Una release NO SHALL recompilar las dependencias que no hayan cambiado.

#### Scenario: las dependencias se reutilizan entre releases
- **GIVEN** dos releases consecutivas con las dependencias sin cambios
- **WHEN** se construye la segunda
- **THEN** las dependencias Rust SHALL reutilizarse de la caché
- **AND** el tiempo de compilación SHALL reducirse respecto a un build en frío

#### Scenario: sccache reutiliza artefactos ante un cambio de código
- **GIVEN** la caché de `sccache` activada
- **WHEN** cambia solo el código del crate y no sus dependencias
- **THEN** `sccache` SHALL reutilizar los artefactos de las dependencias
- **AND** NO SHALL recompilarlas

### Requirement: El guard de `latest` y el smoke test siguen vigentes

El workflow de release SHALL mantener el guard que publica `latest` solo cuando el tag es la versión semver más alta, con independencia del orden en que terminen las construcciones, y SHALL ejecutar el smoke test de arranque sobre la imagen publicada. La construcción por arquitectura NO SHALL alterar esa lógica.

#### Scenario: una release no-máxima no publica `latest`
- **GIVEN** `latest` publicado en `X.Y.Z`
- **WHEN** termina la construcción de una release anterior
- **THEN** el workflow NO SHALL publicar `latest` desde esa release
- **AND** `latest` SHALL seguir resolviendo a `X.Y.Z`

#### Scenario: la imagen publicada arranca
- **GIVEN** la imagen `X.Y.Z` publicada
- **WHEN** se arranca el contenedor con la configuración de prueba
- **THEN** SHALL aplicar las migraciones y escuchar en el puerto HTTP
- **AND** el smoke test SHALL pasar en el job amd64

### Requirement: La imagen publicada es funcionalmente idéntica

La imagen SHALL contener el mismo binario y el mismo panel compilado que antes, sin toolchain en el runtime y corriendo sin privilegios, de modo que la aceleración del build NO SHALL cambiar el artefacto entregado.

#### Scenario: el runtime sigue sin toolchain y sin privilegios
- **GIVEN** la imagen `X.Y.Z` publicada
- **WHEN** se inspecciona el runtime
- **THEN** NO SHALL contener el toolchain de compilación
- **AND** el proceso SHALL correr con el usuario sin privilegios

### Requirement: La imagen contiene el binario real del crate

El `Dockerfile` SHALL garantizar que el binario de release se compila a partir del código fuente real y NO SHALL publicar un artefacto de la caché de compilación. Cuando la etapa de dependencias use el patrón dummy, el build SHALL invalidar la huella de Cargo del crate tras copiar el código, de modo que Cargo lo recompile.

#### Scenario: la imagen arranca con el binario real
- **GIVEN** el `Dockerfile` con caché de dependencias por el patrón dummy
- **WHEN** se construye y se arranca la imagen
- **THEN** el binario SHALL corresponder al código fuente real
- **AND** el contenedor SHALL escuchar en el puerto HTTP (smoke test superado)

#### Scenario: el guard estático detecta la regresión
- **GIVEN** `scripts/release-image-workflow.test.sh`
- **WHEN** el `Dockerfile` copia el código real sin invalidar la huella de Cargo
- **THEN** el test SHALL fallar
