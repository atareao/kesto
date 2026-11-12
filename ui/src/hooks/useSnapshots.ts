import { useQuery } from '@tanstack/react-query'
import { listarSnapshots } from '../api/snapshots'
import { claves } from './claves'

/**
 * Instantáneas de un repositorio. Se pide solo cuando hay `repositoryId`: una
 * pantalla que todavía no lo conoce no dispara una petición vacía.
 */
export function useSnapshots(repositoryId: string | null | undefined) {
  return useQuery({
    queryKey: claves.snapshots(repositoryId ?? ''),
    queryFn: ({ signal }) => listarSnapshots(repositoryId as string, signal),
    enabled: repositoryId !== null && repositoryId !== undefined && repositoryId !== '',
  })
}
