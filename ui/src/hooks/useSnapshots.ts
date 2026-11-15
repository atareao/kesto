import { useMutation, useQuery } from '@tanstack/react-query'
import { listarSnapshots, restaurarSnapshot } from '../api/snapshots'
import { claves } from './claves'

/**
 * Instantáneas de un repositorio. Se pide solo cuando hay `repositoryId`: una
 * pantalla que todavía no lo conoce no dispara una petición vacía.
 */
export function useSnapshots(repositoryId: string | null | undefined) {
  return useQuery({
    queryKey: claves.snapshots(repositoryId ?? ''),
    queryFn: ({ signal }) => {
      // `enabled` garantiza que la consulta no se lanza sin repositorio; el
      // guardia deja el estrechamiento de tipo en manos del compilador en vez
      // de un `as string`.
      if (repositoryId === null || repositoryId === undefined || repositoryId === '') {
        return Promise.reject(new Error('no hay repositorio seleccionado'))
      }
      return listarSnapshots(repositoryId, signal)
    },
    enabled: repositoryId !== null && repositoryId !== undefined && repositoryId !== '',
  })
}

/** La instantánea que se quiere restaurar y el repositorio en el que vive. */
export interface RestauracionPedida {
  repositoryId: string
  snapshotId: string
  /** Permite cancelar la petición si la pantalla se desmonta. */
  signal?: AbortSignal
}

/**
 * Lanza la restauración de un snapshot sobre su target de origen.
 *
 * No invalida el listado: restaurar no cambia qué instantáneas hay en el
 * repositorio. El avance llega por el stream SSE (`restore_progress`) y no por
 * una nueva lectura, que es justo lo que el panel evita («sin recargar»). El
 * error se propaga para que la pantalla enseñe el `message` de la API.
 */
export function useRestaurarSnapshot() {
  return useMutation({
    mutationFn: ({ repositoryId, snapshotId, signal }: RestauracionPedida) =>
      restaurarSnapshot({ repositoryId, snapshotId, signal }),
  })
}
