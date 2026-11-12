import { useQueries } from '@tanstack/react-query'
import { listarSnapshots } from '../api/snapshots'
import type { Repository } from '../services/api/types'
import { claves } from './claves'

/** Lo que se puede afirmar del espacio ocupado en S3. */
export interface EspacioEnS3 {
  /** Suma de los `bytes` de las instantáneas devueltas. */
  bytes: number
  /**
   * `true` si algún repositorio devolvió `truncated`: la suma es un **mínimo**,
   * no el total.
   */
  truncado: boolean
  cargando: boolean
  /** El primer error de las consultas, si alguna falló. */
  error: unknown
}

/**
 * Suma el espacio de las instantáneas de **todos** los repositorios.
 *
 * Se pide una consulta por repositorio con `useQueries` —no un bucle de hooks—
 * y cada una se cachea con su propia clave, así que volver al Dashboard no
 * repite las peticiones que ya están frescas.
 */
export function useEspacioEnS3(repositorios: readonly Repository[]): EspacioEnS3 {
  const consultas = useQueries({
    queries: repositorios.map((repositorio) => ({
      queryKey: claves.snapshots(repositorio.id),
      queryFn: ({ signal }: { signal: AbortSignal }) => listarSnapshots(repositorio.id, signal),
    })),
  })

  let bytes = 0
  let truncado = false
  let cargando = false
  let error: unknown = null

  for (const consulta of consultas) {
    if (consulta.isPending) cargando = true
    if (consulta.error !== null) error = error ?? consulta.error
    if (consulta.data === undefined) continue

    truncado = truncado || consulta.data.truncated
    for (const snapshot of consulta.data.snapshots) bytes += snapshot.bytes
  }

  return { bytes, truncado, cargando, error }
}
