import { useQuery } from '@tanstack/react-query'
import { listarEjecuciones } from '../api/ejecuciones'
import type { FiltrosDeEjecuciones } from '../api/ejecuciones'
import { claves } from './claves'

/**
 * Ejecuciones, opcionalmente filtradas. `filtros` forma parte de la clave: dos
 * pantallas con filtros distintos no comparten la misma entrada de caché.
 */
export function useEjecuciones(filtros: FiltrosDeEjecuciones = {}) {
  return useQuery({
    queryKey: claves.ejecuciones(filtros),
    queryFn: ({ signal }) => listarEjecuciones(filtros, signal),
  })
}
