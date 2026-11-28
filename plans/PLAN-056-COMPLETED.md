# Plan 056 – Parada opcional de contenedores antes del backup

Issue #210

## Objetivo

Añadir capacidad opcional para detener uno o más contenedores antes de una copia, garantizar una instantánea consistente y reiniciarlos al terminar, tanto si la copia sale bien como si falla.

## Arquitectura

No hay bandera de línea de comandos: Kesto es un servidor y no tiene comando `backup`. La lista se declara con `KESTO_STOP_CONTAINERS`, variable separada por comas que leen **la API y el motor** con el mismo valor. La API valida cada nombre en el borde de configuración, lo pasa a los ajustes del motor (`Ajustes::con_stop_containers`) y detiene/arranca a través del socket IPC del proceso privilegiado; el motor repite la validación y solo detiene contenedores que estén en su lista. No se usa `DOCKER_HOST` desde la API.

La orquestación vive en `src/engine/ejecucion.rs`: `ejecutar_con` inyecta el control de contenedores y la copia. Si el `stop` falla, la fila acaba en `failed` con `finished_at` y se reinician los contenedores ya detenidos; si falla la copia o entra en pánico, una guardia RAII reinicia todos los detenidos. Un reinicio fallido se guarda como aviso no fatal en `error_message` y se publica como evento. Un candado de proceso serializa la operación global entre workers de repositorios distintos.

## Tareas

### Tarea 1: Definir cambio de OpenSpec
- **Archivos:** `openspec/changes/backup-container-stop/`
- **Descripción:** Redactar propuesta y spec delta con los escenarios sin contenedores, con contenedores y con fallo de copia.
- **Estado:** completada.

### Tarea 2: Leer y validar `KESTO_STOP_CONTAINERS`
- **Archivos:** `src/config.rs`, `src/motor/config.rs`
- **Descripción:** Lista separada por comas, espacios recortados, duplicados descartados; nombre ASCII alfanumérico con `_`, `.` o `-` y primer carácter alfanumérico. Un nombre inválido impide el arranque y no publica el valor.
- **Estado:** completada.

### Tarea 3: Operaciones stop/start sobre Bollard y por IPC
- **Archivos:** `src/docker/cliente.rs`, `src/docker/ipc_cliente.rs`, `src/ipc/mensaje.rs`, `src/motor/servidor.rs`
- **Descripción:** Añadir `stop_container`/`start_container` al cliente del daemon y al cliente IPC; nuevas solicitudes `DetenerContenedor`/`IniciarContenedor` en la lista blanca, con comprobación de la lista autorizada en el motor antes de detener.
- **Estado:** completada.

### Tarea 4: Orquestar stop/copia/reinicio y abortar si el stop falla
- **Archivos:** `src/engine/ejecucion.rs`, `src/engine/mod.rs`, `src/main.rs`
- **Descripción:** Guardia RAII, cerrar la fila en `failed` si el stop falla, aviso no fatal si el reinicio falla, candado global por worker y paso de la configuración desde `main`.
- **Estado:** completada.

### Tarea 5: Tests sin Docker
- **Archivos:** `src/engine/ejecucion.rs`, `src/config.rs`, `tests/sec_06_separacion_privilegios.rs`
- **Descripción:** Escenarios de orquestación con control inyectado y base SQLite real; lectura de la variable con casos ausente, vacía, espacios, comas, duplicados e inválidos; cobertura del mensaje IPC nuevo.
- **Estado:** completada.

### Tarea 6: Documentación
- **Archivos:** `docs/ENTORNO.md`, `docs/FEATURE-056-backup-container-stop.md`, `compose.yaml`
- **Descripción:** Documentar la variable, el alcance y las consideraciones de consistencia; pasar la misma lista a los dos servicios del `compose`.
- **Estado:** completada.

### Tarea 7: Verificación y revisión
- **Archivos:** Flujo git habitual
- **Descripción:** `cargo fmt`, `git diff --check`, `cargo clippy` (workspace y binario `kesto-motor` con `motor`), `cargo test --locked --workspace` y PR a `development`.
- **Estado:** verificación local completada; PR pendiente.

## Criterios de aceptación

- La configuración acepta `KESTO_STOP_CONTAINERS` y valida los nombres de contenedor.
- El backup detiene los contenedores especificados, realiza la copia y los reinicia, tanto si acaba bien como si falla; el pánico de la copia también los reinicia.
- Un fallo del stop cierra la fila en `failed` con `finished_at`; un fallo del reinicio se avisa sin cambiar el éxito de la copia.
- Ningún contenedor queda detenido por una ejecución terminada, salvo aviso explícito.
- Las nuevas pruebas y los checks del CI pasan; documentación actualizada.

## Riesgos / Consideraciones

- El proceso `kesto-motor` necesita permisos sobre el socket del daemon; la API solo tiene el socket IPC.
- Si falla el stop o el start, el desenlace queda registrado y, cuando procede, publicado al panel.
- El impacto de rendimiento es mínimo: stop/start son operaciones rápidas y serializadas.
