# Design — endurecimiento de la validación de entrada

## Contexto

El borde HTTP de los recursos (`src/api/targets.rs`, `src/api/repositories.rs`, `src/api/jobs.rs`) escribe en la base lo que le llega. Solo `jobs.name` pasa por `nombre_del_cuerpo` (`src/api/jobs.rs:344`), que recorta y aplica `NOMBRE_MAXIMO = 120`; el resto de campos de texto se copian tal cual. El motor (`src/rustic/retencion.rs`) y el cliente de Docker (`src/docker/cliente.rs`) confían en lo que la base tenga.

## SEC-13 — cotas y recorte

### Alternativas

1. **Reutilizar `nombre_del_cuerpo` de `jobs.rs`.** Es privado de `jobs` y está atado a la forma de `AltaDeTareaDelCable`; no sirve para los demás recursos. Descartada.
2. **Repetir la validación en cada recurso.** Tres copias de la misma regla, que se separan con el tiempo —y ya se han separado: `jobs` la tiene y los otros dos no—. Descartada.
3. **Un módulo compartido `src/api/texto.rs`.** Una función que recorta y acota, usada por los tres recursos. **Elegida.**

### Decisión

`src/api/texto.rs` expone tres funciones y las cotas por campo:

- `obligatorio(campo, valor: Option<&str>, maximo) -> Result<String, AppError>`: recorta, exige no vacío y acota.
- `opcional(campo, valor: Option<&str>, maximo) -> Result<Option<String>, AppError>`: recorta; ausente o en blanco tras recortar → `None`; si no, acota.
- `nombre_de_contenedor(valor: Option<&str>) -> Result<Option<String>, AppError>`: como `opcional` más el juego de caracteres de Docker (abajo).

Las cotas viven como constantes con nombre, para que el mensaje del `400` y el `maxLength` del panel no puedan separarse:

| campo | cota | porqué |
|---|---|---|
| `name` | 120 | la misma que `jobs.name`; una sola noción de «nombre de recurso» |
| `container_name` | 128 | holgado para un nombre o un identificador de Docker |
| `db_user` | 63 | el máximo de identificador de PostgreSQL (MySQL, 32) |
| `db_name` | 63 | el máximo de nombre de base de PostgreSQL (MySQL, 64) |
| `bucket` | 63 | el máximo real de un cubo de S3 |
| `region` | 64 | holgado para una región de AWS |
| `access_key_id` | 128 | holgado para un identificador de credencial |
| `endpoint` | 2048 | holgado para una URL de servicio S3 |
| `db_path` | 1024 | holgado para una ruta dentro de un contenedor |
| `host_path` | 4096 | el máximo de ruta de Linux |

### El recorte de los opcionales en blanco

Hoy `opcional_no_vacio` (`src/api/targets.rs:234`) rechaza `db_name: ""`; con el módulo, un opcional en blanco tras recortar pasa a `NULL`, que es lo que el §3 significa para «ausente» y lo que el motor espera. Es un cambio de comportamiento deliberado: una cadena vacía donde el esquema espera nulo es una errata, no un valor.

### `targets.name` y `repositories.name`

Ninguno de los dos se valida hoy: se copian a la fila sin recorte ni cota, y un `name` vacío pasa. Se les aplica `obligatorio(..., 120)`, igual que a `jobs.name`.

## SEC-14 — la retención alineada con el motor

El motor lee `-1` como «conservar todo» y rechaza cualquier otro negativo, además de lo que no cabe en `i32`. El borde tiene que rechazar **lo mismo** que el motor no leería, para que el fallo aparezca en el formulario y no en la primera copia.

### Alternativas

1. **Rechazar todo negativo, `-1` incluido.** Rompe las configs que usan `-1` para «conservar todo», que el motor hoy acepta. Descartada.
2. **Dejar el borde como está y solo endurecer el motor.** No cierra SEC-14. Descartada.
3. **Alinear con el motor: `-1` válido, `< -1` o `> i32::MAX` → `400`.** **Elegida.**

## `container_name` — el juego de caracteres de Docker

bollard interpola el nombre en la ruta sin codificar (`/containers/{container_name}/exec`). Un nombre con `/`, `..` o `?` mueve la petición a otro punto de la API de Docker. El borde exige el juego de caracteres con el que Docker nombra y localiza contenedores —`[A-Za-z0-9][A-Za-z0-9_.-]*`—, que también cubre los identificadores cortos (hexadecimales). Un nombre válido para Docker no puede llevar caracteres de ruta.

### Alternativas

1. **Codificar el nombre en el cliente de Docker.** Arregla la ruta, pero deja entrar un nombre que ningún contenedor tendrá, y el fallo se traslada a un `404` de Docker difícil de leer. Descartada como arreglo principal; el borde es donde se conoce la intención de quien escribe.
2. **Validar el juego de caracteres en el borde.** El nombre que se escribe es el que Docker admite, y el error sale en el formulario. **Elegida.**

## Impacto en la compatibilidad

- Un valor con más de su cota deja de aceptarse (endurecimiento). Las filas ya guardadas no se tocan.
- Un opcional en blanco (`db_name`) pasa de `400` a `NULL`.
- Un `container_name` con caracteres fuera del juego de Docker pasa a `400`.
