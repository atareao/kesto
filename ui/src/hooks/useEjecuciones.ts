import { useQuery } from '@tanstack/react-query'
import { listarEjecuciones } from '../api/ejecuciones'
import type { FiltrosDeEjecuciones } from '../api/ejecuciones'
import { claves } from './claves'

/**
 * Ejecuciones, opcionalmente filtradas y paginadas. `filtros` —con su `limit` y
 * su `offset`— forma parte de la clave: dos páginas o dos filtros distintos no
 * comparten la misma entrada de caché. El resultado trae la página en
 * `data.executions` y el total en `data.total`.
 */
export function useEjecuciones(filtros: FiltrosDeEjecuciones = {}) {
  return useQuery({
    queryKey: claves.ejecuciones(filtros),
    queryFn: ({ signal }) => listarEjecuciones(filtros, signal),
  })
}
