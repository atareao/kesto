import { render, screen } from '@testing-library/react'
import { EventSourceFalso, eventoDePrueba, instalarEventSourceFalso } from '../../../tests/eventosDePrueba'
import { ProveedorDeEventos } from '../../services/events'
import { NOMBRE_DEL_EVENTO } from '../../services/useEventStream'
import { ProgresoEnVivo } from './ProgresoEnVivo'

function montar() {
  return render(
    <ProveedorDeEventos>
      <ProgresoEnVivo />
    </ProveedorDeEventos>,
  )
}

beforeEach(() => {
  instalarEventSourceFalso()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('ProgresoEnVivo', () => {
  it('enseña que no hay copia en curso antes del primer evento', () => {
    const { unmount } = montar()
    expect(screen.getByText('No hay ninguna copia en curso')).toBeInTheDocument()
    unmount()
  })

  it('un evento SSE actualiza el porcentaje y el estado', () => {
    const { unmount } = montar()

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ status: 'running', percentage: 42, current_action: 'subiendo' }),
    )

    expect(screen.getByText('42%')).toBeInTheDocument()
    expect(screen.getByText('En curso · subiendo')).toBeInTheDocument()
    unmount()
  })

  it('un `percentage` nulo pinta la barra indeterminada, no un 0', () => {
    const { unmount } = montar()

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ status: 'running', percentage: null }),
    )

    expect(screen.getByTestId('progreso-en-vivo')).toHaveAttribute('data-indeterminado', 'true')
    expect(screen.getByText('Progreso indeterminado')).toBeInTheDocument()
    expect(screen.queryByText('0%')).not.toBeInTheDocument()
    unmount()
  })
})
