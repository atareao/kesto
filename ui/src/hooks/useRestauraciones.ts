import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { listarRestauraciones } from '../api/restauraciones'
import type { FiltrosDeRestauraciones } from '../api/restauraciones'
import type { ListadoDeRestauraciones } from '../services/api/types'
import { claves } from './claves'

/** Cada cuánto se vuelve a pedir el listado mientras queden filas sin cerrar. */
export const INTERVALO_DE_REFRESCO_MS = 5_000

/**
 * ¿Queda alguna fila sin cerrar? `pending` todavía no ha arrancado y `running`
 * está en curso: las dos cambian solas en el servidor, así que el listado tiene
 * que refrescarse por su cuenta hasta que lleguen a un desenlace.
 */
export function hayRestauracionesSinCerrar(listado: ListadoDeRestauraciones | undefined): boolean {
  return (listado?.restorations ?? []).some(
    (fila) => fila.status === 'pending' || fila.status === 'running',
  )
}

/**
 * El histórico de restauraciones, opcionalmente filtrado. `filtros` forma parte
 * de la clave: dos pantallas con filtros distintos no comparten la entrada.
 *
 * Mientras haya filas `pending` o `running` el listado se vuelve a pedir cada
 * `INTERVALO_DE_REFRESCO_MS` —sin recargar la página—; cuando todas cierran, el
 * refresco automático se apaga. El desenlace se lee del **servidor** y no de un
 * «lanzada» optimista: es el estado real el que se pinta.
 */
export function useRestauraciones(filtros: FiltrosDeRestauraciones = {}) {
  return useQuery({
    queryKey: claves.restauraciones(filtros),
    queryFn: ({ signal }) => listarRestauraciones(filtros, signal),
    placeholderData: keepPreviousData,
    refetchInterval: (query) =>
      hayRestauracionesSinCerrar(query.state.data) ? INTERVALO_DE_REFRESCO_MS : false,
  })
}
