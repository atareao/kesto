import { peticion } from '../services/api/client'
import type { Estado, ListadoDeRestauraciones } from '../services/api/types'

/**
 * Filtros de `GET /api/v1/restorations`. `limit` va de 1 a 200 (50 por defecto) y
 * `offset` no puede ser negativo: fuera de rango el backend contesta `400`
 * nombrando el campo.
 */
export interface FiltrosDeRestauraciones {
  status?: Estado
  target_id?: string
  limit?: number
  offset?: number
}

/**
 * `GET /api/v1/restorations`, en orden de más reciente a más antigua.
 *
 * Devuelve la página **y** el total: el total es lo que le dice al panel si hay
 * página siguiente, sin tener que pedir una de más para descubrirlo.
 */
export function listarRestauraciones(
  filtros: FiltrosDeRestauraciones = {},
  signal?: AbortSignal,
): Promise<ListadoDeRestauraciones> {
  return peticion<ListadoDeRestauraciones>('/restorations', {
    consulta: {
      status: filtros.status,
      target_id: filtros.target_id,
      limit: filtros.limit,
      offset: filtros.offset,
    },
    signal,
  })
}
