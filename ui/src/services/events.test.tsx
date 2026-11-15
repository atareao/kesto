import { act, render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { conexiones, EventSourceFalso, eventoDePrueba, exigirTodoCerrado, instalarEventSourceFalso } from '../../tests/eventosDePrueba'
import { ProveedorDeEventos } from './events'
import {
  ESPERA_BASE_MS,
  LIMITE_DE_RESTAURACIONES,
  NOMBRE_DEL_EVENTO,
  NOMBRE_DEL_EVENTO_DE_RESTAURACION,
  useEjecucion,
  useEventStream,
} from './useEventStream'

// Ciclo de vida de la conexión: cuándo se abre, cuándo se reconecta y cuándo se
// cierra. La interpretación del payload se prueba en `useEventStream.test.tsx`.
//
// El contexto del evento, para leer las pruebas de abajo:
//   conexiones()        las conexiones abiertas en la prueba
//   EventSourceFalso.ultima()  la última abierta, sobre la que se emite

/** Consumidor mínimo: monta el hook y pinta el estado y el progreso de e1. */
function Sonda() {
  const { estado } = useEventStream()
  const { evento } = useEjecucion('e1')

  return (
    <div>
      <span data-testid="estado">{estado}</span>
      <span data-testid="progreso">
        {evento === null ? 'sin datos' : `${evento.current_action}|${String(evento.percentage)}`}
      </span>
    </div>
  )
}

function montar(ui: ReactNode) {
  return render(<ProveedorDeEventos>{ui}</ProveedorDeEventos>)
}

/** Cuenta las restauraciones que el proveedor recuerda, para ver su tope. */
function ContadorDeRestauraciones() {
  const { restauraciones } = useEventStream()

  return <span data-testid="restauraciones">{restauraciones.size}</span>
}

/** El `data` literal del evento de restauración: siete campos, contadores a cero. */
function eventoDeRestauracion(campos: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    execution_id: 'e1',
    snapshot_id: 's1',
    phase: 'restaurando',
    files_done: 0,
    files_total: 0,
    bytes_done: 0,
    bytes_total: 0,
    ...campos,
  }
}

/** Desmonta y comprueba que no queda ninguna conexión abierta. */
function desmontarYComprobar(desmontar: () => void) {
  desmontar()
  exigirTodoCerrado()
}

beforeEach(() => {
  instalarEventSourceFalso()
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('ProveedorDeEventos', () => {
  it('abre una sola conexión aunque haya dos consumidores', () => {
    const { unmount } = montar(
      <>
        <Sonda />
        <Sonda />
      </>,
    )

    expect(conexiones()).toHaveLength(1)
    expect(EventSourceFalso.ultima().url).toBe('/api/v1/events')
    desmontarYComprobar(unmount)
  })

  it('cierra la conexión solo al desmontar el último consumidor', () => {
    const { rerender, unmount } = montar(
      <>
        <Sonda />
        <Sonda />
      </>,
    )
    const conexion = EventSourceFalso.ultima()

    // Se va un consumidor y el proveedor sigue montado: la conexión sigue viva.
    rerender(
      <ProveedorDeEventos>
        <Sonda />
      </ProveedorDeEventos>,
    )
    expect(conexion.cerrado).toBe(false)
    expect(conexiones()).toHaveLength(1)

    // Se va el último: ahora sí.
    unmount()
    expect(conexion.cerrado).toBe(true)
  })

  it('descarta un evento al que le faltan campos en vez de publicarlo a medias', () => {
    const { unmount } = montar(<Sonda />)

    EventSourceFalso.ultima().emitir(NOMBRE_DEL_EVENTO, { execution_id: 'e1', status: 'running' })

    expect(screen.getByTestId('progreso')).toHaveTextContent('sin datos')
    desmontarYComprobar(unmount)
  })

  it('reconecta con una espera que crece en cada intento', () => {
    vi.useFakeTimers()
    const { unmount } = montar(<Sonda />)

    const primera = EventSourceFalso.ultima()
    primera.emitir('error')
    expect(primera.cerrado).toBe(true)

    // Antes de agotar la primera espera no hay conexión nueva.
    act(() => {
      vi.advanceTimersByTime(ESPERA_BASE_MS - 1)
    })
    expect(conexiones()).toHaveLength(1)
    expect(screen.getByTestId('estado')).toHaveTextContent('reconectando')

    act(() => {
      vi.advanceTimersByTime(1)
    })
    expect(conexiones()).toHaveLength(2)

    // El segundo intento espera el doble que el primero.
    EventSourceFalso.ultima().emitir('error')
    act(() => {
      vi.advanceTimersByTime(ESPERA_BASE_MS * 2 - 1)
    })
    expect(conexiones()).toHaveLength(2)

    act(() => {
      vi.advanceTimersByTime(1)
    })
    expect(conexiones()).toHaveLength(3)

    desmontarYComprobar(unmount)
  })

  it('no deja reconectar después de desmontar: la espera pendiente se cancela', () => {
    vi.useFakeTimers()
    const { unmount } = montar(<Sonda />)

    EventSourceFalso.ultima().emitir('error')
    unmount()

    act(() => {
      vi.advanceTimersByTime(ESPERA_BASE_MS * 8)
    })

    expect(conexiones()).toHaveLength(1)
    expect(EventSourceFalso.ultima().cerrado).toBe(true)
  })

  it('publica el evento a los consumidores mientras la conexión vive', () => {
    const { unmount } = montar(<Sonda />)

    EventSourceFalso.ultima().emitir(NOMBRE_DEL_EVENTO, eventoDePrueba({ current_action: 'volcando' }))

    expect(screen.getByTestId('progreso')).toHaveTextContent('volcando|50')
    desmontarYComprobar(unmount)
  })

  it('descarta un `restore_progress` al que le faltan campos en vez de publicarlo a medias', () => {
    const { unmount } = montar(<ContadorDeRestauraciones />)

    EventSourceFalso.ultima().emitir(NOMBRE_DEL_EVENTO_DE_RESTAURACION, {
      execution_id: 'e1',
      phase: 'restaurando',
    })

    expect(screen.getByTestId('restauraciones')).toHaveTextContent('0')
    desmontarYComprobar(unmount)
  })

  it('acota el mapa de restauraciones: recuerda las últimas, no todas', () => {
    const { unmount } = montar(<ContadorDeRestauraciones />)

    // Una sesión larga puede ver muchas restauraciones y el stream no guarda
    // replay: el tope es lo que impide que el mapa crezca sin fin en memoria.
    for (let i = 0; i <= LIMITE_DE_RESTAURACIONES; i += 1) {
      EventSourceFalso.ultima().emitir(
        NOMBRE_DEL_EVENTO_DE_RESTAURACION,
        eventoDeRestauracion({ execution_id: `e${i}` }),
      )
    }

    expect(screen.getByTestId('restauraciones')).toHaveTextContent(
      String(LIMITE_DE_RESTAURACIONES),
    )
    desmontarYComprobar(unmount)
  })
})
