# PLAN-040 — Confinamiento de la frontera «operador → motor»

- **Estado:** propuesto (pendiente de aprobación del usuario)
- **Issues:** #160 (SEC-04), #161 (SEC-05), #162 (SEC-06) — hijos de la auditoría de seguridad #156 (bloque B)
- **Change OpenSpec:** `openspec/changes/confinamiento-frontera-motor/`
- **Rama prevista:** `feature/confinamiento-frontera-motor` (desde `development`)
- **Documento de la feature:** `docs/FEATURE-040-confinamiento-frontera-motor.md`
- **Bump esperado:** minor (`0.18.0`), con commits `feat:`/`fix:` sin `!`

## 1. Objetivo

Cerrar el alcance de los tres campos que el operador escribe en un target y que el motor usa sin más límite que su forma:

- `host_path` (targets `directory`): queda bajo la raíz de copias `KESTO_BACKUP_ROOT` (defecto `/backup_sources`).
- `db_path` (targets `sqlite`): ha de ser la forma canónica dentro del contenedor, y el `rm -f` de la restauración queda acotado al fichero declarado y sus hermanos `-wal`/`-shm`.
- `container_name`: solo se hace `exec` en contenedores que llevan la etiqueta `io.kesto.backup-source=true`.

Ninguno de los tres es un fallo de forma: los tres campos están bien formados y aun así alcanzan lo que no deben. La frontera que falta es de **alcance**, no de validación de forma.

## 2. Alcance

Dentro:

- `src/config.rs` — variable `KESTO_BACKUP_ROOT` (absoluta, defecto `/backup_sources`, error de arranque si es relativa o vacía).
- `src/engine/frontera.rs` — módulo nuevo con las funciones puras de alcance (forma de las rutas, comparación por componentes, composición canónica del anteproyecto existente más profundo).
- `src/api/targets.rs` — el borde HTTP comprueba el alcance en `alta_desde` (punto compartido por `POST` y `PUT`).
- `src/engine/extraccion.rs` y `src/engine/restauracion.rs` — canonicalización de `host_path` y comprobación canónica de `db_path`; `rm -f` acotado.
- `src/docker/cliente.rs` — la etiqueta de opt-in, comprobada antes de crear el `exec` (único punto por el que pasan todos los `exec` de bollard).
- `tests/e2e/` y `src/test_support.rs` — etiquetas en los contenedores de origen y raíz de copias apuntada a la raíz temporal.
- `docs/DEFINITION.md` §8, `docs/ENTORNO.md`, `docs/FEATURE-005-backup-engine.md`, `docs/FEATURE-040-confinamiento-frontera-motor.md`.

Fuera (non-goals):

- Separación de privilegio del motor (segunda mitad de SEC-06): issue de seguimiento.
- `CHECK` nuevos en el esquema de `backup_targets`: no se migran filas.
- Raíz propia para `db_path`: la frontera es el contenedor permitido.
- Cambios en la forma del contrato HTTP de los targets.

## 3. Decisiones de diseño

- **D1** — `KESTO_BACKUP_ROOT` con defecto `/backup_sources`, absoluta obligatoria. No se cablea en código (el e2e y el modo local copian temporales) ni se deriva de los montajes (en local no hay contenedor de Kesto).
- **D2** — La comparación contra la raíz es **por componentes** (`Path::starts_with`), no por prefijo de cadena: `/backup_sources_x` no cuela.
- **D3** — El borde HTTP comprueba la **forma** (textual, sin exigir existencia); el motor **canonicaliza** (resuelve enlaces simbólicos, también de los padres) y se aplica también a filas anteriores a la regla.
- **D4** — `db_path`: la frontera es el contenedor permitido; la comprobación es `readlink -f` y la comparación exacta con la ruta declarada. Se descarta sondear con `sh -c` (el repo construye los `argv` sin shell a propósito). Requisito nuevo del contenedor: `readlink` además de `sqlite3`.
- **D5** — `container_name`: etiqueta de opt-in `io.kesto.backup-source=true`, fail-closed sin configuración, comprobada en el único punto que crea `exec`. `kesto-app` queda denegado por sí solo.
- **D6** — `src/engine/frontera.rs` es el único sitio donde se decide qué es una ruta aceptable; no se duplica por dirección.
- **D7** — Tres cambios incompatibles, documentados en `docs/ENTORNO.md`: target `directory` fuera de la raíz, contenedor sin etiqueta (hay que recrearlo) y `db_path` no canónico.
- **D8** — Lo que NO se hace: separación de privilegio, `CHECK` en el esquema, tocar el socket montado.

