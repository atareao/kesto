import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { EventSourceFalso, eventoDePrueba, instalarEventSourceFalso } from '../../tests/eventosDePrueba'
import { ProveedorDeEventos } from '../services/events'
import { NOMBRE_DEL_EVENTO } from '../services/useEventStream'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import { ejecucion, repositorio, snapshot, tarea, target } from '../../tests/fixtures'
import Dashboard from './dashboard'

function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <ProveedorDeEventos>
        <Dashboard />
      </ProveedorDeEventos>
    </QueryClientProvider>,
  )
}

function filaDe(nombre: string): HTMLElement {
  const fila = screen.getByText(nombre).closest('tr')
  if (fila === null) throw new Error(`no hay fila para ${nombre}`)
  return fila as HTMLElement
}

beforeEach(() => {
  instalarEventSourceFalso()
  instalarFetchFalso(({ ruta, url }) => {
    if (ruta === '/api/v1/repositories') {
      return { status: 200, data: [repositorio({ id: 'r1' }), repositorio({ id: 'r2' })] }
    }
    if (ruta === '/api/v1/jobs') {
      return {
        status: 200,
        data: [
          tarea({ id: 'j1', target_id: 't1' }),
          tarea({ id: 'j2', target_id: 't2' }),
          tarea({ id: 'j3', target_id: 't3', enabled: false }),
        ],
      }
    }
    if (ruta === '/api/v1/targets') {
      return {
        status: 200,
        data: [
          target({ id: 't1', name: 'web' }),
          target({ id: 't2', name: 'base' }),
          target({ id: 't3', name: 'pausada' }),
        ],
      }
    }
    if (ruta === '/api/v1/executions') {
      return { status: 200, data: [ejecucion({ id: 'e1', job_id: 'j1', status: 'success' })] }
    }
    if (ruta === '/api/v1/snapshots') {
      const id = new URL(url, 'http://localhost').searchParams.get('repository_id')
      return {
        status: 200,
        data: {
          snapshots: [snapshot({ id: `s-${id}`, bytes: 4 * 1024 })],
          truncated: id === 'r2',
        },
      }
    }
    throw new Error(`ruta inesperada: ${ruta}`)
  })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('Dashboard', () => {
  it('resume repositorios, tareas activas y espacio mínimo, y pinta el avance en vivo', async () => {
    const { unmount } = montar()

    expect(await screen.findByTestId('tarjeta-repositorios')).toBeInTheDocument()
    expect(within(screen.getByTestId('tarjeta-repositorios')).getByText('2')).toBeInTheDocument()
    expect(within(screen.getByTestId('tarjeta-tareas')).getByText('2')).toBeInTheDocument()
    expect(within(screen.getByTestId('tarjeta-espacio')).getByText(/al menos/i)).toBeInTheDocument()
    expect(within(screen.getByTestId('tarjeta-espacio')).getByText('8 KiB')).toBeInTheDocument()

    await screen.findByText('web')
    expect(within(filaDe('web')).getByText('Correcta')).toBeInTheDocument()

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e2', job_id: 'j1', status: 'running', percentage: 42 }),
    )

    expect(within(filaDe('web')).getByText('En curso')).toBeInTheDocument()
    expect(screen.getAllByText('42%').length).toBeGreaterThan(0)
    unmount()
  })

  it('una tarea activa sin ejecuciones no se pinta como correcta', async () => {
    const { unmount } = montar()

    await screen.findByText('base')

    expect(within(filaDe('base')).getByText('Sin ejecuciones')).toBeInTheDocument()
    expect(within(filaDe('base')).queryByText('Correcta')).not.toBeInTheDocument()
    unmount()
  })
})
