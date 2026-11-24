# PLAN-051 — SEC-06 (2/2): separación de privilegios motor/API

- Estado: EN CURSO
- Entrada: issue #173 (campaña de seguridad #156 / paraguas #168)
- Rama: `feature/sec-06-separacion-privilegios-motor-api` (desde `development`)
- Change SDD: `seguridad-sec-06-separacion-privilegios-motor-api`

## 1. Objetivo

Separar el proceso que atiende la API del proceso que posee el socket de Docker, con una frontera de proceso real. Criterio de cierre: un fallo, `panic`, `unwrap` explotable o inyección en `argv` en la superficie de la API **no debe entregar el socket de Docker**, y una prueba CLI debe demostrarlo. La API deja de montar/leer el socket y deja de enlazar `bollard`.

## 2. Decisión arquitectónica

Se descarta el proxy de socket genérico con lista blanca de endpoints: no inspecciona el cuerpo de `POST /containers/create`, no puede exigir la etiqueta `io.kesto.backup-source=true` ni validar `argv`, por lo que una API comprometida podría crear un contenedor con montaje de `/`.

Se adopta un **segundo binario privilegiado `kesto-motor`** con `bollard` y `DOCKER_HOST` en exclusiva, y un **canal IPC local tipado** con lista blanca de operaciones. La API solo conoce un cliente de ese protocolo. `bollard` pasa a dependencia opcional activada únicamente por la feature `motor`, de modo que el binario de la API no enlaza el cliente Docker (frontera verificable en tiempo de compilación).

Credenciales: `DOCKER_HOST` y las credenciales de `Orden.env` se leen/construyen solo en el proceso motor; viajan en el mensaje IPC, nunca en el entorno ni en los logs de la API. `Debug` de los mensajes IPC y de `Orden` debe redactar `env`. Lo que viaja por IPC queda limitado a los tipos de la lista blanca; no se reenvía HTTP crudo.

## 3. Mapa de archivos

- `Cargo.toml`: `bollard` opcional; feature `motor`; `[[bin]] kesto-motor` con `required-features = ["motor"]`.
- `src/bin/kesto-motor.rs`: nuevo punto de entrada privilegiado; configura y acepta solo el IPC.
- `src/ipc/`: tipos `SolicitudMotor` / `RespuestaMotor`, framing con longitud y tope, control de credenciales del par (`SO_PEERCRED`) y redacción de secretos en `Debug`.
- `src/docker/cliente.rs` y `src/docker/mod.rs`: permanecen en el lado motor; `conectar()`, `exigir_fuente_de_copia()`, `ejecutar()` y `ejecutar_con_entrada()` conservan la etiqueta y el confinamiento actuales.
- `src/api/executions.rs`, `src/api/restorations.rs`, `src/api/repositories.rs`: sustituyen el cliente directo por `MotorClient`.
- `src/main.rs`: deja de inyectar `config.docker_host` en `Ajustes` para el binario API.
- `src/config.rs`: `DOCKER_HOST` solo se valida en el binario motor; la API usa `KESTO_MOTOR_SOCKET`; el motor exige `KESTO_API_UID` para comprobar el par.
- `docs/DEFINITION.md` §8 y `docs/ENTORNO.md`: compose con servicio `kesto-motor` (socket `:ro`), sin socket en `kesto-app`, volumen compartido para el socket IPC y documentación de las dos variables nuevas.
- `docs/FEATURE-051-separacion-privilegios-motor-api.md`: doc de feature (se crea al cerrar).
- `ui/`: sin cambios.

## 4. Contratos

- `SolicitudMotor` (lista blanca cerrada): `Comprobar`, `CrearContenedor`, `IniciarContenedor`, `CrearExec`, `InspeccionarExec`, `IniciarExec`. Cualquier otra variante es error sin llegar al daemon.
- `RespuestaMotor`: variantes tipadas de éxito/error; los errores al cliente no incluyen rutas de socket ni credenciales.
- IPC: Unix domain socket `KESTO_MOTOR_SOCKET` (defecto `/run/kesto/motor.sock`), framing `u32` big-endian + JSON, tope de 1 MiB, `SO_PEERCRED` comparado con `KESTO_API_UID`.
- `DOCKER_HOST`: recortado, con defecto `unix:///var/run/docker.sock`, leído solo por `kesto-motor`; `Docker::connect_with_socket(socket, segundos, bollard::API_DEFAULT_VERSION)` sin `connect_with_defaults()`.
- Etiqueta `io.kesto.backup-source=true` y rutas confinadas se comprueban en el motor, antes de cualquier `exec`.

