# FEATURE-051 — Separación de privilegios motor/API (SEC-06 2/2)

- Issue de entrada: **#173** (auditoría de seguridad #156 / paraguas #168).
- Change SDD: `seguridad-sec-06-separacion-privilegios-motor-api`.
- Plan: `plans/PLAN-051-COMPLETED.md`.
- Rama: `feature/sec-06-separacion-privilegios-motor-api`.

## Qué es

Cierra el segundo mitad de SEC-06: el proceso que atiende HTTP ya no posee el socket de Docker. Se añade un binario privilegiado `kesto-motor` que es el único que enlaza `bollard` y lee `DOCKER_HOST`. La API habla con él por un canal IPC local tipado. Un `panic`, `unwrap` explotable o inyección en `argv` en la superficie de la API no entrega el daemon. FEATURE-040 había cerrado solo el alcance (etiqueta `io.kesto.backup-source=true` y rutas confinadas); el socket montado `:ro` seguía siendo equivalente a root.

## Ficheros implicados

- `Cargo.toml` / `Cargo.lock`: `bollard` opcional, feature `motor`, `[[bin]] kesto-motor` con `required-features`, y `tower-http` con `catch-panic`.
- `src/bin/kesto-motor.rs`: punto de entrada del proceso privilegiado.
- `src/ipc/`: `SolicitudMotor` / `RespuestaMotor` / `OrdenIpc`, framing `u32` big-endian + JSON, tope de 1 MiB, error tipado antes de asignar.
- `src/motor/{config,servidor}.rs`: configuración del motor, control de `SO_PEERCRED`, lista blanca, timeout, tope de conexiones, revalidación de etiqueta y rutas, redacción de errores.
- `src/docker/ipc_cliente.rs`: cliente IPC usado por la API (también bajo `cfg(test)`).
- `src/docker/{cliente,mod}.rs`: transporte `bollard` tras la feature `motor`.
- `src/config.rs` / `src/main.rs` / `src/api/mod.rs`: la API pierde `DOCKER_HOST`, gana `KESTO_MOTOR_SOCKET` y `CatchPanicLayer`.
- `src/engine/frontera.rs`: reutilizada por el motor para confinar rutas.
- `Dockerfile`: compilación en `target-api` y `target-motor` separados; copia de ambos binarios.
- `compose.yaml`, `.env.example`, `docs/ENTORNO.md`, `docs/DEFINITION.md` §8.
- `tests/sec_06_separacion_privilegios.rs`: 13 tests de contrato y comportamiento.

## Cómo funciona

1. El motor enlaza el Unix socket `KESTO_MOTOR_SOCKET` y comprueba `SO_PEERCRED` contra `KESTO_API_UID` antes de leer nada.
2. Cada mensaje lleva una longitud `u32` big-endian y un cuerpo JSON de hasta 1 MiB. Longitud 0, fuera de tope o lectura parcial se descartan con error; la lectura es cancel-safe y con timeout.
3. La lista blanca es cerrada: `Comprobar`, `CrearContenedor`, `IniciarContenedor`, `CrearExec`, `InspeccionarExec`, `IniciarExec`. Cualquier otra variante se rechaza sin llamar a `bollard`.
4. Antes de cualquier `exec`, el motor exige la etiqueta `io.kesto.backup-source=true` y revalida las rutas contra `KESTO_BACKUP_ROOT`, sin confiar en la API.
5. `Debug` de `OrdenIpc` y los errores IPC redactan `env` y la ruta del socket del daemon. El binario API no contiene símbolos `bollard::`.

## Variables y configuración

| Variable | Quién la lee | Comportamiento |
|---|---|---|
| `DOCKER_HOST` | Solo `kesto-motor` | Recortada; defecto `unix:///var/run/docker.sock`. La API no la lee. |
| `KESTO_MOTOR_SOCKET` | API y motor | Ruta del IPC; defecto `/run/kesto/motor.sock`. |
| `KESTO_API_UID` | Solo `kesto-motor` | Obligatoria: UID autorizado del par IPC; ausenteimpide arrancar el motor. |
| `KESTO_BACKUP_ROOT` | Solo `kesto-motor` | Ausente → `/backup_sources`; presente debe ser absoluta y no blanca, o el motor falla nombrando la variable. |
| `DOCKER_GID` | Compose | Grupo del daemon para el servicio motor. |

## Cómo desplegar

Reconstruir la imagen: el Dockerfile genera `kesto` y `kesto-motor` en árboles de compilación separados y copia ambos; el motor no sobrescribe la API. El compose de referencia declara dos servicios: `kesto` comparte el volumen `kesto_ipc` pero no monta `/var/run/docker.sock` ni declara `DOCKER_HOST`; `kesto-motor` corre con usuario no root, monta el socket `:ro`, declara `KESTO_API_UID`, `KESTO_MOTOR_SOCKET` y `KESTO_BACKUP_ROOT`, y comparte `kesto_ipc`. Migración: los despliegues que montaban el socket en la API deben moverlo al motor y fijar el UID de la API. `podman compose config` valida la plantilla con las variables obligatorias declaradas.

## Decisiones

- Se descartó un proxy de socket genérico: no inspecciona el cuerpo de `create`, no puede exigir la etiqueta ni validar `argv`.
- `bollard` es dependencia opcional y en dev solo la usan los tests del motor/e2e; la API bajo `cfg(test)` usa `ipc_cliente.rs`.
- El motor revalida etiqueta y rutas: la API se trata como no confiable.
- El tope de 1 MiB se aplica antes de asignar memoria; las lecturas no son cancelables por eventos.

## Ajuste posterior (2026-11-24)

- `src/motor/servidor.rs`: `atender_con_plazo` sustituye al `timeout` que envolvía `recibir_solicitud`. Un vigilante con `en_curso` y un contador de solicitudes entregadas solo declara `TiempoDeLectura` cuando no hay operación pendiente y no llega nada en 30 s; una `IniciarExec` esperando un `stdin` que no drena mantiene viva la conexión. El lector se levanta de su `select!` por señal, sin cancelar una lectura a medias.
- `src/docker/mod.rs` y `src/ipc/mensaje.rs`: el `Display` de `MotorNoDisponible` / `NoDisponible` ya no publica la ruta del socket, que sigue en el campo `socket` para diagnósticos. `SocketInexistente` conserva su redacción porque es un error de configuración deliberado.

## Verificación

`just check` (RC=0), `just check-all` (RC=0), `cargo test --features motor --bin kesto-motor` (6/6), `podman compose config` (RC=0), `cargo tree --bin kesto -e normal` sin `bollard`, y `nm -C` confirmando que `kesto` no tiene símbolos `bollard::` y `kesto-motor` sí.

## Follow-ups

No cubiertos: compromiso total del proceso motor, `COPY … FROM PROGRAM` de PostgreSQL (#175 para `ON_ERROR_STOP`), KDF (#169), DNS del endpoint (#182), chrono (#185), arm64 (#129).
