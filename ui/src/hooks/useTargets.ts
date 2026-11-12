import { useQuery } from '@tanstack/react-query'
import { listarTargets } from '../api/targets'
import { claves } from './claves'

/** Lista de targets (orígenes de copia). */
export function useTargets() {
  return useQuery({
    queryKey: claves.targets,
    queryFn: ({ signal }) => listarTargets(signal),
  })
}
