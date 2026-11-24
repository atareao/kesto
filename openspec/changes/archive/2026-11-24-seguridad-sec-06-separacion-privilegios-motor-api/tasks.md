# Tasks — seguridad-sec-06-separacion-privilegios-motor-api

> Issue **#173** (auditoría **#156** / paraguas **#168**), SEC-06 (2/2). Plan: `plans/PLAN-051.md`.
> Documento de la feature, a escribir al cerrar: `docs/FEATURE-051-separacion-privilegios-motor-api.md`.
> Decisiones y alternativas descartadas: `design.md`. TDD estricto: rojo → verde → refactor.
> Todas las casillas quedan **sin marcar**: el agente que ejecuta el change las va tachando.

## 0. Preparación y puerta de aprobación

- [x] 0.1 Rama `feature/sec-06-separacion-privilegios-motor-api` desde `development` (verificación: `git branch --show-current` devuelve ese nombre y `git merge-base --is-ancestor development HEAD` termina con `0`).
- [x] 0.2 Change activo y specs vigentes antes de tocar código (verificación: `just check-spec` y `openspec validate seguridad-sec-06-separacion-privilegios-motor-api --strict` terminan sin error).
- [x] 0.3 Línea base en verde (verificación: `just check` termina sin error antes de escribir ningún test).
- [x] 0.4 **Aprobación explícita del usuario** de esta propuesta y de `plans/PLAN-051.md` (verificación: el usuario dice «aprobado» o equivalente en la conversación; NO SHALL empezar el RED sin ella).

## 1. RED — escenarios 1, 2, 3, 6 y 7: frontera de proceso e IPC

- [x] 1.1 Test —un `panic`/`unwrap` explotable en un handler de la API no entrega el socket: el binario API no posee `DOCKER_HOST` ni `/var/run/docker.sock`, la petición no llega al daemon falso y el proceso motor sigue vivo (verificación: `cargo test --locked --workspace` falla; `rg -n 'bollard' src/main.rs src/api` no encuentra nada todavía).
- [x] 1.2 Test de integración con daemon falso —las seis operaciones legítimas (`Comprobar`, `CrearContenedor`, `IniciarContenedor`, `CrearExec`, `InspeccionarExec`, `IniciarExec`) llegan al motor y conservan la etiqueta y el confinamiento actuales (verificación: `cargo test --locked --workspace ipc::` falla por módulo inexistente).
- [x] 1.3 Test —una variante fuera de la lista blanca se rechaza con error tipado y NO SHALL emitir ninguna llamada a bollard (verificación: el test del escenario falla porque no hay `SolicitudMotor`).
- [x] 1.4 Test —un par IPC con UID distinto de `KESTO_API_UID` se cierra sin procesar la solicitud (verificación: el test falla porque no hay control de `SO_PEERCRED`).
- [x] 1.5 Test —un mensaje de más de 1 MiB y otro con longitud inválida se descartan sin pánico (verificación: el test falla porque no hay framing).
- [x] 1.6 Test —`Debug` de `SolicitudMotor`, `RespuestaMotor` y `Orden` NO SHALL mostrar `env`; el error devuelto tampoco incluye la ruta del socket (verificación: el test falla por tipos inexistentes).
- [x] 1.7 Test de frontera de compilación —el grafo del binario API no contiene `bollard` (verificación: `cargo tree --bin kesto -e normal | rg -q '^bollard| bollard '` **encuentra** la dependencia hoy; el cambio tiene que invertir el resultado).
- [x] 1.8 Ejecutar la batería roja completa (verificación: `cargo test --locked --workspace` falla solo por los tests nuevos y los heredados siguen verdes).

## 2. RED — escenarios 4 y 5: etiqueta y `argv` literal

- [x] 2.1 Test —un contenedor sin `io.kesto.backup-source=true` al que se pide `exec` a través del IPC queda `failed`, no se crea el `exec` y el error no filtra credenciales; se apoya en las pruebas existentes de `src/docker/cliente.rs` como contrapeso (verificación: `cargo test --locked --workspace confinamiento::` falla en el caso nuevo y las pruebas heredadas pasan).
- [x] 2.2 Test —un `argv` con metacomandos o inyección de shell se pasa como argv literal y el motor no lo interpreta (verificación: el test falla porque hoy la construcción no está detrás del IPC).
- [x] 2.3 Contrapeso verde —la etiqueta sigue exigiéndose al restaurar y el propio contenedor de Kesto no es fuente de copia (verificación: `cargo test --locked --workspace 'confinamiento::' ` sigue en verde).

## 3. RED — escenarios 8 y 9: configuración y despliegue

- [x] 3.1 Test de configuración —`DOCKER_HOST` ausente, vacío y con espacios: en los tres casos el motor aplica el defecto recortado `unix:///var/run/docker.sock` o el valor recortado, y la API no lo lee (verificación: `cargo test --locked --workspace config::` falla en los casos nuevos y los tres heredados pasan).
- [x] 3.2 Test —un socket inexistente produce en el motor un error que lo nombra sin impedir el arranque de la API (verificación: el test falla hoy porque la API arranca con `config.docker_host`).
- [x] 3.3 Test —`KESTO_API_UID` ausente impide arrancar el motor, con un mensaje que nombra la variable (verificación: el test falla por variable inexistente).
- [x] 3.4 Test del compose —`kesto-app` no monta `/var/run/docker.sock`; `kesto-motor` lo monta `:ro`, corre con usuario no root y ambas comparten el volumen del socket IPC (verificación: `rg -n 'docker.sock' compose.yaml` muestra hoy el montaje en el servicio equivocado; la aserción falla).
- [x] 3.5 Validación del compose de referencia (verificación: `podman compose config` sigue pasando con las variables obligatorias declaradas).

