import { useMutation, useQuery } from '@tanstack/react-query'
import { listarSnapshots, restaurarSnapshot } from '../api/snapshots'
import { claves } from './claves'

/**
 * Tope de reintento de las consultas que **dependen del S3**. Acotado a
 * propósito: el `retry: 3` por defecto de React Query multiplica una espera que
 * ya es larga cuando el repositorio no responde, y la spec exige que estas
 * vistas alcancen un desenlace —datos, o error con reintentar— sin quedarse
 * «cargando» de forma indefinida.
 *
 * Vive aquí —junto a la consulta más elemental del S3— y la reutiliza
 * `useEspacioEnS3`: el acotado es de **cada consulta** que depende del S3, así
 * que la política de reintento **global** del cliente no se toca.
 *
 * Y no confundas reintentos con espera: el cliente HTTP **no** trae deadline
 * (`services/api/client.ts` no impone timeout ni el `AbortSignal` lo lleva), de
 * modo que `retry: false` solo acota las tentativas. Que un listado que nunca
 * resuelve deje de ser «cargando» depende del **backend**: rustic corta el
 * destino a los 10 s (`TOPE_DEL_DESTINO`) y ese corte rechaza el `fetch`. Esta
 * constante no sustituye esa garantía.
 */
export const REINTENTOS_DE_S3 = false

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
    retry: REINTENTOS_DE_S3,
  })
}

/** La instantánea que se quiere restaurar y el repositorio en el que vive. */
export interface RestauracionPedida {
  repositoryId: string
  snapshotId: string
  /**
   * Autoriza la poda del destino: la sobrescritura de los datos vivos de una
   * base, o retirar del destino lo que no está en la instantánea en un
   * `directory`. Es **obligatoria** y explícita: la decide la confirmación de la
   * pantalla, no el hook, y no se deduce del tipo de target.
   */
  sobrescribir: boolean
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
    mutationFn: ({ repositoryId, snapshotId, sobrescribir, signal }: RestauracionPedida) =>
      restaurarSnapshot({ repositoryId, snapshotId, sobrescribir, signal }),
  })
}
