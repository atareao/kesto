import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { EventSourceFalso, eventoDePrueba, instalarEventSourceFalso } from '../../../tests/eventosDePrueba'
import { ProveedorDeEventos } from '../../services/events'
import { NOMBRE_DEL_EVENTO } from '../../services/useEventStream'
import { instalarFetchFalso } from '../../../tests/fetchDePrueba'
import { ejecucion, tarea, target } from '../../../tests/fixtures'
import { TablaDeTareasEnCurso } from './TablaDeTareasEnCurso'

function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <ProveedorDeEventos>
        <TablaDeTareasEnCurso />
      </ProveedorDeEventos>
    </QueryClientProvider>,
  )
}

function filaDe(nombre: string): HTMLElement {
  const celda = screen.getByText(nombre)
  const fila = celda.closest('tr')
  if (fila === null) throw new Error(`no hay fila para ${nombre}`)
  return fila as HTMLElement
}

beforeEach(() => {
  instalarEventSourceFalso()
  instalarFetchFalso(({ ruta }) => {
    if (ruta === '/api/v1/jobs') {
      return { status: 200, data: [tarea({ id: 'j1', target_id: 't1' }), tarea({ id: 'j2', target_id: 't2' })] }
    }
    if (ruta === '/api/v1/targets') {
      return { status: 200, data: [target({ id: 't1', name: 'web' }), target({ id: 't2', name: 'base' })] }
    }
    if (ruta === '/api/v1/executions') {
      return { status: 200, data: [ejecucion({ id: 'e1', job_id: 'j1', status: 'success' })] }
    }
    throw new Error(`ruta inesperada: ${ruta}`)
  })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('TablaDeTareasEnCurso', () => {
  it('el estado es el de la última ejecución y el SSE lo actualiza en vivo', async () => {
    const { unmount } = montar()

    await screen.findByText('web')
    expect(within(filaDe('web')).getByText('Correcta')).toBeInTheDocument()

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e2', job_id: 'j1', status: 'running', percentage: 42 }),
    )

    expect(within(filaDe('web')).getByText('En curso')).toBeInTheDocument()
    expect(within(filaDe('web')).getByText('42%')).toBeInTheDocument()
    unmount()
  })

  it('una tarea sin ejecuciones se pinta como «sin ejecuciones», no como éxito', async () => {
    const { unmount } = montar()

    await screen.findByText('base')

    expect(within(filaDe('base')).getByText('Sin ejecuciones')).toBeInTheDocument()
    expect(within(filaDe('base')).queryByText('Correcta')).not.toBeInTheDocument()
    unmount()
  })
})
