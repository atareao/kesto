import { peticion } from '../services/api/client'
import type { Estado, Execution } from '../services/api/types'

/**
 * Filtros de `GET /api/v1/executions`. `limit` va de 1 a 200 (50 por defecto) y
 * `offset` no puede ser negativo: fuera de rango el backend contesta `400`.
 */
export interface FiltrosDeEjecuciones {
  job_id?: string
  status?: Estado
  limit?: number
  offset?: number
}

/** `GET /api/v1/executions`, en orden de más reciente a más antigua. */
export function listarEjecuciones(
  filtros: FiltrosDeEjecuciones = {},
  signal?: AbortSignal,
): Promise<Execution[]> {
  return peticion<Execution[]>('/executions', {
    consulta: {
      job_id: filtros.job_id,
      status: filtros.status,
      limit: filtros.limit,
      offset: filtros.offset,
    },
    signal,
  })
}
