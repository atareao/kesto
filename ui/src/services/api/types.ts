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
  files: number
  bytes: number
}

/** `GET /api/v1/snapshots?repository_id=<uuid>` cuando no se pide un snapshot. */
export interface ListadoDeSnapshots {
  snapshots: Snapshot[]
  /** `true` si el repositorio tenía más snapshots de los que se devolvieron. */
  truncated: boolean
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