## 5. Escenarios Given/When/Then

1. Dado un `panic` o `unwrap` explotable en un handler de la API, cuando se intenta alcanzar el motor, entonces el binario API no posee el socket ni `DOCKER_HOST`, la petición no llega al daemon y el proceso motor sigue vivo.
2. Dada una solicitud IPC fuera de la lista blanca, cuando llega al motor, entonces se rechaza con error tipado y no se emite ninguna llamada a bollard.
3. Dado un `exec`/`inspect`/`create`/`start` legítimos, cuando llegan al motor, entonces se ejecutan conservando la etiqueta y el confinamiento actuales.
4. Dado un contenedor sin la etiqueta, cuando se pide `exec`, entonces resulta `failed`, no se crea `exec` y el error no filtra credenciales (pruebas existentes en `cliente.rs` como base).
5. Dado `argv` con metacomandos o inyección de shell, cuando se construye `Orden`, entonces se pasa como argv literal y el motor no interpreta metacomandos.
6. Dado un par IPC con UID distinto de `KESTO_API_UID`, cuando conecta, entonces se cierra sin procesar la solicitud.
7. Dado un mensaje que excede 1 MiB o con longitud inválida, cuando se lee, entonces se descarta sin pánico.
8. Dado `DOCKER_HOST` ausente/vacío/con espacios, cuando arranca el motor, entonces aplica el defecto recortado; un socket inexistente produce error que lo nombra sin impedir el arranque de la API.
9. Dado el despliegue compose, cuando se inspecciona, entonces `kesto-app` no monta `/var/run/docker.sock` y `kesto-motor` lo monta `:ro` con usuario no root.

## 6. Tareas TDD

- [x] RED: escribir los tests de los escenarios 1-9, incluidos test de integración con daemon falso y aserción de que el binario API no enlaza `bollard`.
- [x] RED: ejecutar `cargo test` y confirmar que los nuevos tests fallan y los heredados siguen verdes.
- [x] GREEN: implementar feature `motor`, `src/bin/kesto-motor.rs`, `src/ipc/` y `MotorClient` en la API; mover la conexión Docker al motor.
- [x] GREEN: actualizar `src/config.rs`, compose, `docs/ENTORNO.md` y `docs/DEFINITION.md` §8.
- [x] REFACTOR: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` y `just check`.
- [x] Auditoría de `@rust-reviewer`; corregir hallazgos y repetir `just check`.
- [x] Cerrar: `openspec archive`, crear `docs/FEATURE-051-...md`, marcar tareas y renombrar el plan a `plans/PLAN-051-COMPLETED.md`; actualizar `HANDOFF.md`.
- [x] Gitflow: commits con gitmoji en la rama feature, push y PR a `development`.

## 7. Verificación

`just check` (fmt + clippy + test sobre el workspace) y comprobación adicional de que el Grafo de dependencias del binario API no contiene `bollard`. Revisión del compose con `rg 'docker.sock'` para confirmar el montaje solo en el servicio motor.

## 8. Fuera de alcance

No cambia la API HTTP, el esquema SQLite ni el panel. No reabre SEC-04/SEC-05/SEC-06(1/2) (rutas, `readlink -f`, etiqueta). No cubre `#175` (`ON_ERROR_STOP`), `#169` (KDF), `#182` (DNS), el residual `COPY … FROM PROGRAM`, ni el compromiso total del proceso motor. No se recortan privilegios de restauración que crean bases/roles.

## 9. Riesgos

- El cambio de compose puede alterar despliegues existentes: documentar migración en `docs/FEATURE-051-...md`.
- `cargo deny` puede fallar por advisories ajenos al change.
- En local se usa Podman (`DOCKER_HOST` apunta al socket de Podman); en producción, Docker.
