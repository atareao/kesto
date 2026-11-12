import { peticion } from '../services/api/client'
import type { Target } from '../services/api/types'

/** `GET /api/v1/targets`: todos los orígenes de copia, sin secretos. */
export function listarTargets(signal?: AbortSignal): Promise<Target[]> {
  return peticion<Target[]>('/targets', { signal })
}
