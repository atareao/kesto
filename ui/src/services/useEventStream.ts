/**
 * Los consumidores del stream SSE: el contexto, el nombre del evento y los
 * hooks. El proveedor que abre la conexión vive en `events.tsx`.
 *
 * Están separados a propósito: `eslint.config.js` activa
 * `react-refresh/only-export-components` —tal como lo dejó la Fase 1— y un
 * fichero que exporta un componente **y** hooks deja de ser refrescable. Aquí no
 * hay ningún componente, así que el aviso no aparece y no hay que tocar la
 * configuración.
 *
 * Contrato del evento (`src/api/events.rs`):
 *
 * - El nombre es literalmente `execution_progress`; renombrarlo en un lado es un
 *   cambio de protocolo, no una decisión de estilo.
 * - El `data` es un JSON con siete campos: `execution_id`, `job_id`, `status`,
 *   `bytes_processed`, `bytes_uploaded`, `percentage` y `current_action`.
 * - `execution_id` y `job_id` vienen con guiones, igual que en `/executions`, y
 *   se comparan tal cual: no se normalizan.
 * - `percentage` es `null` cuando no se conoce el total (barra indeterminada).
 */

import { createContext, useContext, useEffect } from 'react'
import type { EventoDeEjecucion } from './api/types'

/** Lo que se espera antes del primer reintento; cada uno dobla al anterior. */
export const ESPERA_BASE_MS = 1000

/** Tope de la espera entre reintentos: no crece sin fin. */
export const ESPERA_MAXIMA_MS = 30_000

/** Única ruta del stream de eventos. */
export const RUTA_DE_EVENTOS = '/api/v1/events'

/** Nombre del evento. Es un literal de protocolo, no una etiqueta cualquiera. */
export const NOMBRE_DEL_EVENTO = 'execution_progress'

/** Estado de la conexión, para pintar un aviso si el stream no está vivo. */
export type EstadoDeLaConexion = 'conectando' | 'abierto' | 'reconectando' | 'detenido'

/**
 * Líneas que guarda la consola por ejecución. Es un tope, no una promesa: una
 * copia larga emite muchos eventos y la consola no puede crecer sin fin en
 * memoria. Se conservan **las últimas**.
 */
export const LIMITE_DE_LINEAS = 200

/** Lo que ve un consumidor: la conexión y el último progreso de cada ejecución. */
export interface ContextoDeEventos {
  estado: EstadoDeLaConexion
  /** Último evento conocido de cada ejecución, indexado por `execution_id`. */
  ejecuciones: ReadonlyMap<string, EventoDeEjecucion>
  /**
   * Eventos recibidos de cada ejecución, indexado por `execution_id`, con un
   * máximo de `LIMITE_DE_LINEAS` y en orden de llegada. Alimenta la consola.
   */
  historial: ReadonlyMap<string, readonly EventoDeEjecucion[]>
}

/** El contexto y las dos operaciones con las que el proveedor cuenta consumidores. */
export interface ValorDelStream extends ContextoDeEventos {
  suscribir: () => void
  desuscribir: () => void
}

export const ContextoDelStream = createContext<ValorDelStream | null>(null)

/** El contexto y la suscripción del consumidor que llama. */
function useContextoDelStream(): ValorDelStream {
  const contexto = useContext(ContextoDelStream)
  if (contexto === null) {
    throw new Error('useEventStream necesita un <ProveedorDeEventos> por encima en el árbol')
  }

  const { suscribir, desuscribir } = contexto
  useEffect(() => {
    suscribir()
    return desuscribir
  }, [suscribir, desuscribir])

  return contexto
}

/**
 * El stream: estado de la conexión y el último progreso conocido de cada
 * ejecución, indexado por `execution_id`.
 */
export function useEventStream(): ContextoDeEventos {
  const { estado, ejecuciones, historial } = useContextoDelStream()
  return { estado, ejecuciones, historial }
}

/**
 * Las líneas de la consola de **una** ejecución, en orden de llegada y con el
 * tope de `LIMITE_DE_LINEAS`. Los eventos de otras ejecuciones no aparecen.
 */
export function useLineasDeEjecucion(executionId: string): {
  lineas: readonly EventoDeEjecucion[]
  estado: EstadoDeLaConexion
} {
  const { estado, historial } = useContextoDelStream()
  return { estado, lineas: historial.get(executionId) ?? [] }
}

/**
 * El progreso de **una** ejecución. Los eventos de otras ejecuciones no tocan lo
 * que devuelve este hook: se leen del mapa por su identificador.
 */
export function useEjecucion(executionId: string): {
  evento: EventoDeEjecucion | null
  estado: EstadoDeLaConexion
} {
  const { estado, ejecuciones } = useContextoDelStream()
  return { estado, evento: ejecuciones.get(executionId) ?? null }
}
