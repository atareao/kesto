import { render, screen } from '@testing-library/react'
import {
  EventSourceFalso,
  eventoDePrueba,
  instalarEventSourceFalso,
} from '../../services/eventosDePrueba'
import { ProveedorDeEventos } from '../../services/events'
import { LIMITE_DE_LINEAS, NOMBRE_DEL_EVENTO } from '../../services/useEventStream'
import { ConsolaDeEjecucion } from './ConsolaDeEjecucion'

function montar(executionId: string | null) {
  return render(
    <ProveedorDeEventos>
      <ConsolaDeEjecucion executionId={executionId} />
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
})
