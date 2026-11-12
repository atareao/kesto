/**
 * El proveedor del stream SSE: **una sola** conexión a `GET /api/v1/events` para
 * toda la aplicación, con un `EventSource` que se reconecta con espera creciente.
 *
 * Cómo se evita abrir una conexión por pantalla: el proveedor lleva la cuenta de
 * consumidores. El primer `useEventStream()` abre el `EventSource` y el último en
 * desmontarse lo cierra. Reconectar es cosa nuestra —`EventSource` cerraría y
 * reabriría solo, sin espera—, así que ante un `error` se cierra la fuente y se
 * reintenta con un retardo que se dobla en cada intento hasta un tope.
 *
 * Los consumidores —el contexto y los hooks— viven en `useEventStream.ts`: este
 * fichero exporta un componente y solo un componente, que es lo que exige
 * `react-refresh/only-export-components`.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import type { Estado, EventoDeEjecucion } from './api/types'
import {
  ContextoDelStream,
  ESPERA_BASE_MS,
  ESPERA_MAXIMA_MS,
  LIMITE_DE_LINEAS,
  NOMBRE_DEL_EVENTO,
  RUTA_DE_EVENTOS,
} from './useEventStream'
import type { EstadoDeLaConexion, ValorDelStream } from './useEventStream'

const ESTADOS: readonly Estado[] = ['pending', 'running', 'success', 'failed']

function esEstado(valor: unknown): valor is Estado {
  return typeof valor === 'string' && (ESTADOS as readonly string[]).includes(valor)
}

/**
 * Valida el JSON del evento y lo convierte en el DTO. Un evento al que le falten
 * campos se descarta: es preferible a pintar un progreso con campos a
 * `undefined`, que romperían la vista de esa ejecución.
 */
function interpretarEvento(datos: string): EventoDeEjecucion | null {
  let crudo: unknown
  try {
    crudo = JSON.parse(datos) as unknown
  } catch {
    return null
  }

  if (typeof crudo !== 'object' || crudo === null) return null
  const campos = crudo as Record<string, unknown>

  if (typeof campos.execution_id !== 'string' || typeof campos.job_id !== 'string') return null
  if (!esEstado(campos.status)) return null
  if (typeof campos.bytes_processed !== 'number' || typeof campos.bytes_uploaded !== 'number') return null
  if (campos.percentage !== null && typeof campos.percentage !== 'number') return null
  if (typeof campos.current_action !== 'string') return null

  return {
    execution_id: campos.execution_id,
    job_id: campos.job_id,
    status: campos.status,
    bytes_processed: campos.bytes_processed,
    bytes_uploaded: campos.bytes_uploaded,
    percentage: campos.percentage,
    current_action: campos.current_action,
  }
}

/**
 * Abre (y cierra) la conexión SSE según haya consumidores. Va por encima de las
 * pantallas que muestren progreso.
 */
export function ProveedorDeEventos({
  children,
  url = RUTA_DE_EVENTOS,
}: {
  children: ReactNode
  /** Solo para pruebas: la ruta del stream. */
  url?: string
}) {
  const [estado, setEstado] = useState<EstadoDeLaConexion>('detenido')
  const [ejecuciones, setEjecuciones] = useState<ReadonlyMap<string, EventoDeEjecucion>>(
    () => new Map(),
  )
  const [historial, setHistorial] = useState<
    ReadonlyMap<string, readonly EventoDeEjecucion[]>
  >(() => new Map())
  const [hayConsumidores, setHayConsumidores] = useState(false)
  const consumidores = useRef(0)

  const suscribir = useCallback(() => {
    consumidores.current += 1
    if (consumidores.current === 1) setHayConsumidores(true)
  }, [])

  const desuscribir = useCallback(() => {
    consumidores.current = Math.max(0, consumidores.current - 1)
    if (consumidores.current === 0) {
      setHayConsumidores(false)
      setEstado('detenido')
    }
  }, [])

  useEffect(() => {
    if (!hayConsumidores) return

    let intento = 0
    let fuente: EventSource | null = null
    let espera: ReturnType<typeof setTimeout> | null = null
    let desmontado = false

    const abrir = () => {
      if (desmontado) return

      setEstado(intento === 0 ? 'conectando' : 'reconectando')
      // Ruta relativa: la resuelve el navegador contra el documento, que es lo
      // que hace también el proxy de Vite en desarrollo.
      fuente = new EventSource(url)

      fuente.addEventListener('open', () => {
        intento = 0
        setEstado('abierto')
      })

      fuente.addEventListener(NOMBRE_DEL_EVENTO, (evento) => {
        const progreso = interpretarEvento((evento as MessageEvent<string>).data)
        if (progreso === null) return

        setEjecuciones((previas) => {
          const siguientes = new Map(previas)
          siguientes.set(progreso.execution_id, progreso)
          return siguientes
        })

        setHistorial((previo) => {
          const anteriores = previo.get(progreso.execution_id) ?? []
          const siguientes = [...anteriores, progreso].slice(-LIMITE_DE_LINEAS)
          const mapa = new Map(previo)
          mapa.set(progreso.execution_id, siguientes)
          return mapa
        })
      })

      fuente.addEventListener('error', () => {
        fuente?.close()
        if (desmontado) return

        const retardo = Math.min(ESPERA_BASE_MS * 2 ** intento, ESPERA_MAXIMA_MS)
        intento += 1
        setEstado('reconectando')
        espera = setTimeout(abrir, retardo)
      })
    }

    abrir()

    return () => {
      desmontado = true
      if (espera !== null) clearTimeout(espera)
      fuente?.close()
    }
  }, [hayConsumidores, url])

  const valor = useMemo<ValorDelStream>(
    () => ({ estado, ejecuciones, historial, suscribir, desuscribir }),
    [estado, ejecuciones, historial, suscribir, desuscribir],
  )

  return <ContextoDelStream.Provider value={valor}>{children}</ContextoDelStream.Provider>
}