## 4. Tareas

### 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check`.
- [x] 0.2 Rama desde `development`: `just gf-feature confinamiento-frontera-motor`.
- [x] 0.3 Change aprobado por el usuario (`just check-spec` encuentra el change activo).

### 1. La raíz de copias: configuración (SEC-04, #160)

- [x] 1.1 RED: sin `KESTO_BACKUP_ROOT`, la raíz es `/backup_sources`.
- [x] 1.2 RED: raíz relativa → error que nombra la variable.
- [x] 1.3 RED: raíz vacía → el mismo error, sin caer al defecto.
- [x] 1.4 RED: raíz absoluta explícita → configuración válida con esa raíz.
- [x] 1.5 GREEN: `src/engine/frontera.rs` con las funciones puras de alcance.
- [x] 1.6 GREEN: `src/config.rs` con la variable, el campo `backup_root` y el defecto.
- [x] 1.7 REFACTOR: rustdoc y `cargo fmt --all`; `clippy -D warnings` sin warnings.

### 2. El borde HTTP comprueba el alcance (SEC-04 · SEC-05)

- [x] 2.1 RED: `host_path = "/"` → `400` nombrando el campo, sin fila.
- [x] 2.2 RED: `host_path = "/backup_sources_x"` → `400` (prefijo de cadena que no es prefijo de componentes).
- [x] 2.3 RED: `host_path` relativo → `400`.
- [x] 2.4 RED: `host_path = "/backup_sources/var_www"` inexistente → `201` (contrapeso).
- [x] 2.5 RED: `db_path = "/datos/../etc/base.db"` → `400`.
- [x] 2.6 RED: `db_path` relativo → `400`.
- [x] 2.7 RED: `PUT` con `host_path` fuera de la raíz → `400` y la fila no cambia.
- [x] 2.8 `cargo test --locked --bin kesto 'api::targets::tests::'`.
- [x] 2.9 GREEN: `alta_desde` llama a `frontera` con la raíz de `AppState`.
- [x] 2.10 REFACTOR: mensajes que nombran el campo y la raíz sin revelar rutas ajenas.

### 3. El motor confina `host_path` (SEC-04, #160)

- [x] 3.1 RED: copia con `host_path` fuera de la raíz → `failed` sin leer.
- [x] 3.2 RED: enlace intermedio que escapa → `failed`.
- [x] 3.3 RED: `host_path` bajo la raíz y existente → copia completada (contrapeso).
- [x] 3.4 RED: restauración con `host_path` fuera de la raíz → `failed` sin escribir ni podar.
- [x] 3.5 RED: composición canónica de una ruta que no existe (anteproyecto existente más profundo).
- [x] 3.6 GREEN: `extraccion.rs`, rama `directory`, canonicaliza y confina antes de construir la ruta.
- [x] 3.7 GREEN: `restauracion.rs`, rama `directory`, canonicaliza y confina el destino.
- [x] 3.8 REFACTOR: un solo sitio decide qué es una ruta aceptable.

### 4. `db_path` canónico dentro del contenedor (SEC-05, #161)

- [x] 4.1 RED: `readlink -f` distinto del declarado → copia `failed` sin volcar.
- [x] 4.2 RED: el mismo caso en restauración → `failed` sin `rm -f`.
- [x] 4.3 RED: `db_path` canónico → el volcado corre con la orden de siempre (contrapeso).
- [x] 4.4 RED: `db_path` canónico con vecinos → el `rm -f` no los toca (contrapeso).
- [x] 4.5 RED: contenedor sin `readlink` → `failed` con el error del contenedor.
- [x] 4.6 GREEN: `extraccion.rs` comprueba la forma canónica antes de la orden de `sqlite`.
- [x] 4.7 GREEN: `restauracion.rs` la comprueba antes de la retirada, y acota el `rm -f`.
- [x] 4.8 GREEN: la lectura del `stdout` se apoya en el cliente del daemon, sin `sh -c`.
- [x] 4.9 REFACTOR: rustdoc del requisito de `readlink` y del coste de declarar la ruta canónica.

