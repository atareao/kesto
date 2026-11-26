# Design — SEC-06 (2/2): separación de privilegios motor/API

Decisiones y alternativas descartadas del change `seguridad-sec-06-separacion-privilegios-motor-api`. Alcance y contratos: `plans/PLAN-051.md`.

## D1 · Frontera de proceso, no de montaje ni de ruta

El socket montado `:ro` NO SHALL considerarse un privilegio menor: es un canal de control del daemon, y `POST /containers/create` con un montaje de `/` convierte al que lo posee en root del host. `FEATURE-040` cerró **dónde** se ejecuta (`exec` solo en contenedores con `io.kesto.backup-source=true`) y **qué rutas** se leen (`host_path`, `db_path`, `readlink -f`), pero no **quién** tiene el socket: el proceso API. Un fallo policía de memoria, un `unwrap` exploitable o una inyección en `argv` en cualquier handler llega hoy al daemon.

Se descarta el **proxy genérico de socket** con lista blanca de rutas HTTP: no inspecciona el cuerpo de `POST /containers/create`, luego no puede exigir la etiqueta ni impedir un montaje de `/`; no puede validar el `argv` de `POST /containers/{id}/exec`; y un `inspect` posterior tampoco evita que el contenedor creado ya exista. Filtrar rutas es filtrar la sintaxis y dejar la semántica entera en manos del atacante.

Se adopta un **segundo binario privilegiado**, `kesto-motor`, único poseedor de `DOCKER_HOST` y del socket del daemon, y un **IPC local tipado**. La API no puede pedir «un HTTP cualquiera al daemon»: solo puede pedir una de las seis operaciones de la lista blanca, con tipos construidos en Rust.

## D2 · `bollard` opcional: la frontera se comprueba al compilar

`bollard` deja de ser una dependencia incondicional y pasa a una dependencia opcional activada solo por la feature `motor`; `[[bin]] kesto-motor` lleva `required-features = ["motor"]`. `src/docker/cliente.rs` y `src/docker/mod.rs` permanecen en el lado motor: `conectar()`, `exigir_fuente_de_copia()`, `ejecutar()` y `ejecutar_con_entrada()` conservan su lógica actual.

Así la afirmación «la API no posee el cliente Docker» deja de ser una convención de código y pasa a ser una propiedad del artefacto: el binario API se compila sin la feature y su grafo no contiene `bollard` (verificación: `cargo tree --bin kesto -e normal | rg '^bollard| bollard '` sin resultados, además de `cargo check --locked --bin kesto`). Un reingreso accidental de `bollard` en código alcanzable por la API rompe la compilación o la comprobación del grafo, no solo una revisión.

## D3 · Framing del IPC: `u32` big-endian + JSON, tope de 1 MiB

El canal es un Unix domain socket en `KESTO_MOTOR_SOCKET` (defecto `/run/kesto/motor.sock`). Cada mensaje lleva una longitud de **4 bytes en big-endian** seguida de un cuerpo JSON UTF-8. Big-endian se elige porque es el orden de red y no depende de la arquitectura del host; `u32` acota el tamaño declarado al lector.

El cuerpo NO SHALL superar **1 MiB**. Al leer: longitud inválida, longitud mayor que el tope o cuerpo que no llega a la longitud declarada se descartan **sin pánico** y cerrando la conexión. El límite no es una preferencia de rendimiento: los volcados viajan por streaming directo entre el daemon y `rustic_core`, nunca por el IPC, así que ningún mensaje legítimo necesita acercarse al tope.

Los tipos `SolicitudMotor` / `RespuestaMotor` viven en `src/ipc/`. NO SHALL reenviarse HTTP crudo ni cabeceras: solo la lista blanca tipada.

## D4 · La credencial del par: `SO_PEERCRED` contra `KESTO_API_UID`

La posesión del path del socket no es autenticación: cualquier proceso local con permisos de escritura podría conectarse. El motor exige **`KESTO_API_UID`** —obligatorio: sin él NO SHALL arrancar— y obtiene la credencial del par con `SO_PEERCRED` al aceptar la conexión. Si el UID no coincide, se cierra **antes de leer el cuerpo**, sin procesar la solicitud y sin emitir ninguna llamada a `bollard`.

`KESTO_API_UID` es un UID numérico, no un nombre de usuario: los nombres se pueden reutilizar y resolver distinto en imagen y host. El motor no confía en ningún campo del mensaje para identificarse. La comprobación es local a Linux, que es el único despliegue soportado; si `SO_PEERCRED` no está disponible, el motor falla en alto en lugar de aceptar conexiones sin identificar.

