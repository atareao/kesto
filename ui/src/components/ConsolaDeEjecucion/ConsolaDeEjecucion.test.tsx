import { render, screen } from '@testing-library/react'
import {
  EventSourceFalso,
  eventoDePrueba,
  instalarEventSourceFalso,
} from '../../../tests/eventosDePrueba'
import { ProveedorDeEventos } from '../../services/events'
import type { Estado } from '../../services/api/types'
import { LIMITE_DE_LINEAS, NOMBRE_DEL_EVENTO } from '../../services/useEventStream'
import { ConsolaDeEjecucion } from './ConsolaDeEjecucion'

function montar(
  executionId: string | null,
  resumen: { status?: Estado; error_message?: string | null } = {},
) {
  return render(
    <ProveedorDeEventos>
      <ConsolaDeEjecucion
        executionId={executionId}
        status={resumen.status ?? null}
        errorMessage={resumen.error_message ?? null}
      />
    </ProveedorDeEventos>,
  )
}

beforeEach(() => {
  instalarEventSourceFalso()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('ConsolaDeEjecucion', () => {
  it('pide seleccionar una ejecución antes de abrir la consola', () => {
    const { unmount } = montar(null)

    expect(
      screen.getByText('Selecciona una ejecución para ver su consola'),
    ).toBeInTheDocument()
    unmount()
  })

  it('solo pinta los eventos de la ejecución seleccionada', () => {
    const { unmount } = montar('e1')

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e2', current_action: 'accion-ajena' }),
    )
    expect(screen.queryByText(/accion-ajena/)).not.toBeInTheDocument()

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e1', current_action: 'accion-propia' }),
    )
    expect(screen.getByText(/accion-propia/)).toBeInTheDocument()
    unmount()
  })

  it('avisa cuando la conexión se cae', () => {
    const { unmount } = montar('e1')

    expect(
      screen.queryByText(/se ha perdido la conexión con el servidor/i),
    ).not.toBeInTheDocument()

    EventSourceFalso.ultima().emitir('error')

    expect(
      screen.getByText(/se ha perdido la conexión con el servidor/i),
    ).toBeInTheDocument()
    unmount()
  })

  it('conserva solo las últimas líneas', () => {
    const { unmount } = montar('e1')

    for (let indice = 0; indice < LIMITE_DE_LINEAS + 5; indice += 1) {
      EventSourceFalso.ultima().emitir(
        NOMBRE_DEL_EVENTO,
        eventoDePrueba({ execution_id: 'e1', current_action: `linea-${indice}` }),
      )
    }

    const lineas = screen.getAllByTestId('linea-de-consola')
    expect(lineas).toHaveLength(LIMITE_DE_LINEAS)
    expect(lineas[0]).toHaveTextContent('linea-5')
    expect(lineas.at(-1)).toHaveTextContent(`linea-${LIMITE_DE_LINEAS + 4}`)
    unmount()
  })

  it('muestra el error_message de una ejecución fallida terminada', () => {
    const { unmount } = montar('e1', {
      status: 'failed',
      error_message: 'el repositorio no responde',
    })

    expect(screen.getByText('el repositorio no responde')).toBeInTheDocument()
    // No inventa líneas: sigue sin haber ninguna de consola.
    expect(screen.queryByTestId('linea-de-consola')).not.toBeInTheDocument()
    unmount()
  })

  it('avisa de que no hay eventos cuando la ejecución terminó sin líneas', () => {
    const { unmount } = montar('e1', { status: 'success' })

    expect(screen.getByText(/no hay eventos que mostrar/i)).toBeInTheDocument()
    expect(screen.queryByTestId('linea-de-consola')).not.toBeInTheDocument()
    unmount()
  })

  it('una ejecución en curso sin líneas espera a los eventos', () => {
    const { unmount } = montar('e1', { status: 'running' })

    expect(screen.getByText(/todavía no hay eventos/i)).toBeInTheDocument()
    unmount()
  })
})
