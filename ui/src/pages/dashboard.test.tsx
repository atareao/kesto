import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { EventSourceFalso, eventoDePrueba, instalarEventSourceFalso } from '../../tests/eventosDePrueba'
import { ProveedorDeEventos } from '../services/events'
import { NOMBRE_DEL_EVENTO } from '../services/useEventStream'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import type { ManejadorDeFetch } from '../../tests/fetchDePrueba'
import type { Repository } from '../services/api/types'
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

/** Las rutas que pide el Dashboard; el repositorio 1 llega con salud anotada. */
const manejadorBase: ManejadorDeFetch = ({ ruta, url }) => {
  if (ruta === '/api/v1/repositories') {
    return {
      status: 200,
      data: [
        repositorio({
          id: 'r1',
          name: 's3-uno',
          health: 'ok',
          health_checked_at: '2026-01-02T03:04:05Z',
        }),
        repositorio({ id: 'r2', name: 's3-dos' }),
      ],
    }
  }
  if (ruta === '/api/v1/jobs') {
    return {
      status: 200,
      data: [
        tarea({ id: 'j1', target_id: 't1', name: 'copia web' }),
        tarea({ id: 'j2', target_id: 't2', name: 'copia base' }),
        tarea({ id: 'j3', target_id: 't3', name: 'copia pausada', enabled: false }),
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
    return {
      status: 200,
      data: { executions: [ejecucion({ id: 'e1', job_id: 'j1', status: 'success' })], total: 1 },
    }
  }
  if (ruta === '/api/v1/snapshots') {
    const id = new URL(url, 'http://localhost').searchParams.get('repository_id')
    return {
      status: 200,
      data: {
        snapshots: [snapshot({ id: `s-${id}`, bytes: 4 * 1024 })],
        truncated: false,
        total: id === 'r2' ? 500 : 1,
      },
    }
  }
  throw new Error(`ruta inesperada: ${ruta}`)
}

/** El mismo panel, pero con el listado de repositorios que pida la prueba. */
function manejadorConRepositorios(delPanel: Repository[]): ManejadorDeFetch {
  return (llamada) => {
    if (llamada.ruta === '/api/v1/repositories') return { status: 200, data: delPanel }
    return manejadorBase(llamada)
  }
}

beforeEach(() => {
  instalarEventSourceFalso()
  instalarFetchFalso(manejadorBase)
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

    await screen.findByText('copia web')
    expect(within(filaDe('copia web')).getByText('Correcta')).toBeInTheDocument()

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e2', job_id: 'j1', status: 'running', percentage: 42 }),
    )

    expect(within(filaDe('copia web')).getByText('En curso')).toBeInTheDocument()
    expect(screen.getAllByText('42%').length).toBeGreaterThan(0)
    unmount()
  })

  it('una tarea activa sin ejecuciones no se pinta como correcta', async () => {
    const { unmount } = montar()

    await screen.findByText('copia base')

    expect(within(filaDe('copia base')).getByText('Sin ejecuciones')).toBeInTheDocument()
    expect(within(filaDe('copia base')).queryByText('Correcta')).not.toBeInTheDocument()
    unmount()
  })

  it('muestra la salud persistida de los repositorios sin disparar ninguna comprobación', async () => {
    const fetch = instalarFetchFalso(manejadorBase)

    montar()

    const tabla = await screen.findByRole('table', { name: 'Salud de los repositorios' })
    expect(within(filaDe('s3-uno')).getByText('Correcto')).toBeInTheDocument()
    // El repositorio 2 nunca se comprobó: se pinta como desconocido, no como sano.
    expect(within(filaDe('s3-dos')).getByText('Desconocido')).toBeInTheDocument()

    // Cargar el Dashboard no comprueba nada contra los destinos.
    expect(fetch.a('/api/v1/repositories/health')).toHaveLength(0)
    expect(fetch.a('/api/v1/repositories/r1/health')).toHaveLength(0)
    expect(fetch.a('/api/v1/repositories/r2/health')).toHaveLength(0)
    expect(fetch.llamadas.every((llamada) => llamada.method === 'GET')).toBe(true)
    expect(tabla).toBeInTheDocument()
  })

  it('con un repositorio caído, el listado sigue enseñando a los sanos', async () => {
    // El listado del Dashboard no es todo-o-nada: un destino caído se nombra en
    // su fila y no borra la información de los que sí responden.
    const fetch = instalarFetchFalso(
      manejadorConRepositorios([
        repositorio({
          id: 'r1',
          name: 'sano',
          health: 'ok',
          health_checked_at: '2026-01-02T03:04:05Z',
        }),
        repositorio({
          id: 'r2',
          name: 'caido',
          health: 'inaccesible',
          health_detail: 'el destino no responde',
        }),
      ]),
    )

    montar()

    await screen.findByRole('table', { name: 'Salud de los repositorios' })

    const sano = filaDe('sano')
    const caido = filaDe('caido')
    // El caído se nombra, con su motivo…
    expect(within(caido).getByText('Inaccesible')).toBeInTheDocument()
    expect(within(caido).getByText('el destino no responde')).toBeInTheDocument()
    // …y el sano sigue ahí, con su estado y su fecha.
    expect(within(sano).getByText('Correcto')).toBeInTheDocument()
    expect(within(sano).getByText(/última comprobación/i)).toHaveTextContent('2026')
    // El resumen de arriba tampoco se sustituye por un error global.
    expect(screen.getByTestId('tarjeta-repositorios')).toBeInTheDocument()
    expect(screen.queryByText('No se pudo cargar el resumen')).not.toBeInTheDocument()
    // Y en ningún caso se ha lanzado una comprobación al cargar.
    expect(fetch.llamadas.every((llamada) => llamada.method === 'GET')).toBe(true)
  })
})
