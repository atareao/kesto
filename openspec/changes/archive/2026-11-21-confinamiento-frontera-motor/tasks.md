# Tasks — confinamiento-frontera-motor

> Issues **#160 (SEC-04)**, **#161 (SEC-05)** y **#162 (SEC-06)**, hijos de la auditoría **#156** (bloque B).
> Change de **producto**: configuración, borde HTTP de targets, motor (extracción y restauración), cliente del daemon y arnés.
> Decisiones y alternativas descartadas, en `design.md`. TDD: primero los tests que hoy fallan (rojo),
> luego el mínimo (verde), luego el refactor. Sin dependencias nuevas.
> Plan: `plans/PLAN-040.md`. Documento de la feature: `docs/FEATURE-040-confinamiento-frontera-motor.md`.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Rama desde `development`: `just gf-feature confinamiento-frontera-motor` (verificación: `git branch --show-current`).
- [x] 0.3 Change aprobado por el usuario (verificación: aprobación explícita y `just check-spec` encuentra el change activo).

## 1. La raíz de copias: configuración (SEC-04, #160)

### RED

- [x] 1.1 Rust: test —sin `KESTO_BACKUP_ROOT`— exige que la raíz sea `/backup_sources` (verificación: rojo hoy, que no existe la variable).
- [x] 1.2 Rust: test con `KESTO_BACKUP_ROOT=backup_sources` (relativa) exige error que **nombre la variable** (verificación: rojo hoy).
- [x] 1.3 Rust: test con `KESTO_BACKUP_ROOT` presente pero vacía exige el mismo error, sin caer al defecto (verificación: rojo hoy).
- [x] 1.4 Rust: test con `KESTO_BACKUP_ROOT=/tmp/kesto-copias` exige una configuración válida con esa raíz (verificación: verde tras 1.6).

### GREEN

- [x] 1.5 `src/engine/frontera.rs`: módulo nuevo con las funciones **puras** de alcance —forma de `host_path` y de `db_path`, comparación **por componentes** contra una raíz, y composición de la ruta canónica a partir del anteproyecto existente más profundo— (verificación: `cargo test --locked --bin kesto 'frontera::tests::'` verde).
- [x] 1.6 `src/config.rs`: constante `ENV_KESTO_BACKUP_ROOT`, campo `backup_root` en `Config`, defecto `DEFAULT_BACKUP_ROOT = "/backup_sources"` y la variante de error que nombra la variable; la raíz se guarda **canonicalizada** cuando existe y textualmente cuando no (verificación: 1.1–1.4 en verde).

### REFACTOR

