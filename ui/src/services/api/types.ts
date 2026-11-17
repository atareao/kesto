/**
 * DTO del **cable**: el espejo exacto del JSON que publica la API de Kesto.
 *
 * Son tipos de lectura y de escritura, y su pareja es `src/api/*.rs` del
 * backend. Los nombres van en `snake_case` porque son los del JSON: no se
 * traducen a `camelCase` en la frontera, o cada capa tendría que recordar dos
 * nombres para el mismo campo.
 *
 * Regla de secretos: un DTO de **lectura** nunca trae el valor de un secreto.
 * Trae un booleano `has_*` que dice si está puesto. Los secretos
 * (`db_password`, `secret_access_key`, `passphrase`) solo existen en los DTO de
 * **escritura**, que es el único camino por el que un valor así viaja, y nunca
 * se guardan en el navegador.
 */

/** Modo de autenticación del backend, tal cual lo publica `/health`. */
export type ModoDeAutenticacion = 'disabled' | 'oidc'

/** Clase de origen de una copia. Es el `CHECK` de `targets.target_type` (§3). */
export type TipoDeTarget = 'directory' | 'mariadb' | 'postgresql' | 'sqlite'

/** Estado de una ejecución. Es el `CHECK` de `executions.status` (§3). */
export type Estado = 'pending' | 'running' | 'success' | 'failed'

// ─── Targets ─────────────────────────────────────────────────────────────────

/** Lectura: `GET /api/v1/targets` (lista) y `GET /api/v1/targets/{id}`. */
export interface Target {
  id: string
  name: string
  target_type: TipoDeTarget
  container_name: string | null
  db_user: string | null
  /** El secreto solo viaja como booleano: nunca `db_password` en una lectura. */
  has_db_password: boolean
  db_name: string | null
  host_path: string | null
  /**
   * Ruta absoluta del `.db` **dentro** del contenedor; solo tiene valor en un
   * `sqlite`. La clave **siempre** viaja en la lectura (el backend no la omite
   * cuando va a `null`), así que aquí es obligatoria. No es un secreto: viaja en
   * claro en las dos direcciones.
   */
  db_path: string | null
  /** RFC3339, en UTC. */
  created_at: string
}

/**
 * Escritura: `POST` y `PUT` de `/api/v1/targets`.
 *
 * Es el único DTO de target que lleva `db_password`, y es opcional: en un `PUT`
 * sin él, el backend conserva la contraseña guardada.
 */
export interface AltaTarget {
  name: string
  target_type: TipoDeTarget
  container_name?: string | null
  db_user?: string | null
  db_password?: string | null
  db_name?: string | null
  host_path?: string | null
  /** Solo un `sqlite`: la ruta del `.db` dentro del contenedor. */
  db_path?: string | null
}

// ─── Repositorios ────────────────────────────────────────────────────────────

/**
 * Estado de salud de un repositorio. Es el `CHECK` de `repositories.health`
 * (§3) y el vocabulario que sirve la API: el panel **no** inventa ninguno más.
 *
 * Un repositorio que nunca se ha comprobado llega como «desconocido», nunca
 * como sano: el panel no puede deducir salud de algo que no se ha consultado.
 */
export type SaludDeRepositorio =
  | 'ok'
  | 'sin_repositorio'
  | 'passphrase'
  | 'inaccesible'
  | 'sin_permiso'
  | 'desconocido'

/**
 * Lectura: desenlace de `POST /api/v1/repositories/{id}/health` y del
 * equivalente global, que comprueba todos y devuelve un elemento por
 * repositorio.
 *
 * Son los tres campos de salud **ya persistidos**: la comprobación anota el
 * desenlace en la fila antes de responder, así que esto y lo que sirve el
 * listado después es lo mismo.
 */
export interface SaludComprobada {
  health: SaludDeRepositorio
  /** RFC3339, en UTC. */
  health_checked_at: string | null
  /** Motivo acotado y saneado de la dependencia; `null` si no hay nada que contar. */
  health_detail: string | null
}

/** Lectura: `GET /api/v1/repositories` y `GET /api/v1/repositories/{id}`. */
export interface Repository {
  id: string
  name: string
  endpoint: string
  bucket: string
  region: string
  access_key_id: string
  /** El secreto solo viaja como booleano: nunca `secret_access_key` en una lectura. */
  has_secret_access_key: boolean
  /** Igual que el anterior: la passphrase del repositorio solo se lee como booleano. */
  has_passphrase: boolean
  /**
   * Salud **persistida** de la fila, derivada del almacén. **No es escribible**:
   * el alta y la actualización la ignoran, y solo la mueve la comprobación. Un
   * repositorio nunca comprobado llega como «desconocido».
   */
  health: SaludDeRepositorio
  /** RFC3339, en UTC, de la última comprobación; `null` si nunca se comprobó. */
  health_checked_at: string | null
  /** Motivo acotado y saneado de la última comprobación; `null` si no hay. */
  health_detail: string | null
  /** RFC3339, en UTC. */
  created_at: string
}

