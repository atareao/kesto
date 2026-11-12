import { useQuery } from '@tanstack/react-query'
import { listarRepositorios } from '../api/repositorios'
import { claves } from './claves'

/** Lista de repositorios. La clave es la raíz: una mutación invalida la lista entera. */
export function useRepositorios() {
  return useQuery({
    queryKey: claves.repositorios,
    queryFn: ({ signal }) => listarRepositorios(signal),
  })
}