### 5. Solo contenedores declarados fuentes de copia (SEC-06, #162)

- [x] 5.1 RED: contenedor sin etiqueta → `failed` nombrando el contenedor y sin `exec`.
- [x] 5.2 RED: contenedor con etiqueta → el `exec` se crea y la orden corre.
- [x] 5.3 RED: contenedor que no existe → el mismo fallo que hoy (contrapeso).
- [x] 5.4 RED: restauración hacia contenedor sin etiqueta → `failed` sin `exec`.
- [x] 5.5 GREEN: `cliente.rs` inspecciona antes de `create_exec` y exige la etiqueta.
- [x] 5.6 GREEN: la lectura de las etiquetas, con su propio tipo de error.
- [x] 5.7 REFACTOR: rustdoc del porqué de la etiqueta y del único punto de comprobación.

### 6. Arnés, e2e y documentación

- [x] 6.1 `Receta` gana etiquetas; el e2e etiqueta sus contenedores de origen y apunta la raíz a su temporal.
- [x] 6.2 Escenarios nuevos: contenedor sin etiqueta no se copia; `directory` fuera de la raíz no se copia.
- [x] 6.3 `src/test_support.rs` y los tests con contenedores declaran la raíz y la etiqueta.
- [x] 6.4 `docs/DEFINITION.md` §8: la variable y la etiqueta.
- [x] 6.5 `docs/ENTORNO.md`: la variable en la tabla y la nota de compatibilidad.
- [x] 6.6 `docs/FEATURE-005-backup-engine.md`: el confinamiento deja de ser una promesa de los tests.
- [x] 6.7 `docs/FEATURE-040-confinamiento-frontera-motor.md`.

### 7. Cierre

- [x] 7.1 `just check-all` en verde.
- [x] 7.2 Revisión de `@rust-reviewer` sin bloqueantes.
- [x] 7.3 PR a `development` con `Closes #160`, `Closes #161` y `Closes #162`.
- [x] 7.4 Fusionado: `just spec-archive confinamiento-frontera-motor`.
- [x] 7.5 `plans/PLAN-040.md` → `plans/PLAN-040-COMPLETED.md`.
- [x] 7.6 Issue de seguimiento para la separación de privilegio del motor.

## 5. Verificación

- `just check` (fmt + clippy + test) y `just check-all` (más el panel) en verde.
- `openspec validate confinamiento-frontera-motor --strict` válido.
- `just e2e` en verde, con los escenarios nuevos de la frontera.
- El E2E de CI pasa (job con docker; en local se usa podman).

## 6. Riesgos

- **Cambios incompatibles en despliegues existentes** (target `directory` fuera de la raíz, contenedor sin etiqueta, `db_path` no canónico): se asumen a conciencia y se documentan en `docs/ENTORNO.md`; el motor falla nombrando el campo en vez de copiar mal.
- **`readlink` ausente en un contenedor exótico**: se reporta como el fallo de la ejecución, con el error del contenedor; busybox y coreutils lo traen.
- **`inspect_container` en el camino de cada `exec`**: una llamada más al daemon por ejecución, coste despreciable frente a la duración de una copia.
- **Canonicalización de rutas que no existen**: se resuelve con el anteproyecto existente más profundo, patrón ya usado por `normalizar_el_destino`.

## 7. Referencias

- `openspec/changes/confinamiento-frontera-motor/` — proposal, design, deltas (`confinamiento`, `config`, `api-resources`, `extraction`, `restauracion`, `verificacion-e2e`) y tasks.
- Issues #156 (campaña), #160, #161, #162; seguimiento previsto para la separación de privilegio del motor.
- `docs/DEFINITION.md` §3, §4, §5, §8; `docs/FEATURE-005-backup-engine.md`.