/** Escritura: `POST` y `PUT` de `/api/v1/repositories`. */
export interface AltaRepository {
  name: string
  endpoint: string
  bucket: string
  region: string
  access_key_id: string
  secret_access_key?: string | null
  passphrase?: string | null
}

// ─── Tareas programadas ──────────────────────────────────────────────────────

/** Lectura: `GET /api/v1/jobs` y `GET /api/v1/jobs/{id}`. */
export interface Job {
  id: string
  /**
   * Nombre con el que el operador distingue la tarea. Es **obligatorio y único**
   * (#87): la columna es `NOT NULL` con `UNIQUE(name)`, así que la clave viaja
   * **siempre** como cadena no vacía y nunca `null`. Es la identidad legible de
   * la tarea —la misma cadena en Tareas, en el Histórico y en Snapshots—.
   */
  name: string
  target_id: string
  repository_id: string
  cron_expression: string
  keep_daily: number
  keep_weekly: number
  keep_monthly: number
  enabled: boolean
  /** RFC3339, en UTC. */
  created_at: string
}

/** Escritura: `POST` y `PUT` de `/api/v1/jobs`. */
export interface AltaJob {
  /**
   * Nombre de la tarea; **obligatorio**. El backend lo recorta, contesta `400`
   * —nombrando `name`— si falta o queda en blanco tras recortar, y `409` si ya
   * lo tiene otra tarea. El panel lo manda **siempre**, ya recortado.
   */
  name: string
  target_id: string
  repository_id: string
  cron_expression: string
  keep_daily: number
  keep_weekly: number
  keep_monthly: number
  enabled: boolean
}

/**
 * Respuesta de `POST /api/v1/jobs/{id}/run` (**202**): la ejecución ya aceptada,
 * todavía en `pending`. Un `409` significa que esa tarea ya tiene una en curso.
 */
export interface EjecucionEncolada {
  id: string
  job_id: string
  status: string
}

// ─── Ejecuciones ─────────────────────────────────────────────────────────────

/**
 * Lectura: `GET /api/v1/executions?job_id=&status=&limit=&offset=`.
 *
 * `limit` va de 1 a 200 (50 por defecto) y `offset` no puede ser negativo: fuera
 * de rango, el backend contesta `400` **nombrando el campo**.
 */
export interface Execution {
  id: string
  job_id: string
  /**
   * Nombre de la tarea que originó la ejecución, resuelto por el backend en el
   * mismo vínculo que el resto de la fila; `null` cuando esa tarea no tiene
   * nombre. La clave **siempre** viaja, así que la columna «Tarea» cae al
   * identificador cuando llega a `null` —nunca al nombre del target—.
   */
  job_name: string | null
  status: Estado
  snapshot_id: string | null
  bytes_processed: number
  bytes_uploaded: number
  error_message: string | null
  /** RFC3339, en UTC. */
  started_at: string
  /** RFC3339, en UTC, o `null` si todavía no ha terminado. */
  finished_at: string | null
}

// ─── Snapshots ───────────────────────────────────────────────────────────────

/** Un snapshot del repositorio. `id` es el hexadecimal completo de rustic. */
export interface Snapshot {
  id: string
  /** RFC3339, en UTC. */
  time: string
  paths: string[]
  label: string | null
  /**
   * Nombre de la tarea de la que procede la instantánea, por el vínculo
   * `snapshot → job_executions → backup_jobs`. Toda tarea tiene nombre, así que
   * el único `null` que queda es la instantánea **huérfana** —la que no casa con
   * ninguna ejecución—: la columna «Tarea» pinta entonces un marcador atenuado y
   * **nunca** cae al identificador de la tarea que publica `label`.
   */
  job_name: string | null
  files: number
  bytes: number
  /**
   * El target **del que procede** la instantánea, resuelto por el backend con
   * el mismo vínculo que usa la restauración —`snapshot → job_executions →
   * backup_jobs → backup_targets`— y acotado al repositorio que se pide.
   *
   * Los tres son `null` cuando la instantánea no casa con ninguna ejecución
   * —una copia hecha por otra herramienta, o cuya fila se borró en cascada—, y
   * `null` es un valor del contrato: significa «no se conoce el target», no un
   * error del listado. Es lo que decide si la fila puede ofrecer restaurar y,
   * cuando la ofrece, sobre qué se va a restaurar.
   */
  target_id: string | null
  /** Nombre legible del target de origen; `null` junto a los otros dos. */
  target_name: string | null
  /** Tipo del target de origen, en el vocabulario del cable; `null` igual. */
  target_type: TipoDeTarget | null
}

