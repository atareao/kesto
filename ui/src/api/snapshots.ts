import { peticion } from '../services/api/client'
import type { ListadoDeSnapshots, Restauracion } from '../services/api/types'

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

/** Lo que necesita el `POST` de restauración: el snapshot y su repositorio. */
export interface PeticionDeRestauracion {
  repositoryId: string
  snapshotId: string
  /** Permite cancelar la petición si la pantalla se desmonta. */
  signal?: AbortSignal
}

/**
 * `POST /api/v1/snapshots/{id}/restore` — restaura el snapshot sobre su target
 * de origen y devuelve la ejecución de restauración (**200**).
 *
 * El `repository_id` va en el **cuerpo** (el `id` ya está en la ruta): es el
 * repositorio en el que vive la instantánea, y sin él la API no sabe dónde
 * buscarla. Un `409` significa que el snapshot no casa con ninguna ejecución
 * —no hay target de origen— y un `422` que su target no es restaurable en esta
 * versión; los dos llegan con su `message`, que es lo que se enseña.
 *
 * La restauración sigue en segundo plano: el avance llega por SSE
 * (`restore_progress`), no por esta respuesta.
 */
export function restaurarSnapshot({
  repositoryId,
  snapshotId,
  signal,
}: PeticionDeRestauracion): Promise<Restauracion> {
  return peticion<Restauracion>(`/snapshots/${snapshotId}/restore`, {
    method: 'POST',
    cuerpo: { repository_id: repositoryId },
    signal,
  })
}
