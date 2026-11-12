import { peticion } from '../services/api/client'
import type { AltaRepository, Repository } from '../services/api/types'

/** `GET /api/v1/repositories`: todos los repositorios, sin secretos. */
export function listarRepositorios(signal?: AbortSignal): Promise<Repository[]> {
  return peticion<Repository[]>('/repositories', { signal })
}

/** `POST /api/v1/repositories` → `201` con el repositorio creado. */
export function crearRepositorio(cuerpo: AltaRepository): Promise<Repository> {
  return peticion<Repository>('/repositories', { method: 'POST', cuerpo })
}

/**
 * `PUT /api/v1/repositories/:id` → `200` con el repositorio actualizado. Un
 * `secret_access_key` o `passphrase` ausentes conservan el valor guardado.
 */
export function actualizarRepositorio(id: string, cuerpo: AltaRepository): Promise<Repository> {
  return peticion<Repository>(`/repositories/${id}`, { method: 'PUT', cuerpo })
}

/** `DELETE /api/v1/repositories/:id` → **`200` con `data: null`**. */
export function borrarRepositorio(id: string): Promise<null> {
  return peticion<null>(`/repositories/${id}`, { method: 'DELETE' })
}
