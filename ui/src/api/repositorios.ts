import { peticion } from '../services/api/client'
import type { Repository } from '../services/api/types'

/** `GET /api/v1/repositories`: todos los repositorios, sin secretos. */
export function listarRepositorios(signal?: AbortSignal): Promise<Repository[]> {
  return peticion<Repository[]>('/repositories', { signal })
}