- [x] 1.7 Rustdoc de la variable y de la raíz —con el porqué y con el coste de compatibilidad— y `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 2. El borde HTTP comprueba el alcance (SEC-04 · SEC-05)

### RED

- [x] 2.1 Rust: test de alta `directory` con `host_path = "/"` exige `400` nombrando `host_path` y sin fila escrita (verificación: rojo hoy, que contesta `201`).
- [x] 2.2 Rust: test de alta `directory` con `host_path = "/backup_sources_x"` exige `400` (raíz que solo coincide como prefijo de cadena) (verificación: rojo hoy).
- [x] 2.3 Rust: test de alta `directory` con `host_path` relativo exige `400` (verificación: rojo hoy).
- [x] 2.4 Rust: test de alta `directory` con `host_path = "/backup_sources/var_www"` —que **no existe** en disco— exige `201` (verificación: verde hoy; se conserva como contrapeso de 2.1–2.3).
- [x] 2.5 Rust: test de alta `sqlite` con `db_path = "/datos/../etc/base.db"` exige `400` nombrando `db_path` (verificación: rojo hoy).
- [x] 2.6 Rust: test de alta `sqlite` con `db_path` relativo exige `400` (verificación: rojo hoy).
- [x] 2.7 Rust: test de `PUT` que pone a un `directory` un `host_path` fuera de la raíz exige `400` y que la fila no cambie (verificación: rojo hoy).
- [x] 2.8 Verificación conjunta: `cargo test --locked --bin kesto 'api::targets::tests::'` (verificación: falla solo por los casos nuevos).

### GREEN

- [x] 2.9 `src/api/targets.rs`: `alta_desde` llama a `frontera` para el alcance de `host_path` (absoluto y bajo la raíz, por componentes) y de `db_path` (absoluto, sin `..`, sin segmento vacío), con `400` que nombra el campo; la raíz sale de `AppState` (verificación: 2.1–2.8 en verde).

### REFACTOR

- [x] 2.10 Mensajes de error que nombran el campo y la raíz, sin revelar rutas ajenas; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 3. El motor confina `host_path` (SEC-04, #160)

### RED

- [x] 3.1 Rust: test —target `directory` con `host_path` fuera de la raíz— exige que la copia acabe en `failed` nombrando `host_path` y **sin leer** el directorio (verificación: rojo hoy, que lo copia).
- [x] 3.2 Rust: test —`host_path` textualmente bajo la raíz pero con un directorio intermedio que es un enlace a fuera— exige `failed` (verificación: rojo hoy).
- [x] 3.3 Rust: test —`host_path` bajo la raíz y existente— exige que la copia se complete (verificación: verde hoy; se conserva como contrapeso).
- [x] 3.4 Rust: test —restauración a un `directory` cuyo `host_path` está fuera de la raíz— exige `failed` sin escribir ni podar nada (verificación: rojo hoy).
- [x] 3.5 Rust: test de la composición canónica cuando la ruta **no existe** —el destino de una restauración— exige que se canonicalice el anteproyecto existente más profundo y se recomponga el resto (verificación: rojo hoy).

### GREEN

- [x] 3.6 `src/engine/extraccion.rs`: la rama `directory` de `ruta()` canonicaliza y confina contra la raíz antes de construir la `Ruta::PorFichero`, con el error de alcance (verificación: 3.1–3.3 en verde).
- [x] 3.7 `src/engine/restauracion.rs`: la rama `directory` de `comando()` canonicaliza y confina el destino antes de construir el `CaminoDeRestauracion::PorDirectorio` (verificación: 3.4 en verde).

### REFACTOR

- [x] 3.8 Un solo sitio decide qué es una ruta aceptable —`frontera`—, usado por la API y por el motor; rustdoc del porqué (la defensa en profundidad y las filas anteriores a la regla) y `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 4. `db_path` canónico dentro del contenedor (SEC-05, #161)

### RED

- [x] 4.1 Rust: test —target `sqlite` cuyo `readlink -f` de `db_path` difiere del declarado— exige que la copia acabe en `failed` **sin** ejecutar el volcado (verificación: rojo hoy, que lo ejecuta).
- [x] 4.2 Rust: test —mismo caso en restauración— exige `failed` **sin** ejecutar el `rm -f` (verificación: rojo hoy).
- [x] 4.3 Rust: test —`db_path` canónico— exige que el volcado se ejecute con la orden de siempre (verificación: verde hoy; se conserva como contrapeso).
- [x] 4.4 Rust: test —`db_path` canónico con ficheros vecinos— exige que el `rm -f` se acote al fichero y a sus `-wal`/`-shm`, sin tocar los vecinos (verificación: verde hoy; se conserva como contrapeso).
- [x] 4.5 Rust: test —contenedor sin `readlink`— exige `failed` con el error del contenedor leído (verificación: rojo hoy, que no lo invoca).

### GREEN

- [x] 4.6 `src/engine/extraccion.rs`: antes de construir la orden de `sqlite`, se ejecuta la comprobación de forma canónica y se compara su salida con el `db_path` declarado; si difiere, la ejecución falla nombrando el campo (verificación: 4.1, 4.3, 4.5 en verde).
- [x] 4.7 `src/engine/restauracion.rs`: la misma comprobación **antes** de la orden de retirada, y el `rm -f` acotado al `<db_path>` y sus hermanos (verificación: 4.2, 4.4 en verde).
- [x] 4.8 La comprobación se apoya en el cliente del daemon para leer el `stdout` de la orden de forma canónica, sin `sh -c` (verificación: el `cmd` del `exec` no contiene ninguna shell).

### REFACTOR

- [x] 4.9 Rustdoc del requisito de `readlink` en el contenedor y del coste de declarar la ruta canónica; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 5. Solo contenedores declarados fuentes de copia (SEC-06, #162)

### RED

- [x] 5.1 Rust: test —contenedor sin la etiqueta `io.kesto.backup-source=true`— exige que la ejecución acabe en `failed` nombrando el contenedor y que **no se cree** ningún `exec` (verificación: rojo hoy, que lo crea).
- [x] 5.2 Rust: test —contenedor con la etiqueta— exige que el `exec` se cree y la orden corra (verificación: rojo hoy; verde tras 5.5).
- [x] 5.3 Rust: test —contenedor que no existe— exige el mismo fallo que hoy, sin confundirse con el de la etiqueta (verificación: verde hoy; se conserva como contrapeso).
- [x] 5.4 Rust: test —restauración hacia un contenedor sin etiqueta— exige `failed` sin `exec` (verificación: rojo hoy).

### GREEN

- [x] 5.5 `src/docker/cliente.rs`: `ejecutar` y `ejecutar_con_entrada` inspeccionan el contenedor **antes** de `create_exec` y exigen la etiqueta `io.kesto.backup-source=true`, con un error que nombra el contenedor; el `exec` no se crea si falta (verificación: 5.1–5.4 en verde).
- [x] 5.6 `src/docker/mod.rs` (o donde encaje la inspección): la lectura de las etiquetas del contenedor, con su propio tipo de error (verificación: `cargo test --locked --bin kesto 'docker::'` verde).

### REFACTOR

- [x] 5.7 Rustdoc del porqué de la etiqueta frente a una lista de configuración y de que la comprobación va en el único punto que crea `exec`; `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 6. Arnés, e2e y documentación

- [x] 6.1 `tests/e2e/arnes.rs`: `Receta` gana las etiquetas del contenedor y la creación las aplica; `tests/e2e/escenarios.rs` etiqueta todos sus contenedores de origen con `io.kesto.backup-source=true` y apunta `KESTO_BACKUP_ROOT` a la raíz temporal de la batería (verificación: `just e2e` en verde).
- [x] 6.2 `tests/e2e/escenarios.rs`: escenario nuevo —un contenedor de origen **sin** la etiqueta no se copia— y escenario nuevo —un `directory` fuera de la raíz no se copia— (verificación: los dos pasan y fallarían con el código de antes).
- [x] 6.3 `src/test_support.rs` y los tests unitarios con contenedores: declaran la raíz de copias y la etiqueta donde haga falta (verificación: `just check` en verde).
- [x] 6.4 `docs/DEFINITION.md` §8: la variable `KESTO_BACKUP_ROOT` y la etiqueta de fuente de copia (verificación: la tabla de variables y el `compose` de referencia concuerdan).
- [x] 6.5 `docs/ENTORNO.md`: `KESTO_BACKUP_ROOT` en la tabla, con su defecto y su obligación de ser absoluta, y la nota de actualización por los tres cambios incompatibles (verificación: la tabla lista la variable y el aviso de compatibilidad).
- [x] 6.6 `docs/FEATURE-005-backup-engine.md`: el confinamiento deja de ser una promesa de los tests y pasa a ser una regla del motor (verificación: el documento ya no describe `/backup_sources` como algo solo probado).
- [x] 6.7 `docs/FEATURE-040-confinamiento-frontera-motor.md`: documento de la feature entregada (qué es, ficheros, cómo funciona, variables, decisiones y follow-ups) (verificación: existe y cubre los cinco apartados).

## 7. Cierre

- [x] 7.1 `just check-all` (backend y panel) en verde (verificación: termina sin error).
- [x] 7.2 Revisión de `@rust-reviewer` sin bloqueantes (verificación: las observaciones aplicadas o contestadas).
- [x] 7.3 PR a `development` con `Closes #160`, `Closes #161` y `Closes #162` (verificación: el PR abre con los tres cierres y el CI en verde).
- [x] 7.4 Fusionado el PR: `just spec-archive confinamiento-frontera-motor` (verificación: el delta entra en `openspec/specs/`).
- [x] 7.5 `plans/PLAN-040.md` → `plans/PLAN-040-COMPLETED.md` (verificación: `git mv` hecho).
- [x] 7.6 Issue de seguimiento para la separación de privilegio del motor (segunda mitad de SEC-06) (verificación: el issue existe y nombra el hallazgo).
