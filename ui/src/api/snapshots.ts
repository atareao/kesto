import { peticion } from '../services/api/client'
import type { ListadoDeSnapshots } from '../services/api/types'

/**
 * `GET /api/v1/snapshots?repository_id=<uuid>`.
 *
 * `truncated` es `true` cuando el repositorio tenía más instantáneas de las que
 * devolvió la respuesta: quien sume `bytes` tiene que presentar la cifra como
 * mínimo, no como total.
 */
export function listarSnapshots(
  repositoryId: string,
  signal?: AbortSignal,
): Promise<ListadoDeSnapshots> {
  return peticion<ListadoDeSnapshots>('/snapshots', {
    consulta: { repository_id: repositoryId },
    signal,
  })
}
