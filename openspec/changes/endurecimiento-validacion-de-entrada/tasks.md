# Tasks — endurecimiento-validacion-de-entrada

> SEC-13 y SEC-14 del bloque F, más el `container_name` de SEC-22, hijos de la auditoría #156 (#168).
> Decisiones y alternativas descartadas, en `design.md`.
> TDD: primero los tests que hoy fallan (rojo), luego el mínimo (verde), luego el refactor.
> Sin dependencias nuevas.
> Plan: `plans/PLAN-046.md`. Documento de la feature: `docs/FEATURE-046-endurecimiento-validacion-de-entrada.md`.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Rama desde `development`: `just gf-feature endurecimiento-validacion-de-entrada` (verificación: `git branch --show-current`).
- [x] 0.3 Change aprobado por el usuario (verificación: aprobación explícita y `just check-spec` encuentra el change activo).

## 1. Módulo de texto compartido

### RED

- [x] 1.1 Rust: tests unitarios de `src/api/texto.rs` — `obligatorio` recorta, rechaza vacío/blanco y rechaza lo que supera su cota nombrando el campo; `opcional` recorta, devuelve `None` para ausente o en blanco y rechaza lo que supera su cota (verificación: rojo hoy, que el módulo no existe).

### GREEN

- [x] 1.2 `src/api/texto.rs`: las tres funciones y las constantes de cota (verificación: 1.1 en verde).

## 2. Cotas y recorte en targets (SEC-13)

### RED

- [x] 2.1 Rust (`src/api/targets.rs`): un `name` de solo espacios en un alta contesta `400` nombrando `name` (verificación: rojo hoy, que se escribe).
- [x] 2.2 Rust: un `name` de 121 caracteres contesta `400` nombrando `name` y sin repetir el valor (verificación: rojo hoy).
- [x] 2.3 Rust: un `container_name` de más de 128 caracteres contesta `400` nombrando `container_name` (verificación: rojo hoy).
- [x] 2.4 Rust: un `db_user`/`db_name`/`db_path`/`host_path` que supere su cota contesta `400` nombrando el campo (verificación: rojo hoy).
- [x] 2.5 Rust: un `db_name` informado y en blanco se guarda como `NULL` en la columna (verificación: rojo hoy, que contesta `400`).
- [x] 2.6 Rust: los campos se guardan **recortados** —`container_name` y `db_user` con espacios a los lados se almacenan sin ellos— (verificación: rojo hoy).

### GREEN

- [x] 2.7 `src/api/targets.rs`: `alta_desde` pasa por `texto::obligatorio`/`texto::opcional` (verificación: 2.1–2.6 en verde).
- [x] 2.8 `src/api/targets.rs`: se retiran `exigir_presente`, `opcional_no_vacio` y `vacio` si quedan sin uso (verificación: `cargo clippy --workspace --all-targets -- -D warnings` sin `dead_code`).

### REFACTOR

- [x] 2.9 `src/api/targets.rs`: doc de las cotas y del cambio de comportamiento de los opcionales en blanco (verificación: lectura).

## 3. Cotas y recorte en repositorios (SEC-13)

### RED

- [x] 3.1 Rust (`src/api/repositories.rs`): `name` vacío o demasiado largo → `400` nombrando `name` (verificación: rojo hoy).
- [x] 3.2 Rust: `bucket`/`region`/`endpoint`/`access_key_id` por encima de su cota → `400` nombrando el campo (verificación: rojo hoy).
- [x] 3.3 Rust: `bucket` con espacios a los lados se guarda recortado (verificación: rojo hoy).

### GREEN

- [x] 3.4 `src/api/repositories.rs`: el cuerpo pasa por `texto::obligatorio` con las cotas de cada campo (verificación: 3.1–3.3 en verde).

### REFACTOR

- [x] 3.5 `src/api/repositories.rs`: doc de las cotas (verificación: lectura).

## 4. El nombre del contenedor contra Docker (SEC-22)

### RED

- [x] 4.1 Rust (`src/api/targets.rs`): un `container_name` con `/`, `..` o `?` contesta `400` nombrando `container_name` (verificación: rojo hoy, que se escribe).
- [x] 4.2 Rust: un `container_name` válido para Docker —nombre, `_`, `.`, `-`, dígitos, un identificador corto hexadecimal— se acepta (verificación: verde hoy; se conserva como contrapeso).

### GREEN

- [x] 4.3 `src/api/texto.rs`: `nombre_de_contenedor` valida el juego de caracteres `[A-Za-z0-9][A-Za-z0-9_.-]*` (verificación: 4.1, 4.2 en verde).

## 5. La retención alineada con el motor (SEC-14)

### RED

- [x] 5.1 Rust (`src/api/jobs.rs`): un alta con `keep_daily = -5` contesta `400` nombrando `keep_daily` (verificación: rojo hoy, que se escribe).
- [x] 5.2 Rust: un alta con `keep_daily = -1` se acepta (verificación: verde hoy; se conserva como contrapeso).
- [x] 5.3 Rust: un alta con una regla por encima de `i32::MAX` contesta `400` (verificación: rojo hoy).

### GREEN

- [x] 5.4 `src/api/jobs.rs`: `retencion_que_conserva` rechaza `< -1` y `> i32::MAX` además de los tres ceros (verificación: 5.1–5.3 en verde).

### REFACTOR

- [x] 5.5 `src/api/jobs.rs`: `nombre_del_cuerpo` delega en `texto::obligatorio` (verificación: `cargo test --locked --bin kesto -- 'jobs::'` en verde).

## 6. Verificación de la feature

- [x] 6.1 `just check` (verificación: fmt + clippy + test en verde).
- [x] 6.2 `just check-all` (verificación: el análogo local del CI en verde).
- [ ] 6.3 Comprobación manual: un alta con un valor por encima de su cota se rechaza desde el panel con el campo señalado (verificación: navegación con `podman`).

## 7. Documentación y cierre

- [x] 7.1 `docs/FEATURE-046-endurecimiento-validacion-de-entrada.md`.
- [x] 7.2 Actualizar `docs/ARQUITECTURA.md` si la validación de entrada lo merece como invariante.
- [ ] 7.3 `openspec archive endurecimiento-validacion-de-entrada`.
- [ ] 7.4 Renombrar `plans/PLAN-046.md` a `plans/PLAN-046-COMPLETED.md`.
