# FEATURE-046 — Endurecimiento de la validación de entrada (SEC-13 + SEC-14 + `container_name` de SEC-22)

- **Issue**: #168 (bloque F de la auditoría #156), hallazgos SEC-13, SEC-14 y el NIT del `container_name` de SEC-22.
- **Change OpenSpec**: `openspec/changes/endurecimiento-validacion-de-entrada/`.
- **Plan**: `plans/PLAN-046.md`.
- **Rama**: `feature/endurecimiento-validacion-de-entrada` desde `development`.

## Qué es

Cierra tres huecos del borde HTTP de los recursos, que hasta ahora escribía en la base lo que le llegaba: los campos de texto no tenían cota ni se recortaban (`targets.name` ni se exigía), la retención solo se rechazaba si las tres reglas eran cero —y el motor rechaza además cualquier negativo que no sea `-1`—, y el `container_name` se interpolaba sin codificar en la API de Docker.

Los tres se resolvieron en el borde, que es donde se conoce la intención de quien escribe, para que el fallo aparezca en el formulario y no más tarde: en la primera copia, en una etiqueta de snapshot o en una petición desviada a la API de Docker.

## Ficheros implicados

| Fichero | Cambio |
|---|---|
| `src/api/texto.rs` | **Nuevo.** Módulo compartido: `obligatorio`, `opcional`, `nombre_de_contenedor` y las diez cotas por campo. |
| `src/api/mod.rs` | Registra `pub mod texto;`. |
| `src/api/targets.rs` | `alta_desde` pasa por el módulo (`name`, `container_name`, `db_user`, `db_name`, `db_path`, `host_path`); `container_name` validado contra Docker; se retiran `exigir_presente` y `opcional_no_vacio`. |
| `src/api/repositories.rs` | `campos_de_texto` recorta y acota `name`, `bucket`, `region`, `endpoint` y `access_key_id` en alta y edición; el `endpoint` se juzga **recortado**. |
| `src/api/jobs.rs` | `retencion_que_conserva` rechaza `< -1` y `> i32::MAX`; `nombre_del_cuerpo` delega en `texto::obligatorio`; se retira `NOMBRE_MAXIMO`. |
| `ui/src/components/ModalDeTarget/ModalDeTarget.tsx` | `maxLength` por campo de texto. |
| `ui/src/components/ModalDeRepositorio/ModalDeRepositorio.tsx` | `maxLength` por campo de texto. |
| `ui/src/components/ModalDeTarget/ModalDeTarget.test.tsx`, `.../ModalDeRepositorio/ModalDeRepositorio.test.tsx` | Tests de los topes. |

## Cómo funciona

### Cotas y recorte (`src/api/texto.rs`)

`obligatorio(campo, valor, maximo)` recorta los espacios de los extremos, exige que no quede vacío y acota; `opcional(campo, valor, maximo)` recorta y devuelve `None` si el campo falta o queda en blanco tras recortar. Los dos cuentan **caracteres**, no bytes —contar bytes rechazaría un nombre con acentos que el formulario deja escribir— y el `400` **nombra el campo** y **no repite el valor**.

Cotas: `name` 120 · `container_name` 128 · `db_user` 63 · `db_name` 63 · `bucket` 63 · `region` 64 · `access_key_id` 128 · `endpoint` 2048 · `db_path` 1024 · `host_path` 4096. Los campos de **secreto** (`secret_access_key`, `passphrase`, `db_password`) no se acotan ni se recortan.

### El opcional en blanco pasa a `NULL`

Hasta ahora `db_name: ""` contestaba `400`. Con el módulo es un opcional más: un `db_name` vacío o de solo espacios se guarda como `NULL`, que es lo que el §3 significa para «ausente» y lo que el motor espera. Es un cambio de comportamiento deliberado: una cadena vacía donde el esquema espera nulo es una errata, no un valor.

### El nombre del contenedor contra Docker (SEC-22)

bollard interpola el nombre **sin codificar** en la ruta de la petición a la API de Docker (`/containers/{container_name}/exec`), así que un nombre con `/`, `..` o `?` desviaba la petición a otro punto de la API. El borde exige ahora el juego de caracteres con el que Docker nombra y localiza contenedores —`[A-Za-z0-9][A-Za-z0-9_.-]*`—, que también cubre los identificadores cortos hexadecimales y no admite ningún carácter de ruta.

### La retención alineada con el motor (SEC-14)

El motor (`src/rustic/retencion.rs`) lee `-1` como «conservar todo» y rechaza cualquier otro negativo o lo que no cabe en un entero de 32 bits. El borde rechaza ahora **lo mismo**: `-1` y cualquier valor `>= 0` se aceptan; `< -1` o `> i32::MAX` contestan `400` nombrando la columna. Se mantiene el rechazo de los tres ceros. Las dos capas existen porque protegen de dos caminos distintos: el borde para el formulario, y el motor para una fila escrita por debajo de la API.

## Variables y configuración

Ninguna nueva.

## Cómo se despliega

Sin pasos de despliegue propios: el cambio es validación en el borde y `maxLength` en el panel. Las filas ya guardadas **no se tocan**; una que hoy tuviera un valor por encima de su cota sigue leyéndose, pero ya no se puede volver a escribir tal cual.

## Decisiones

- Un módulo compartido `src/api/texto.rs` en vez de repetir la validación en cada recurso (tres copias se separan con el tiempo —y ya se habían separado: `jobs` la tenía y los otros dos no—).
- El recorte se aplica **antes** de medir y antes de juzgar el `endpoint`, para que el tope se mida sobre lo que de verdad se guarda.
- El `container_name` se valida contra el juego de Docker en vez de codificar el valor en el cliente: lo que se escribe es un nombre que un contenedor puede tener, y el error sale en el formulario en vez de un `404` de Docker.
- La retención se alinea con el motor pero **no lo sustituye** (el guardarraíl del motor sigue vivo).
- `#[serde(default)]` en `name` para que un `name` ausente —no solo en blanco— conteste un `400` que nombre el campo.

## Follow-ups

- El `maxLength = 120` de `ModalDeTarea` es preexistente y sigue sin un test de atributo que lo cubra (los tests nuevos cubren `ModalDeTarget` y `ModalDeRepositorio`).
- La cota del `400` y el `maxLength` del panel son el mismo número por convención, pero no hay una fuente única compartida entre `src/api/texto.rs` y `ui/`; si la lista de cotas cambia, hay que tocar los dos lados.
