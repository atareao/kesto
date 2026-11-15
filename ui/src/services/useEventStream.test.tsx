import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import {
  EventSourceFalso,
  conexiones,
  eventoDePrueba,
  instalarEventSourceFalso,
} from '../../tests/eventosDePrueba'
import { ProveedorDeEventos } from './events'
import {
  NOMBRE_DEL_EVENTO,
  NOMBRE_DEL_EVENTO_DE_RESTAURACION,
  useEjecucion,
  useEventStream,
  useRestauracion,
} from './useEventStream'

/** Pinta el progreso de una ejecución concreta, como haría una pantalla. */
function Consumidor({ executionId }: { executionId: string }) {
  const { evento } = useEjecucion(executionId)

  return (
    <span data-testid={`evento-${executionId}`}>
      {evento === null ? 'sin datos' : `${evento.current_action}|${String(evento.percentage)}`}
    </span>
  )
}

/**
 * Pinta la fase de la restauración que se sigue. `executionId` puede ser `null`:
 * es el estado real de una pantalla entre que el operador confirma y el `POST`
 * contesta con la ejecución.
 */
function ConsumidorDeRestauracion({ executionId }: { executionId: string | null }) {
  const { evento, estado } = useRestauracion(executionId)

  return (
    <span data-testid="restauracion">
      {evento === null ? 'sin datos' : `${evento.phase}|${String(evento.files_done)}`}
      {`|${estado ?? 'sin proveedor'}`}
    </span>
  )
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

describe('useRestauracion', () => {
  it('refleja la fase del evento `restore_progress` de la ejecución que se sigue', () => {
    const { unmount } = montar(<ConsumidorDeRestauracion executionId="e1" />)

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO_DE_RESTAURACION,
      eventoDeRestauracion({ phase: 'terminada' }),
    )

    expect(screen.getByTestId('restauracion')).toHaveTextContent('terminada|0')
    unmount()
  })

  it('no contamina la restauración de un consumidor con la de otra ejecución', () => {
    const { unmount } = montar(
      <>
        <ConsumidorDeRestauracion executionId="e1" />
        <ConsumidorDeRestauracion executionId="e2" />
      </>,
    )

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO_DE_RESTAURACION,
      eventoDeRestauracion({ execution_id: 'e2', phase: 'fallida' }),
    )

    const [deE1, deE2] = screen.getAllByTestId('restauracion')
    expect(deE2).toHaveTextContent('fallida|0')
    // El consumidor de e1 sigue sin datos: el suyo no ha llegado.
    expect(deE1).toHaveTextContent('sin datos')
    unmount()
  })

  it('se suscribe antes de conocer la ejecución y no pierde lo que llegue entretanto', () => {
    // Una pantalla se suscribe al confirmar, no al recibir el `execution_id`: el
    // primer aviso se publica nada más lanzar el trabajo y el stream no guarda
    // replay, así que la conexión tiene que estar abierta ya.
    const { rerender, unmount } = montar(<ConsumidorDeRestauracion executionId={null} />)
    expect(conexiones()).toHaveLength(1)

    EventSourceFalso.ultima().emitir(NOMBRE_DEL_EVENTO_DE_RESTAURACION, eventoDeRestauracion())
    expect(screen.getByTestId('restauracion')).toHaveTextContent('sin datos')

    // En cuanto el `POST` dice en qué ejecución corre, la fase ya está ahí.
    rerender(
      <ProveedorDeEventos>
        <ConsumidorDeRestauracion executionId="e1" />
      </ProveedorDeEventos>,
    )
    expect(screen.getByTestId('restauracion')).toHaveTextContent('restaurando|0')

    // Y sigue siendo la misma conexión: la operación no abre ni cierra ninguna.
    expect(conexiones()).toHaveLength(1)
    unmount()
  })

  it('sin proveedor por encima devuelve `null` en vez de romperse', () => {
    const ConsumidorSuelto = () => {
      const { evento, estado } = useRestauracion('e1')
      return <span data-testid="suelto">{`${String(evento)}|${String(estado)}`}</span>
    }

    render(<ConsumidorSuelto />)

    expect(screen.getByTestId('suelto')).toHaveTextContent('null|null')
  })
})