/** `GET /api/v1/snapshots?repository_id=<uuid>` cuando no se pide un snapshot. */
export interface ListadoDeSnapshots {
  snapshots: Snapshot[]
  /** `true` si el repositorio tenía más snapshots de los que se devolvieron. */
  truncated: boolean
}

/**
 * Respuesta de `POST /api/v1/snapshots/{id}/restore` (**200**): la ejecución de
 * restauración ya aceptada y el target de origen sobre el que escribe.
 *
 * `target_name` es el nombre legible con el que el backend **resolvió** el
 * vínculo snapshot → ejecución → tarea → target, y es la confirmación de lo que
 * se está restaurando: el listado ya publica el de cada fila, así que la
 * respuesta sirve para nombrar el destino con la fuente que manda. El resultado
 * de la restauración no viene aquí —esto solo acepta el trabajo—, sino por SSE
 * (`restore_progress`), y por eso el panel **no** anuncia nada al recibirla.
 */
export interface Restauracion {
  execution_id: string
  snapshot_id: string
  target_id: string
  target_name: string
}

// ─── Histórico de restauraciones ─────────────────────────────────────────────

/**
 * Lectura: una fila de `GET /api/v1/restorations`.
 *
 * Es la ejecución de restauración **persistida** —la tabla
 * `restore_executions`—, no la respuesta del `POST` (`Restauracion`): aquí llega
 * el desenlace real, con su `status` y sus fechas, porque el histórico se lee
 * cuando la operación ya ha ocurrido. Por eso el estado es de fiar y no un
 * «lanzada» optimista.
 *
 * `target_name` y `target_type` los resuelve el backend desde el target
 * referenciado por `target_id`, y son `null` solo si `target_id` es `null`
 * —cuando el target se borró y la FK lo puso a nulo—. `error_message` solo
 * lleva texto en una `failed`; en el resto viene a `null`.
 */
export interface RestauracionRegistrada {
  id: string
  snapshot_id: string
  target_id: string | null
  target_name: string | null
  target_type: TipoDeTarget | null
  status: Estado
  error_message: string | null
  /** RFC3339, en UTC. */
  started_at: string
  /** RFC3339, en UTC, o `null` si todavía no ha terminado. */
  finished_at: string | null
}

/**
 * `GET /api/v1/restorations?status=&target_id=&limit=&offset=` (**200**).
 *
 * Trae la página y el **total** de filas que cumplen los filtros: con el total,
 * el panel sabe si hay página siguiente sin pedir una de más.
 */
export interface ListadoDeRestauraciones {
  restorations: RestauracionRegistrada[]
  total: number
}

// ─── Salud y sesión ──────────────────────────────────────────────────────────

/** `GET /api/v1/health`. `auth_mode` decide si el panel tiene capa de sesión. */
export interface Health {
  version: string
  auth_mode: ModoDeAutenticacion
}

/**
 * `GET /api/v1/auth/session` (200). En modo `disabled` esa ruta **no existe**
 * —contesta `404`, no `401`—, así que el panel no puede llamarla.
 */
export interface Sesion {
  sub: string
  email: string
}

// ─── Stream de eventos ───────────────────────────────────────────────────────

/**
 * Payload de `GET /api/v1/events`, evento `execution_progress`.
 *
 * Son **siete** campos y el nombre del evento es un literal de protocolo
 * (`src/api/events.rs`: `NOMBRE_DEL_EVENTO`). Añadir, quitar o renombrar
 * cualquiera de los dos lados rompe el panel.
 */
export interface EventoDeEjecucion {
  execution_id: string
  job_id: string
  status: Estado
  bytes_processed: number
  bytes_uploaded: number
  /** `null` cuando no se conoce el total: la interfaz pinta indeterminado. */
  percentage: number | null
  current_action: string
}

/**
 * Payload de `GET /api/v1/events`, evento `restore_progress`.
 *
 * Son **siete** campos y el nombre del evento es un literal de protocolo
 * («restore_progress»). Lo que describe es el **empezó/terminó** y la fase, no
 * un avance incremental: en esta versión los cuatro contadores van a `0` porque
 * `restore` de `rustic_core` 0.13 no ofrece progreso por su API pública. El
 * panel **no** los interpreta como una cifra —ni barra ni porcentaje—, así que
 * los dos totales se declaran anulables por prudencia y no para pintar nada con
 * ellos.
 *
 * `phase` es la fase del protocolo: `restaurando` mientras trabaja, `terminada`
 * al acabar y `fallida` si no pudo completarse. Un `fallida` puede traer
 * `motivo` con la explicación; el resto de fases no lo llevan.
 */
export interface EventoDeRestauracion {
  execution_id: string
  snapshot_id: string
  phase: string
  files_done: number
  files_total: number | null
  bytes_done: number
  bytes_total: number | null
  /** Explicación de una restauración `fallida`; opcional y ausente en el resto. */
  motivo?: string | null
}