## D5 · Lista blanca cerrada

`SolicitudMotor` tiene exactamente seis variantes: `Comprobar`, `CrearContenedor`, `IniciarContenedor`, `CrearExec`, `InspeccionarExec`, `IniciarExec`. Cualquier otra variante —incluida una que llegue por un mensaje JSON con un nombre desconocido— es un error tipado y NO SHALL llegar al daemon.

`RespuestaMotor` también es tipada. Los errores que vuelven a la API NO SHALL incluir la ruta del socket, el valor de `DOCKER_HOST` ni credenciales del `env`; la traducción a `AppError` mantiene el JSON de error de la casa. Un mensaje mal formado es un error de protocolo, no un `5xx` que filtra detalles del motor.

## D6 · Dónde viven `DOCKER_HOST` y `Orden.env`

`DOCKER_HOST` se lee, se recorta y se valida **solo en `kesto-motor`**: ausente o vacío cae a `unix:///var/run/docker.sock`; con espacios se usa el valor recortado. La conexión sigue siendo `Docker::connect_with_socket(socket, segundos, bollard::API_DEFAULT_VERSION)` —nunca `connect_with_defaults()`—. Un socket inexistente produce un error que lo nombra en el motor; NO SHALL impedir el arranque de la API, que no lo lee. En local el valor apunta al socket de Podman; en producción, al de Docker.

`Orden.env` —las credenciales que el volcado necesita dentro del contenedor— se construye en el motor a partir de los datos del mensaje IPC. Viaja por el canal, nunca por el entorno de la API ni por sus logs. `Debug` de `SolicitudMotor`, `RespuestaMotor` y `Orden` redacta `env` (y cualquier otro campo secreto) con `****`; el mensaje de error tampoco lo reproduce. Es la misma disciplina que ya se aplica al error de ejecución.

Consecuencia: un `panic` en la API puede filtrar el estado de la petición, pero no la credencial del daemon ni la del target.

## D7 · `argv` literal y comprobaciones dentro del motor

El `argv` de un `exec` se construye en el motor como una secuencia de cadenas literales y se pasa tal cual a bollard; NO SHALL interpretarse como una línea de shell ni como metacomandos. Un nombre de base con `;`, `$()` o un salto de línea permanece dentro de un único argumento.

La etiqueta `io.kesto.backup-source=true` se exige con `inspect_container` **antes** de crear el `exec`, en `exigir_fuente_de_copia()`, tanto al extraer como al restaurar. Las operaciones de la lista blanca no son una alternativa a la etiqueta: `IniciarExec` no puede invocarse sobre un contenedor que no haya pasado la comprobación en el mismo proceso motor. Las rutas confinadas siguen bajo `KESTO_BACKUP_ROOT`.

## D8 · Despliegue: el socket del daemon cambia de servicio

`kesto-app` NO SHALL montar `/var/run/docker.sock` y NO SHALL recibir `DOCKER_HOST`. `kesto-motor` lo monta `:ro`, con `user: "10001:10001"` (no root) y `group_add` desde `DOCKER_GID`, y declara `KESTO_API_UID`. Ambos servicios comparten un volumen para el socket IPC (`KESTO_MOTOR_SOCKET`). La imagen publicada contiene los dos binarios y el mismo `HEALTHCHECK` sigue en la API.

Coste aceptado: un despliegue propio que monte el socket en `kesto-app` deja de funcionar hasta mover el montaje; la migración se documenta en `docs/FEATURE-051-separacion-privilegios-motor-api.md`. En modo `Disabled` la API sigue sin capa de autenticación, pero el motor sigue exigiendo el UID del par.

## D9 · Lo que NO se protege

- **Compromiso total del motor**: quien puede ejecutar `kesto-motor` con el socket tiene el mismo poder que el daemon; la lista blanca protege el canal, no al proceso que ya es privilegiado.
- **`COPY … FROM PROGRAM`** en un volcado: el troceador y los clientes en modo restringido no ejecutan metacomandos, pero un volcado generado con esa cláusula puede contenerla como datos; cerrarlo exige cambiar la semántica de restore.
- **#175**: los errores de SQL dentro de una sección aún no se detectan (`psql` sin `ON_ERROR_STOP` sale con `0`).
- **#169**: la KDF de cifrado en reposo no cambia.
- **#182**: la resolución DNS del `endpoint` de repositorios no se fija.
- SEC-04, SEC-05 y SEC-06(1/2), y los privilegios de restauración que crean bases/roles: no se reabren.
