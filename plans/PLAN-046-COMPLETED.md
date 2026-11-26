# PLAN-046 — Endurecimiento de la validación de entrada (SEC-13 + SEC-14 + container_name de SEC-22)

- **Issue**: #168 (bloque F de la auditoría #156), hallazgos SEC-13, SEC-14 y el `container_name` de SEC-22.
- **Change OpenSpec**: `openspec/changes/endurecimiento-validacion-de-entrada/`.
- **Documento de la feature**: `docs/FEATURE-046-endurecimiento-validacion-de-entrada.md`.
- **Rama**: `feature/endurecimiento-validacion-de-entrada` desde `development`.

## Objetivo

Cerrar tres huecos del borde HTTP que hoy dejan entrar valores que ni el esquema acota ni el motor sabe leer:

1. **SEC-13** — los campos de texto de `/targets` y `/repositories` no tienen cota ni recorte (y `targets.name` ni se exige).
2. **SEC-14** — la retención del borde solo rechaza los tres ceros; el motor rechaza además los negativos distintos de `-1`.
3. **`container_name` (SEC-22)** — bollard interpola el nombre sin codificar en la ruta de la API de Docker.

## Alcance

- `src/api/texto.rs` (nuevo) — recorte y cotas por campo, y el juego de caracteres de Docker para `container_name`.
- `src/api/targets.rs` — el cuerpo pasa por el módulo nuevo.
- `src/api/repositories.rs` — el cuerpo pasa por el módulo nuevo.
- `src/api/jobs.rs` — la retención se alinea con el motor y `nombre_del_cuerpo` delega en el módulo común.
- Specs: `api-resources` (requisitos ADDED de cotas/recorte, `container_name` y retención negativa).

Fuera de alcance: el esquema, el motor, los campos de secreto y el resto de SEC-22.

## Decisiones clave

- Un módulo compartido `src/api/texto.rs` en vez de repetir la validación en cada recurso.
- Cotas en caracteres, no en bytes: contar bytes rechazaría un nombre con acentos.
- Un opcional en blanco (`db_name`) pasa a `NULL` (el §3 no tiene cadena vacía).
- El `container_name` se valida en el borde contra el juego de Docker `[A-Za-z0-9][A-Za-z0-9_.-]*`, cerrando la inyección de ruta.
- La retención acepta `-1` (conservar todo) y `>= 0`; rechaza `<-1` y `> i32::MAX`, como el motor.
- Sin dependencias nuevas.

## Tareas

### 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check`.
- [x] 0.2 Rama desde `development`: `just gf-feature endurecimiento-validacion-de-entrada`.
- [x] 0.3 Change aprobado por el usuario.

### 1. Módulo de texto compartido

- [x] 1.1 RED — tests unitarios de `src/api/texto.rs` para `obligatorio` y `opcional`.
- [x] 1.2 GREEN — las tres funciones y las constantes de cota.

### 2. Cotas y recorte en targets (SEC-13)

- [x] 2.1 RED — `name` en blanco → `400` nombrando `name`.
- [x] 2.2 RED — `name` de 121 caracteres → `400` sin repetir el valor.
- [x] 2.3 RED — campos por encima de su cota → `400` nombrando el campo.
- [x] 2.4 RED — `db_name` en blanco se guarda como `NULL`.
- [x] 2.5 RED — los campos se guardan recortados.
- [x] 2.6 GREEN — `alta_desde` pasa por `texto::obligatorio`/`texto::opcional`.
- [x] 2.7 GREEN — se retiran los helpers sin uso.
- [x] 2.8 REFACTOR — doc de las cotas y del cambio de los opcionales.

### 3. Cotas y recorte en repositorios (SEC-13)

- [x] 3.1 RED — `name` vacío o largo → `400`.
- [x] 3.2 RED — `bucket`/`region`/`endpoint`/`access_key_id` por encima de su cota → `400`.
- [x] 3.3 RED — `bucket` con espacios se guarda recortado.
- [x] 3.4 GREEN — el cuerpo pasa por `texto::obligatorio` con las cotas.
- [x] 3.5 REFACTOR — doc de las cotas.

### 4. El nombre del contenedor contra Docker (SEC-22)

- [x] 4.1 RED — `container_name` con `/`, `..` o `?` → `400` nombrando `container_name`.
- [x] 4.2 RED — un `container_name` válido se acepta.
- [x] 4.3 GREEN — `texto::nombre_de_contenedor` valida el juego de Docker.

### 5. La retención alineada con el motor (SEC-14)

- [x] 5.1 RED — `keep_daily = -5` → `400` nombrando `keep_daily`.
- [x] 5.2 RED — `keep_daily = -1` se acepta.
- [x] 5.3 RED — una regla por encima de `i32::MAX` → `400`.
- [x] 5.4 GREEN — `retencion_que_conserva` rechaza `<-1` y `> i32::MAX`.
- [x] 5.5 REFACTOR — `nombre_del_cuerpo` delega en `texto::obligatorio`.

### 6. Verificación

- [x] 6.1 `just check`.
- [x] 6.2 `just check-all`.
- [x] 6.3 Comprobación manual desde el panel (verificada contra el servidor en marcha con `curl`: cotas y recorte confirmados).

### 7. Documentación y cierre

- [x] 7.1 `docs/FEATURE-046-endurecimiento-validacion-de-entrada.md`.
- [x] 7.2 Actualizar `docs/ARQUITECTURA.md` si procede.
- [ ] 7.3 `openspec archive endurecimiento-validacion-de-entrada`.
- [ ] 7.4 Renombrar este plan a `plans/PLAN-046-COMPLETED.md`.

## Criterios de aceptación

- Todo campo de texto de `/targets` y `/repositories` se guarda recortado y sujeto a su cota.
- `targets.name` y `repositories.name` no pueden quedar vacíos.
- `container_name` con caracteres fuera del juego de Docker contesta `400`.
- Una tarea con una regla de retención `< -1` o `> i32::MAX` contesta `400`; `-1` se sigue aceptando.
- `just check-all` en verde.
