import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { EventSourceFalso, eventoDePrueba, instalarEventSourceFalso } from './eventosDePrueba'
import { ProveedorDeEventos } from './events'
import { NOMBRE_DEL_EVENTO, useEjecucion, useEventStream } from './useEventStream'

/** Pinta el progreso de una ejecución concreta, como haría una pantalla. */
function Consumidor({ executionId }: { executionId: string }) {
  const { evento } = useEjecucion(executionId)

  return (
    <span data-testid={`evento-${executionId}`}>
      {evento === null ? 'sin datos' : `${evento.current_action}|${String(evento.percentage)}`}
    </span>
  )
}

function montar(ui: ReactNode) {
  return render(<ProveedorDeEventos>{ui}</ProveedorDeEventos>)
}

beforeEach(() => {
  instalarEventSourceFalso()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useEjecucion', () => {
  it('publica el evento `execution_progress` con sus siete campos', () => {
    const { unmount } = montar(<Consumidor executionId="e1" />)

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ bytes_processed: 4096, bytes_uploaded: 2048, current_action: 'subiendo' }),
    )

    expect(screen.getByTestId('evento-e1')).toHaveTextContent('subiendo|50')
    unmount()
  })

  it('deja `percentage` en null cuando no se conoce el total', () => {
    const { unmount } = montar(<Consumidor executionId="e1" />)

    EventSourceFalso.ultima().emitir(NOMBRE_DEL_EVENTO, eventoDePrueba({ percentage: null }))

    expect(screen.getByTestId('evento-e1')).toHaveTextContent('subiendo|null')
    unmount()
  })

  it('no contamina la ejecución de un consumidor con el evento de otra', () => {
    const { unmount } = montar(
      <>
        <Consumidor executionId="e1" />
        <Consumidor executionId="e2" />
      </>,
    )

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e2', current_action: 'volcando' }),
    )

    expect(screen.getByTestId('evento-e2')).toHaveTextContent('volcando|50')
    expect(screen.getByTestId('evento-e1')).toHaveTextContent('sin datos')
    unmount()
  })

  it('los identificadores se comparan tal cual, con sus guiones', () => {
    const executionId = '0199f0aa-1111-7000-8000-0000000000ff'
    const { unmount } = montar(<Consumidor executionId={executionId} />)

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: executionId }),
    )

    expect(screen.getByTestId(`evento-${executionId}`)).toHaveTextContent('subiendo|50')
    unmount()
  })

  it('el hook avisa si no hay proveedor por encima', () => {
    const ConsumidorSuelto = () => {
      useEventStream()
      return null
    }

    expect(() => render(<ConsumidorSuelto />)).toThrow(/ProveedorDeEventos/)
  })
})
