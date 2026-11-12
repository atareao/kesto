/**
 * Andamiaje de las pruebas del stream SSE —el equivalente de
 * `src/test_support.rs` en el backend—: un `EventSource` de mentira con el que
 * se controla a mano qué emite y cuántas conexiones quedan vivas.
 *
 * Vive en `src/` porque es código de apoyo de pruebas, no de la aplicación: no
 * lo importa nada del panel, así que no entra en el bundle.
 */

import { act } from '@testing-library/react'
import type { EventoDeEjecucion } from './api/types'

/** Un `EventSource` que no habla con nadie y al que se le emiten eventos a mano. */
export class EventSourceFalso {
  static instancias: EventSourceFalso[] = []

  readonly url: string
  cerrado = false
  private readonly oyentes = new Map<string, Set<(evento: { data: string }) => void>>()

  constructor(url: string) {
    this.url = url
    EventSourceFalso.instancias.push(this)
  }

  addEventListener(tipo: string, oyente: (evento: { data: string }) => void): void {
    const delTipo = this.oyentes.get(tipo) ?? new Set()
    delTipo.add(oyente)
    this.oyentes.set(tipo, delTipo)
  }

  removeEventListener(tipo: string, oyente: (evento: { data: string }) => void): void {
    this.oyentes.get(tipo)?.delete(oyente)
  }

  close(): void {
    this.cerrado = true
  }

  /** Emite un evento del tipo pedido con el JSON del payload (o sin payload). */
  emitir(tipo: string, datos?: unknown): void {
    act(() => {
      for (const oyente of this.oyentes.get(tipo) ?? []) {
        oyente({ data: datos === undefined ? '' : JSON.stringify(datos) })
      }
    })
  }

  /** La última conexión abierta, que es sobre la que se suele emitir. */
  static ultima(): EventSourceFalso {
    const ultima = EventSourceFalso.instancias.at(-1)
    if (ultima === undefined) throw new Error('no se ha abierto ninguna conexión')
    return ultima
  }
}

/** Deja el contador a cero y sustituye el `EventSource` del navegador. */
export function instalarEventSourceFalso(): void {
  EventSourceFalso.instancias = []
  vi.stubGlobal('EventSource', EventSourceFalso)
}

/** Cuántas conexiones se han abierto en la prueba. */
export function conexiones(): readonly EventSourceFalso[] {
  return EventSourceFalso.instancias
}

/** El candado: ninguna conexión puede quedar abierta al acabar la prueba. */
export function exigirTodoCerrado(): void {
  for (const instancia of EventSourceFalso.instancias) {
    expect(instancia.cerrado).toBe(true)
  }
}

/** Un evento `execution_progress` completo, con los siete campos del contrato. */
export function eventoDePrueba(
  campos: Partial<EventoDeEjecucion> = {},
): EventoDeEjecucion {
  return {
    execution_id: 'e1',
    job_id: 'j1',
    status: 'running',
    bytes_processed: 2048,
    bytes_uploaded: 1024,
    percentage: 50,
    current_action: 'subiendo',
    ...campos,
  }
}