## 4. GREEN — binario partido e IPC

- [x] 4.1 `Cargo.toml`: `bollard` opcional, feature `motor` y `[[bin]] kesto-motor` con `required-features = ["motor"]` (verificación: `cargo check --locked --bin kesto` compila sin bollard; `cargo check --locked --features motor --bin kesto-motor` compila el motor).
- [x] 4.2 `src/bin/kesto-motor.rs`: punto de entrada privilegiado que configura y acepta solo el IPC (verificación: `cargo check --locked --features motor --bin kesto-motor` sin errores).
- [x] 4.3 `src/ipc/`: tipos de la lista blanca, framing `u32` big-endian + JSON con tope de 1 MiB, `SO_PEERCRED` contra `KESTO_API_UID`, redacción de secretos en `Debug` y errores tipados sin rutas ni credenciales (verificación: 1.3–1.6 en verde).
- [x] 4.4 `src/docker/cliente.rs` y `src/docker/mod.rs`: `conectar()`, `exigir_fuente_de_copia()`, `ejecutar()` y `ejecutar_con_entrada()` siguen en el lado motor y ejecutan la lista blanca, con la etiqueta y las rutas confinadas comprobadas antes de cualquier `exec` (verificación: 1.2, 2.1 y 2.2 en verde).
- [x] 4.5 `MotorClient` en la API: `src/api/executions.rs`, `src/api/restorations.rs` y `src/api/repositories.rs` sustituyen el cliente directo por el cliente IPC; `src/main.rs` deja de inyectar `config.docker_host` en `Ajustes` (verificación: 1.1 y 1.7 en verde; `cargo tree --bin kesto -e normal | rg -q '^bollard| bollard '` NO encuentra nada).

## 5. GREEN — configuración, despliegue y documentación

- [x] 5.1 `src/config.rs`: `DOCKER_HOST` recortado y leído solo en el motor; `KESTO_MOTOR_SOCKET` con defecto `/run/kesto/motor.sock`; `KESTO_API_UID` obligatorio en el motor (verificación: 3.1–3.3 en verde).
- [x] 5.2 `compose.yaml`: socket del daemon solo en `kesto-motor` (`:ro`, usuario no root, `group_add` desde `DOCKER_GID`), volumen compartido para el socket IPC y sin `DOCKER_HOST` en `kesto-app` (verificación: 3.4 en verde y `podman compose config` pasa).
- [x] 5.3 `docs/DEFINITION.md` §8 y `docs/ENTORNO.md`: servicio `kesto-motor` y las dos variables nuevas, con su obligación y su defecto (verificación: la tabla, el compose de referencia y `rg -n 'KESTO_API_UID|KESTO_MOTOR_SOCKET' docs` concuerdan).
- [x] 5.4 Batería completa en verde (verificación: `cargo test --locked --workspace`).

## 6. REFACTOR y verificación

- [x] 6.1 Rustdoc del porqué de la frontera de proceso, del framing, de `SO_PEERCRED` y de la redacción de `env` (verificación: `cargo doc --no-deps --workspace` sin errores).
- [x] 6.2 Formato y lints (verificación: `cargo fmt --all` y `cargo clippy --workspace --all-targets -- -D warnings` sin warnings).
- [x] 6.3 Puerta completa del CI en local (verificación: `just check` termina sin error; `just check-all` si el entorno tiene `cargo deny` y el panel instalados).
- [x] 6.4 Comprobación explícita de la frontera (verificación: `cargo tree --bin kesto -e normal | rg 'bollard'` sin salida y `rg -n 'docker.sock' compose.yaml` solo bajo `kesto-motor`).
- [x] 6.5 Auditoría de `@rust-reviewer`: corregir hallazgos y repetir `just check` (verificación: sin bloqueantes y checks en verde).

## 7. Cierre

- [x] 7.1 `docs/FEATURE-051-separacion-privilegios-motor-api.md`: qué es, archivos implicados, contrato IPC, variables, migración del compose, decisiones y follow-ups (verificación: el fichero existe y cubre esos apartados).
- [x] 7.2 PR a `development` con `Closes #173` (verificación: el PR abre con el cierre y el CI en verde; commits con conventional + gitmoji, sin commit directo en `main` ni `development`).
- [x] 7.3 Archivar el change (verificación: `just spec-archive seguridad-sec-06-separacion-privilegios-motor-api` aplica los deltas a `openspec/specs/`).
- [x] 7.4 `plans/PLAN-051.md` → `plans/PLAN-051-COMPLETED.md` y `HANDOFF.md` actualizado (verificación: `git mv` hecho; `HANDOFF.md` sigue sin versionarse).
