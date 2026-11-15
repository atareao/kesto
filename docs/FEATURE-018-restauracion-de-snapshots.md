# FEATURE-018 — Restauración de snapshots

- **Issue:** [#64](https://github.com/atareao/kesto/issues/64).
- **Plan:** `plans/PLAN-018.md`.
- **Change SDD:** `openspec/changes/restauracion-de-snapshots/`.

## Qué es

Cierra el ciclo de una copia: hasta ahora el panel solo **listaba** snapshots (`GET /api/v1/snapshots`) y no había forma de devolver los datos. Esta feature añade la operación inversa —**restaurar un snapshot completo** desde el panel sobre el **target del que procede**— con confirmación previa, respuesta inmediata y aviso de fase por el stream SSE.

Dos cambios de contrato acompañan a la operación, decididos tras una auditoría (D1 y D2): el **listado** publica por fila el target del que procede cada instantánea (`target_id`, `target_name`, `target_type`, anulables), y el endpoint de restauración comprueba **antes de responder** que la instantánea existe en el repositorio, devolviendo `404` de verdad si no está. El progreso, en cambio, se declara como lo que es (D3): un aviso de fase, sin contadores.

Alcance de la **v1**:

- Solo targets de tipo **`directory`**: el árbol del snapshot se restaura en el `host_path` del target con `LocalDestination`.
- Se restaura la instantánea **completa**, no por rutas.
- Sin borrado destructivo: `delete: false`.

Fuera de alcance (fase 2, issue [#67](https://github.com/atareao/kesto/issues/67)):

- Restaurar targets de **base de datos** (`mariadb`, `postgresql`, `sqlite`). El volcado se hizo con un `exec` dentro del contenedor del target, así que la restauración real exige el reverso —inyectar el volcado en el contenedor o escribir el `.db`—, con el riesgo de pisar datos vivos que eso conlleva. Se rechaza de forma explícita con `422`.
- Restauración **parcial por rutas** y **borrado/purga** de snapshots.

## Ficheros implicados

| Fichero | Qué aporta |
|---|---|
| `src/db/entities/execution.rs` | `ObjetivoDeRestauracion` y `Ejecuciones::objetivo_de_la_instantanea(snapshot_id, repository_id)`: resuelve `snapshot_id → job_executions → backup_jobs → backup_targets` con una única consulta (el mismo vínculo que usa el listado para rellenar el target de cada fila, D1). |
| `src/rustic/repositorio.rs` | `Repositorio::restaurar_snapshot(self, instantanea, destino_local)`: abre el índice completo, resuelve la instantánea y la escribe con `prepare_restore` + `restore`; y la comprobación de que la instantánea existe antes de restaurarla (D2). |
| `src/rustic/error.rs` | Variante `Error::RestauracionFallida { destino, instantanea, motivo }` (y la que corresponda a «instantánea no encontrada» para el `404`, D2). |
| `src/engine/mod.rs` | `Motor::restaurar(acceso, instantanea, destino_local, execution_id)`: publica el primer aviso, restaura **fuera del runtime** y publica el cierre; guarda contra restauraciones concurrentes sobre el mismo destino. |
| `src/engine/hub.rs` | `EventoDeRestauracion` (siete campos), `EventoDelMotor` (el enum que reparte ejecución o restauración) y `Hub::publicar_restauracion`. |
| `src/api/snapshots.rs` | Ruta `POST /api/v1/snapshots/{id}/restore` y el manejador `restaurar` con la envoltura de `api-shell`; el `404` real antes de responder (D2); y el DTO del listado con los tres campos nuevos (D1). |
| `migrations/0004_….sql` | Índice sobre `job_executions.snapshot_id` (solo índice; sin cambios de columnas). |
| `src/api/events.rs` | Evento SSE `restore_progress` con `NOMBRE_DEL_EVENTO_DE_RESTAURACION` y su DTO de cable. |
| `src/error.rs` | Variante `AppError::UnprocessableEntity` → `422`. |
| `src/scheduler/disparo.rs` | Solo pruebas: el receptor del hub pasa a transportar `EventoDelMotor`, así que se saltan los avisos de restauración. |
| `ui/src/pages/snapshots.tsx` | Acción «Restaurar» por fila, `Popconfirm` de confirmación (con el target del listado) y panel de **fase**; acción deshabilitada para filas sin target o de tipo no restaurable. |
| `ui/src/hooks/useSnapshots.ts` | `useRestaurarSnapshot()` (`useMutation`). |
| `ui/src/api/snapshots.ts` | Cliente `restaurarSnapshot(repositoryId, snapshotId)` y el tipo del listado ampliado. |
| `ui/src/services/api/types.ts` | Tipos `Restauracion` y `EventoDeRestauracion`. |
| `ui/src/services/useEventStream.ts` | `NOMBRE_DEL_EVENTO_DE_RESTAURACION`, mapa `restauraciones` y hook `useRestauracion(executionId)`. |
| `ui/src/services/events.tsx` | Interpretación y validación del JSON de `restore_progress` en el proveedor SSE. |

No hay cambios de **columnas**: el mapeo usa `job_executions`, `backup_jobs` y `backup_targets` existentes (§3 del SRS). Lo único que toca el esquema es la migración que añade el índice `snapshot_id` (D1). No hay cambios en `src/api/mod.rs`: la ruta entra por el `router()` del propio módulo de snapshots.

## Cómo funciona

1. **Listado con el target (D1).** `GET /api/v1/snapshots?repository_id=<id>` no solo lista las instantáneas del repositorio: para cada una resuelve su vínculo `snapshot → job_executions → backup_jobs → backup_targets` y añade `target_id`, `target_name` y `target_type`. Si la instantánea no casa con ninguna ejecución, tarea ni target —copia huérfana, hecha por otra herramienta o con la fila borrada en cascada—, los tres campos van a `null` y la fila **sigue** en el listado. El vínculo se resuelve por fila (una consulta por instantánea o una unión externa; el índice `snapshot_id` mantiene el coste acotado); un fallo de base de datos al resolverlo no se confunde con «no hay vínculo».
2. **Petición de restauración.** El panel envía `POST /api/v1/snapshots/{id}/restore` con `{ "repository_id": "<uuid>" }`. El `id` de la instantánea va en el **camino** (es el recurso sobre el que se actúa) y el repositorio en el **cuerpo** (es el contexto desde el que se lee; la misma instantánea puede estar en dos repositorios). El cuerpo es `deny_unknown_fields`.
3. **Comprobación en el almacenamiento (D2).** Antes de contestar nada, el manejador abre el repositorio y comprueba que la instantánea **existe de verdad** en él; si no está, responde `404` y **no** lanza ninguna ejecución ni publica ningún aviso. El `200` deja de ser optimista.
4. **Resolución del vínculo.** El manejador valida el UUID, lee la fila del repositorio (`404` si no está) y llama a `objetivo_de_la_instantanea(snapshot_id, repository_id)`. Una sola consulta une `job_executions.snapshot_id` → `backup_jobs.job_id` → `backup_targets.id`, filtra por `repository_id` y toma la más reciente (`ORDER BY e.started_at DESC, e.id DESC LIMIT 1`). La instantánea no guarda el nombre de su tarea, así que este es el único camino fiable del snapshot a su target. Sin vínculo → `409`; un fallo de base de datos no se confunde con «no hay vínculo» (`Err` frente a `Ok(None)`).
5. **Validación de tipo.** Si el `target_type` **no** es `directory` → `422` con el motivo. Si lo es pero faltara el `host_path` —el `CHECK` del §3 lo exige, pero no se asume— → `422` con su propio motivo.
6. **Ejecución en segundo plano.** Se acuña un `execution_id = Uuid::new_v4()` —**efímero**, no se persiste— y se lanza el trabajo en un `tokio::spawn`, tras pasar por la **guarda de concurrencia** por destino (dos restauraciones del mismo target a la vez escribirían el mismo árbol); el fallo se registra con `tracing::error!` (ya no hay a quién responderlo). El `200` vuelve de inmediato.
7. **Restauración.** `Motor::restaurar` publica **antes** de tocar el repositorio el primer `restore_progress` (fase `restaurando`, contadores a 0) —el stream es global y el primer fotograma debe verse aunque el cliente acabe de suscribirse— y saca el trabajo del runtime con `fuera_del_runtime`, porque la biblioteca construye su propio runtime para el backend S3 y llamar desde dentro de otro entraría en pánico. `restaurar_snapshot` pasa al **índice completo** (`to_indexed`), resuelve la instantánea por su hex completo, recorre el árbol desde el nodo raíz, crea el destino si no existe y escribe con `prepare_restore` + `restore`. Publica el aviso de cierre —fase `terminada` si termina bien y `fallida` si falla— **fuera del cierre bloqueante**, de modo que el desenlace llega aunque el trabajo falle o el cliente se suscriba tarde.
8. **Respuesta.** `200` con `data: { execution_id, snapshot_id, target_id, target_name }`.

Decisiones de restauración: `delete: false` (lo que hay en el destino y no está en la instantánea **no** se borra) y `verify_existing: true` (un fichero que ya está se compara por contenido, no por fecha y tamaño).

Nota de v1 (limitación declarada, D3): el motor publica **dos** avisos —arranque y cierre— y en ambos los cuatro contadores (`files_done/total`, `bytes_done/total`) van a `0`, porque `Repository::restore` de `rustic_core` 0.13 no ofrece callback de progreso por su API pública. El aviso es un «empezó/terminó» con fase, **no** un avance incremental: el panel pinta la **fase** (en curso / terminada / fallida) sin barra determinada y sin porcentaje.

## Contrato de API

### Listado (D1)

`GET /api/v1/snapshots?repository_id=<id>`. Cada elemento de `data` añade, a los campos que ya publicaba (`id`, `time`, `paths`, `label`, `files`, `bytes`):

| Campo | Tipo | Cuándo |
|---|---|---|
| `target_id` | uuid con guiones o `null` | Target del que procede la instantánea, o `null` si no casa con ninguna ejecución/tarea/target del repositorio. |
| `target_name` | cadena o `null` | Nombre legible de ese target; `null` en el mismo caso. |
| `target_type` | cadena o `null` | Tipo del target (`directory`, `mariadb`, `postgresql`, `sqlite`); `null` en el mismo caso. El panel lo usa para saber si la fila es restaurable. |

`null` es un valor del contrato, no un error: la instantánea huérfana sigue en el listado y no contesta `404` ni `409` por aparecer.

### Restauración

`POST /api/v1/snapshots/{id}/restore`

Cuerpo: `{ "repository_id": "<uuid>" }`.

Respuesta OK (`200`):

```json
{
  "status": 200,
  "message": "restauración lanzada",
  "data": {
    "execution_id": "<uuid>",
    "snapshot_id": "<hex completo>",
    "target_id": "<uuid>",
    "target_name": "..."
  }
}
```

| Código | Cuándo | Detalle |
|---|---|---|
| `200` | La instantánea existe en el repositorio y la restauración se ha aceptado y lanzado. | Devuelve la ejecución **efímera**, el snapshot y el target de origen. |
| `400` | El `repository_id` del cuerpo no es un UUID. | `AppError::InvalidInput`; nombra el campo, nunca el valor. |
| `404` | No existe un repositorio con ese identificador **o** la instantánea no existe en el almacenamiento de ese repositorio (D2). | Sin repo no hay destino que abrir; sin instantánea no hay nada que restaurar. Se comprueba **antes** de responder y no se lanza nada. |
| `409` | La instantánea **existe** y no casa con ninguna ejecución/target de ese repositorio. | `AppError::Conflict`; la copia está, lo que falta es el vínculo. |
| `422` | El target no es un directorio (BD, fuera de v1) o un `directory` sin `host_path`. | `AppError::UnprocessableEntity`; «todavía no», distinto de «no se encuentra». |
| `500` | Fallo de base de datos al resolver el vínculo. | `AppError::Database`. |
| `502` | Servicio externo (p. ej. el backend S3) no contesta al abrir el repositorio. | `AppError::BadGateway`. |
| `503` | El motor no acepta trabajo (apagándose, cola llena). | `AppError::ServiceUnavailable`. |

Evento SSE en `GET /api/v1/events`, `event: restore_progress`:

| Campo | Tipo | Contenido |
|---|---|---|
| `execution_id` | uuid con guiones | La ejecución de restauración efímera. |
| `snapshot_id` | cadena | La instantánea que se restaura. |
| `phase` | cadena | `restaurando` (arranque), `terminada` (cierre correcto) o `fallida` (cierre con error). |
| `files_done` | entero | Ficheros restaurados hasta el momento (0 en v1). |
| `files_total` | entero | Total de ficheros, `0` si no se conoce (0 en v1). |
| `bytes_done` | entero | Bytes restaurados hasta el momento (0 en v1). |
| `bytes_total` | entero | Total de bytes, `0` si no se conoce (0 en v1). |

El manejador **no** espera al resultado: la restauración corre en segundo plano y por eso el desenlace no viaja en la respuesta HTTP, sino en los avisos del stream. El evento `restore_progress` es un añadido a la spec: el §5 solo fija `execution_progress`, y aquí se publica un segundo nombre de evento con su propio payload de siete campos.

El evento describe el **empezó/terminó** con `phase`, y los cuatro contadores van **siempre a `0`**: `Repository::restore` de `rustic_core` 0.13 no ofrece callback de progreso por su API pública, así que no hay de dónde leerlos. El contrato **no** depende de ellos —un `0` en `files_total` significa «no se conoce», no «cero ficheros»— y por eso el panel refleja la fase y no un porcentaje. Queda como limitación explícita y follow-up (ver abajo y D3).

## Variables / configuración

- **Ninguna variable de entorno nueva.** La única migración es la que añade el **índice** `snapshot_id` sobre `job_executions` (D1): no hay tabla de restauraciones, no se persiste la ejecución efímera y no cambia ninguna columna.
- Para descifrar las credenciales del repositorio se usa el mismo camino que el resto de operaciones: la clave derivada de **`KESTO_MASTER_KEY`** (§ cifrado en reposo). Rotarla dejaría ilegibles los secretos ya guardados, como en cualquier otra operación.

## Cómo habilitar / desplegar

No hay nada que activar. La ruta se cuelga del router de negocio como el resto de `/api/v1/*`: **protegida por OIDC** cuando el modo está habilitado, y **abierta** cuando el modo es `Disabled` (sin las tres `OIDC_*`, `/api/v1/*` queda abierto). La acción aparece sola en la pantalla de snapshots del panel; no hay *feature flag*.

## Decisiones

- **D1 — El listado publica el target por instantánea.** `target_id`, `target_name` y `target_type` viajan en cada elemento, anulables. La confirmación del panel necesita **nombrar el destino** y la acción necesita saber si la fila es restaurable; sin estos campos, el panel confirmaría a ciegas y ofrecería restaurar lo que la API va a rechazar. Los tres van a `null` cuando no hay vínculo (copia huérfana, de otra herramienta o con la fila borrada en cascada) porque la ausencia de target es un dato del contrato, no un error: la fila sigue en el listado.
- **D2 — `404` de verdad si la instantánea no está.** El handler comprueba contra el almacenamiento **antes** de responder. El `200` optimista prometía un trabajo contra una instantánea inexistente; ahora la ausencia se distingue de «está pero no sé sobre qué restaurarla» (`409`) y de «está pero su tipo no se restaura» (`422`).
- **D3 — El progreso es un aviso de fase, no un avance.** El evento publica fase (`restaurando` / `terminada` / `fallida`) y los cuatro contadores a `0`; `rustic_core` 0.13 no expone callback de progreso. Se declara la limitación en vez de prometer una barra que no existe.
- **`409` frente a `404`.** Una instantánea **presente** sin vínculo es un **conflicto con el estado del sistema**, no un recurso ausente: el `404` haría creer al panel que la copia no está, cuando sí está. El `404` se reserva para lo que de verdad falta —el repositorio o la instantánea en el almacenamiento—.
- **`422` frente a `409`.** Aquí el vínculo **existe** y la petición es correcta; lo que no se puede cumplir es la operación para ese tipo de target en esta versión. Eso es una entidad que no se puede procesar por su tipo (`422`), no un choque con el estado (`409`), y permite al panel decir «todavía no» en vez de «no existe».
- **La limitación se declara en la fila, no en el error.** Si `target_type` es `null` o no es `directory`, el panel deshabilita la acción y explica por qué, en vez de dejar que el operador confirme y reciba el rechazo. La restaurabilidad y la confirmación usan el **mismo** dato del listado, así que no pueden divergir.
- **`execution_id` efímero.** No hay tabla de restauraciones ni migración, y el progreso viaja por SSE, no por la base. El identificador solo sirve para correlacionar los `restore_progress` con la pantalla que los pidió.
- **Restauración en segundo plano.** Restaurar puede tardar minutos; si el manejador esperara, el cliente tendría la respuesta bloqueada. El contrato devuelve la **ejecución**, no su resultado, así que se lanza y se responde ya. La fase terminal se publica fuera del cierre bloqueante para que el desenlace llegue igualmente.
- **Guarda de concurrencia por destino.** Dos restauraciones del mismo target a la vez escriben el mismo árbol; la segunda se rechaza.
- **`delete: false`.** Es la única opción defendible por defecto: `delete: true` borraría del host todo lo que la instantánea no contenga, y una restauración lanzada sobre el directorio equivocado se llevaría por delante datos sin copia. `verify_existing: true` se activa porque solo cuesta relectura y evita dar por bueno un fichero que cambió sin que la fecha lo delate.
- **El motor no lee la base.** Quien resuelve la fila `snapshot → ejecución → target` es el **manejador** (capa de acceso); el motor solo abre el repositorio y restaura. Así el motor no conoce el esquema y el rechazo por tipo puede contestarse antes de encolar nada.

## Limitaciones y follow-ups

- **Progreso incremental (D3, limitación declarada).** El motor publica los cuatro contadores a `0` porque `Repository::restore` de `rustic_core` 0.13 no ofrece callback de progreso por su API pública; el panel pinta solo la **fase**. Falta hilar desde la biblioteca los ficheros y bytes —o desde un `RestoreOptions`/API que aún no existe— para que la barra sea determinada. Los campos ya están en el contrato esperando ese día.
- **Fase 2 — targets de base de datos** (issue [#67](https://github.com/atareao/kesto/issues/67)): restaurar dentro del contenedor del target, con el reverso del `exec` del volcado. Requiere decidir cómo se inyecta el volcado sin pisar datos vivos. Hoy se rechaza con `422` y el panel deshabilita la fila.
- **Restauración parcial por rutas** (issue [#67](https://github.com/atareao/kesto/issues/67)): restaurar un subárbol o fichero suelto, hoy fuera de alcance.
- **Purga de snapshots**: no hay endpoint de borrado; el panel no ofrece botón a propósito.
- **Cobertura pendiente del escenario `delete: false`**: no hay prueba que afirme que los ficheros presentes en el destino y ausentes del snapshot **sobreviven** a la restauración; es el escenario conservador que justifica la elección de opciones.

## Panel (UI)

| Fichero | Qué aporta |
|---|---|
| `ui/src/pages/snapshots.tsx` | Acción «Restaurar» por fila con `Popconfirm` que nombra el target; deshabilita y explica cuando `target_type` es `null` o distinto de `directory`; aviso de fase por SSE y cierre en `terminada`/`fallida`. |
| `ui/src/pages/snapshots.test.tsx` | Cobertura de confirmación, POST con `repository_id`, fase en curso, cierre terminal, fallo con `motivo`, stream caído y filas no restaurables. |
| `ui/src/services/useEventStream.ts` | `useRestauracion(executionId)`: se suscribe desde el montaje (antes del POST) y lee el mapa de restauraciones. |
| `ui/src/services/events.tsx` | Parsea `restore_progress` (siete campos, `motivo` opcional) y acota el mapa de restauraciones. |
| `ui/src/services/api/types.ts` | `Snapshot` con `target_id`/`target_name`/`target_type` anulables; `EventoDeRestauracion` y `Restauracion`. |
| `ui/src/api/snapshots.ts` · `ui/src/hooks/useSnapshots.ts` | `POST /api/v1/snapshots/{id}/restore` y mutación `useRestaurarSnapshot`. |

Decisiones del panel:

- **Sin porcentaje ni barra determinada.** Los contadores del evento van a `0` en esta versión porque `rustic_core` 0.13 no ofrece callback de progreso; el panel solo refleja la **fase** (`restaurando`, `terminada`, `fallida`) y no inventa un avance.
- **El éxito no se anuncia al aceptar el `POST`.** La respuesta solo confirma que el trabajo se lanzó; el desenlace se comunica al cerrar la fase por SSE. Así el operador no ve «restaurado» y «restaurando» a la vez.
- **La limitación se declara en la fila**, no después del `409`/`422`: si el target no se conoce o no es de directorio, el botón queda deshabilitado con su explicación.
