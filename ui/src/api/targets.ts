import { peticion } from '../services/api/client'
import type { AltaTarget, Target } from '../services/api/types'

/** `GET /api/v1/targets`: todos los orígenes de copia, sin secretos. */
export function listarTargets(signal?: AbortSignal): Promise<Target[]> {
  return peticion<Target[]>('/targets', { signal })
}

/** `POST /api/v1/targets` → `201` con el target creado. */
export function crearTarget(cuerpo: AltaTarget): Promise<Target> {
  return peticion<Target>('/targets', { method: 'POST', cuerpo })
}

/**
 * `PUT /api/v1/targets/:id` → `200` con el target actualizado. Un
 * `db_password` ausente conserva la contraseña guardada: por eso el formulario
 * omite el campo vacío en vez de mandar `""`.
 */
export function actualizarTarget(id: string, cuerpo: AltaTarget): Promise<Target> {
  return peticion<Target>(`/targets/${id}`, { method: 'PUT', cuerpo })
}

/** `DELETE /api/v1/targets/:id` → **`200` con `data: null`** (la API no usa `204`). */
export function borrarTarget(id: string): Promise<null> {
  return peticion<null>(`/targets/${id}`, { method: 'DELETE' })
}
