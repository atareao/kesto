# Proposal — endurecimiento de la validación de entrada

**SEC-13** y **SEC-14**, más el NIT del `container_name` de **SEC-22**, del bloque **F** de la auditoría de seguridad **#156** (#168).

## Why

El borde HTTP de los recursos deja pasar valores que ni el esquema acota ni el motor sabe leer, y el fallo aparece más tarde y más lejos: en la primera copia, o en la API de Docker.

1. **SEC-13 (sin cota ni recorte).** `POST`/`PUT` de `/repositories` y `/targets` guardan los campos de texto tal cual llegan: `name`, `container_name`, `db_user`, `db_name`, `bucket`, `region`, `endpoint`, `db_path` y `host_path` no tienen cota ni se recortan, y `targets.name` ni siquiera se exige no vacío. Solo `jobs.name` recorta y acota (`NOMBRE_MAXIMO = 120`, `src/api/jobs.rs:320`).
2. **SEC-14 (retención que el motor no lee).** El motor trata `-1` como «conservar todo» y rechaza cualquier otro negativo (`src/rustic/retencion.rs:290-307`), pero el borde (`src/api/jobs.rs:282-293`) solo rechaza los tres ceros: una tarea con `keep_daily = -5` se escribe y la retención no falla hasta la primera copia.
3. **`container_name` (SEC-22).** bollard interpola el nombre en la ruta sin codificar (`/containers/{container_name}/exec`, `bollard-0.18.1/src/exec.rs:149`), así que un `container_name` con `/`, `..` o `?` desvía la petición a otro punto de la API de Docker.

## What Changes

### SEC-13 — cotas y recorte en el borde

- Un módulo de texto compartido (`src/api/texto.rs`) recorta los extremos de cada campo de texto y aplica una cota por campo, con un `400` que **nombra el campo** y **no repite el valor**.
- Cotas: `name` 120; `container_name` 128; `db_user`, `db_name` y `bucket` 63; `region` 64; `access_key_id` 128; `endpoint` 2048; `db_path` 1024; `host_path` 4096.
- Un campo **obligatorio** que tras recortar quede vacío sigue siendo un `400`; un campo **opcional** que tras recortar quede vacío pasa a guardarse como `NULL` (hoy se rechaza).
- `targets.name` y `repositories.name` pasan a exigirse **no vacío** y con la cota de 120.
- `jobs.name` reutiliza el mismo módulo en lugar de su copia local.

### SEC-14 — la retención se valida como la lee el motor

- El borde recibe `-1` como «conservar todo» y cualquier valor `>= 0`; rechaza con `400`, nombrando la columna, un valor `< -1` o `> i32::MAX`.
- Se mantiene el rechazo de los tres ceros (`retencion_que_conserva`).

### `container_name` — validado contra el juego de Docker

- `container_name` SHALL cumplir `[A-Za-z0-9][A-Za-z0-9_.-]*`; si no, `400` nombrando el campo. Cierra la inyección de ruta en la API de Docker.

## Non-goals

- No se toca el esquema ni el motor.
- No se añaden cotas a los campos de secreto (`secret_access_key`, `passphrase`, `db_password`).
- No se cambia el contrato de respuesta ni las rutas.

## New Capabilities

_(ninguna)_

## Modified Capabilities

- `api-resources`: los campos de texto se recortan y tienen cota; `targets.name` y `repositories.name` se exigen; `container_name` se valida contra el juego de caracteres de Docker; la retención del borde se alinea con el motor.

## Impact

- **Código**: `src/api/texto.rs` (nuevo), `src/api/targets.rs`, `src/api/repositories.rs`, `src/api/jobs.rs`.
- **API**: nuevas cotas y recortes; un opcional en blanco (`db_name`) pasa a `NULL`; un `container_name` con caracteres fuera del juego de Docker pasa a `400`.
- **Despliegue**: sin variables nuevas.
- **Dependencias**: ninguna nueva.
