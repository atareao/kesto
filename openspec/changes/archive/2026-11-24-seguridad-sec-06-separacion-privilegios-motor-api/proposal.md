# Proposal — SEC-06 (2/2): separación de privilegios motor/API

Issue **#173** (campaña de seguridad **#156** / paraguas **#168**), segunda mitad de SEC-06. Primera mitad: **#162**, entregada como `FEATURE-040-confinamiento-frontera-motor`. Plan: `plans/PLAN-051.md`.

## Why

Hoy el mismo proceso que sirve la API posee `DOCKER_HOST` y habla con el daemon por `/var/run/docker.sock`. El montaje del socket es `:ro`, pero eso **no es una rebaja de privilegios**: el socket es una API, no un fichero; basta `POST /containers/create` con un montaje de `/` en el contenedor nuevo para leer y escribir todo el host. Un `panic`, un `unwrap` explotable o una inyección en `argv` en un handler de la API entregan hoy ese socket.

`FEATURE-040` cerró el **alcance** —en qué contenedor y con qué ruta se ejecuta: etiqueta `io.kesto.backup-source=true`, `KESTO_BACKUP_ROOT`, `readlink -f`— pero no el **privilegio**: los `exec` siguen corriendo con el mismo proceso API que tiene el daemon al alcance. Su propio documento dejó la mitad abierta y la registró como #173.

Se descarta un **proxy genérico de socket** con lista blanca de rutas: inspecciona la línea de petición, no el cuerpo, así que no puede exigir la etiqueta en `POST /containers/create`, ni validar el `argv` de un `exec`, ni distinguir un `create` legítimo de uno que monta `/`. Un proxy que solo filtra URLs deja la puerta abierta al mismo ataque con otra forma.

La frontera tiene que ser de **proceso**: la API deja de enlazar el cliente Docker y deja de tener el socket; un segundo binario privilegiado atiende una lista blanca cerrada de operaciones por un IPC local tipado.

## What Changes

- **Binario partido.** `kesto-motor` es un segundo binario con `bollard` y `DOCKER_HOST` en exclusiva. `bollard` pasa a dependencia opcional tras la feature `motor` y el binario de la API NO SHALL enlazarla: la frontera se comprueba en el árbol de dependencias, no solo por convención.
- **IPC local tipado.** Unix domain socket en `KESTO_MOTOR_SOCKET` (defecto `/run/kesto/motor.sock`), framing de `u32` big-endian + JSON, tope de 1 MiB. `SolicitudMotor` es una lista blanca cerrada de seis variantes: `Comprobar`, `CrearContenedor`, `IniciarContenedor`, `CrearExec`, `InspeccionarExec`, `IniciarExec`. No se reenvía HTTP crudo.
- **Credencial del par.** El motor exige `KESTO_API_UID` y contrasta el `SO_PEERCRED` del socket antes de procesar nada; un UID distinto se cierra sin lectura del cuerpo.
- **Confidencialidad.** `DOCKER_HOST` y el `env` de `Orden` se leen y construyen solo en el motor; viajan en el mensaje IPC, nunca en el entorno ni en los registros de la API. `Debug` de los mensajes IPC y de `Orden` redacta `env`; los errores al cliente no incluyen rutas de socket ni credenciales.
- **Comprobaciones dentro del motor.** La etiqueta de fuente de copia, las rutas confinadas y el `argv` literal se comprueban/construyen en `kesto-motor`, antes de cualquier llamada a `bollard`; la API solo envía la operación tipada.
- **Despliegue.** `compose.yaml` reparte los servicios: `kesto-app` pierde `/var/run/docker.sock`, `kesto-motor` lo monta `:ro` con usuario no root, y ambos comparten un volumen para el socket IPC. `DOCKER_GID` pasa a aplicar al servicio motor.
- **Documentación.** `docs/DEFINITION.md` §8 y `docs/ENTORNO.md` documentan el servicio nuevo y `KESTO_MOTOR_SOCKET` / `KESTO_API_UID`; `docs/FEATURE-051-separacion-privilegios-motor-api.md` se escribe al cerrar.

## Non-goals

- NO se protege contra el **compromiso total del proceso motor**: quien ejecuta `kesto-motor` con el socket puede hablar con el daemon. Se reduce la superficie que lo alcanza; no se inventa una barrera dentro del mismo privilegio.
- NO se cierra el residual `COPY … FROM PROGRAM` de un volcado restaurado: el código no lo interpreta, pero un producto que lo generó puede contenerlo.
- NO se toca **#175** (`ON_ERROR_STOP` de `psql`), **#169** (KDF) ni **#182** (resolución DNS del `endpoint` de repositorios).
- NO se reabren **SEC-04**, **SEC-05** ni la primera mitad de **SEC-06**: rutas, `readlink -f` y etiqueta siguen como están, solo que se ejecutan en el motor.
- NO cambia la API HTTP, el esquema SQLite ni el panel; `ui/` no se toca.
- NO se recortan los privilegios de restauración que crean bases y roles, ni se cambia el usuario con el que corre cada `exec` dentro de su contenedor.
- NO se añade un proxy de socket ni una pasarela HTTP genérica.

## Capabilities

### New Capabilities

- `motor-ipc`: contrato del canal entre la API y `kesto-motor` —framing, tope, lista blanca cerrada, credencial del par, redacción de secretos y errores tipados—.

### Modified Capabilities

- `config`: `DOCKER_HOST` se lee solo en el motor, se recorta y admite su defecto; `KESTO_MOTOR_SOCKET` y `KESTO_API_UID` (obligatorio en el motor) pasan a formar parte del contrato.
- `deploy`: el socket del daemon solo se monta en el servicio `kesto-motor`; `kesto-app` comparte con él el socket IPC y no tiene `bollard`.
- `engine`: la API no enlaza `bollard` y solo envía operaciones tipadas; el motor ejecuta la lista blanca y construye el `argv` literal.
- `confinamiento`: la etiqueta `io.kesto.backup-source=true` y el rechazo de contenedores sin ella se conservan, ahora ejecutados en el proceso motor.

## Impact

- **Código**: `Cargo.toml`, `src/bin/kesto-motor.rs` (nuevo), `src/ipc/` (nuevo), `src/docker/cliente.rs`, `src/docker/mod.rs`, `src/api/executions.rs`, `src/api/restorations.rs`, `src/api/repositories.rs`, `src/main.rs`, `src/config.rs`.
- **Dependencias**: `bollard` pasa a opcional con la feature `motor`; el binario API deja de enlazarla. Sin dependencias nuevas.
- **API HTTP**: ningún cambio de rutas ni de contratos; los errores del motor se traducen a los existentes.
- **Despliegue**: cambio incompatible para un `compose` propio que monte el socket en `kesto-app` o no declare `KESTO_API_UID`: hay que pasar el montaje al servicio motor y publicar el UID de la API. La migración se documenta en `docs/FEATURE-051-separacion-privilegios-motor-api.md`.
- **Verificación**: `just check`, comprobación de que el grafo del binario API no contiene `bollard` y revisión del compose con `rg 'docker.sock'`.
