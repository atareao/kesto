import { peticion } from '../services/api/client'
import type { AltaRepository, Repository, SaludComprobada } from '../services/api/types'

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

/**
 * `POST /api/v1/repositories/:id/health` → el desenlace de comprobar **ese**
 * repositorio. El backend lo persiste antes de responder, así que una lectura
 * posterior del recurso enseña lo mismo.
 */
export function comprobarSaludDeRepositorio(
  id: string,
  signal?: AbortSignal,
): Promise<SaludComprobada> {
  return peticion<SaludComprobada>(`/repositories/${id}/health`, { method: 'POST', signal })
}

/**
 * `POST /api/v1/repositories/health` → comprueba **todos** los repositorios y
 * devuelve un elemento por cada uno, ya persistido en su fila.
 *
 * Cuesta N × el tope del destino (§D3, 10 s): es una acción que pide el
 * operador a propósito, nunca algo que dispare el panel al cargar.
 */
export function comprobarSaludDeTodos(signal?: AbortSignal): Promise<SaludComprobada[]> {
  return peticion<SaludComprobada[]>('/repositories/health', { method: 'POST', signal })
}
