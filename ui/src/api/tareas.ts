import { peticion } from '../services/api/client'
import type { AltaJob, EjecucionEncolada, Job } from '../services/api/types'

/** `GET /api/v1/jobs`. */
export function listarTareas(signal?: AbortSignal): Promise<Job[]> {
  return peticion<Job[]>('/jobs', { signal })
}

/** `POST /api/v1/jobs` → `201` con la tarea creada. */
export function crearTarea(cuerpo: AltaJob): Promise<Job> {
  return peticion<Job>('/jobs', { method: 'POST', cuerpo })
}

/** `PUT /api/v1/jobs/:id` → `200` con la tarea actualizada. */
export function actualizarTarea(id: string, cuerpo: AltaJob): Promise<Job> {
  return peticion<Job>(`/jobs/${id}`, { method: 'PUT', cuerpo })
}

/**
 * `DELETE /api/v1/jobs/:id` → **`200` con `data: null`** (la API no usa `204`).
 */
export function borrarTarea(id: string): Promise<null> {
  return peticion<null>(`/jobs/${id}`, { method: 'DELETE' })
}

/**
 * `POST /api/v1/jobs/:id/run` → **`202`** con la ejecución aceptada, en
 * `pending`. Un `409` significa que la tarea ya tiene una en curso y un `404`
 * que la tarea ya no existe.
 */
export function dispararTarea(id: string): Promise<EjecucionEncolada> {
  return peticion<EjecucionEncolada>(`/jobs/${id}/run`, { method: 'POST' })
}
