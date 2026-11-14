import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  EventSourceFalso,
  eventoDePrueba,
  instalarEventSourceFalso,
} from '../../tests/eventosDePrueba'
import { ProveedorDeEventos } from '../services/events'
import { NOMBRE_DEL_EVENTO } from '../services/useEventStream'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import type { ManejadorDeFetch, RespuestaSimulada } from '../../tests/fetchDePrueba'
import { ejecucion, target, tarea } from '../../tests/fixtures'
import Historico from './historico'

type Usuario = ReturnType<typeof userEvent.setup>

const TAMANO = 20
const J1 = tarea({ id: 'j1', target_id: 't1', repository_id: 'r1' })
const TARGETS = [target({ id: 't1', name: 'web' })]
const TAREAS = [J1]

function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <ProveedorDeEventos>
        <Historico />
      </ProveedorDeEventos>
    </QueryClientProvider>,
  )
}

function lote(cantidad: number, desde = 0) {
  return Array.from({ length: cantidad }, (_valor, indice) =>
    ejecucion({ id: `e${desde + indice}`, job_id: 'j1', status: 'success' }),
  )
}

/** Rutas comunes a todas las pruebas: tareas y targets para el filtro. */
function manejadorCon(sobreEjecuciones: ManejadorDeFetch): ManejadorDeFetch {
  return (llamada) => {
    if (llamada.ruta === '/api/v1/jobs') return { status: 200, data: TAREAS }
    if (llamada.ruta === '/api/v1/targets') return { status: 200, data: TARGETS }
    if (llamada.ruta === '/api/v1/executions') return sobreEjecuciones(llamada)
    throw new Error(`ruta inesperada: ${llamada.method} ${llamada.ruta}`)
  }
}

async function elegir(usuario: Usuario, etiqueta: string, opcion: string) {
  await usuario.click(screen.getByRole('combobox', { name: etiqueta }))
  await usuario.click(await screen.findByTitle(opcion))
}

beforeEach(() => {
  instalarEventSourceFalso()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('pantalla de Histórico', () => {
  it('pide limit y offset, y deshabilita «Siguiente» al final', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(({ url }) => {
        const offset = Number(new URL(url, 'http://localhost').searchParams.get('offset') ?? 0)
        return { status: 200, data: offset === 0 ? lote(TAMANO) : lote(5, TAMANO) }
      }),
    )

    const { unmount } = montar()

    await screen.findByRole('table', { name: 'Ejecuciones' })

    const primera = fetch.a('/api/v1/executions')[0]
    expect(primera?.url).toContain(`limit=${TAMANO}`)
    expect(primera?.url).toContain('offset=0')

    const siguiente = screen.getByRole('button', { name: 'Siguiente' })
    expect(siguiente).toBeEnabled()

    await usuario.click(siguiente)

    await waitFor(() => {
      expect(
        fetch.a('/api/v1/executions').some((llamada) => llamada.url.includes('offset=20')),
      ).toBe(true)
    })
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Siguiente' })).toBeDisabled()
    })
    expect(screen.getByRole('button', { name: 'Anterior' })).toBeEnabled()
    unmount()
  })

  it('los filtros viajan en la consulta y vuelven a la primera página', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(() => ({ status: 200, data: lote(3) })),
    )

    const { unmount } = montar()
    await screen.findByRole('table', { name: 'Ejecuciones' })

    await elegir(usuario, 'Tarea', 'web')
    await waitFor(() => {
      expect(fetch.a('/api/v1/executions').at(-1)?.url).toContain('job_id=j1')
    })

    await elegir(usuario, 'Estado', 'Fallida')
    await waitFor(() => {
      const ultima = fetch.a('/api/v1/executions').at(-1)
      expect(ultima?.url).toContain('status=failed')
      expect(ultima?.url).toContain('offset=0')
    })
    unmount()
  })

  it('la consola solo recibe los eventos de la ejecución elegida y avisa si el stream cae', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(
      manejadorCon(() => ({
        status: 200,
        data: [ejecucion({ id: 'e1', job_id: 'j1' }), ejecucion({ id: 'e2', job_id: 'j1' })],
      })),
    )

    const { unmount } = montar()
    await screen.findByRole('table', { name: 'Ejecuciones' })

    await usuario.click(screen.getAllByRole('button', { name: 'Ver consola' })[0] as HTMLElement)

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e2', current_action: 'otra-ejecucion' }),
    )
    expect(screen.queryByText(/otra-ejecucion/)).not.toBeInTheDocument()

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e1', current_action: 'subiendo-datos' }),
    )
    expect(screen.getByText(/subiendo-datos/)).toBeInTheDocument()

    EventSourceFalso.ultima().emitir('error')
    expect(
      screen.getByText(/se ha perdido la conexión con el servidor/i),
    ).toBeInTheDocument()
    unmount()
  })

  it('sin ejecuciones enseña un estado vacío, no una tabla vacía', async () => {
    instalarFetchFalso(manejadorCon(() => ({ status: 200, data: [] })))

    const { unmount } = montar()

    expect(
      await screen.findByText('No hay ejecuciones para estos filtros'),
    ).toBeInTheDocument()
    expect(screen.queryByRole('table', { name: 'Ejecuciones' })).not.toBeInTheDocument()
    unmount()
  })

  it('mientras carga enseña un indicador visible', async () => {
    instalarFetchFalso(manejadorCon(() => new Promise<RespuestaSimulada>(() => {})))

    const { unmount } = montar()

    expect(await screen.findByTestId('cargando')).toBeInTheDocument()
    unmount()
  })

  it('un error de carga se explica y se puede reintentar', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(() => ({ status: 500, message: 'el motor no responde' })),
    )

    const { unmount } = montar()

    expect(
      await screen.findByText('No se pudieron cargar las ejecuciones'),
    ).toBeInTheDocument()
    expect(screen.getByText('el motor no responde')).toBeInTheDocument()

    const antes = fetch.a('/api/v1/executions').length
    await usuario.click(screen.getByRole('button', { name: 'Reintentar' }))
    await waitFor(() => {
      expect(fetch.a('/api/v1/executions').length).toBeGreaterThan(antes)
    })
    unmount()
  })
})

describe('tabla de ejecuciones', () => {
  it('pinta la duración de las terminadas y «en curso» para las vivas', async () => {
    instalarFetchFalso(
      manejadorCon(() => ({
        status: 200,
        data: [
          ejecucion({
            id: 'e1',
            job_id: 'j1',
            status: 'success',
            started_at: '2026-01-02T03:00:00Z',
            finished_at: '2026-01-02T03:05:00Z',
          }),
          ejecucion({
            id: 'e2',
            job_id: 'j1',
            status: 'running',
            started_at: '2026-01-02T03:00:00Z',
            finished_at: null,
          }),
        ],
      })),
    )

    const { unmount } = montar()

    const tabla = await screen.findByRole('table', { name: 'Ejecuciones' })
    expect(within(tabla).getByText('5 min')).toBeInTheDocument()
    expect(within(tabla).getByText('en curso')).toBeInTheDocument()
    unmount()
  })
})
