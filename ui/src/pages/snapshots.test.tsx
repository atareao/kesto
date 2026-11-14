import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import type { ManejadorDeFetch, RespuestaSimulada } from '../../tests/fetchDePrueba'
import { repositorio, snapshot } from '../../tests/fixtures'
import Snapshots from './snapshots'

const HEX = '0123456789abcdef'.repeat(4)
const REPOS = [repositorio({ id: 'r1', name: 's3' }), repositorio({ id: 'r2', name: 'glacier' })]

function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <Snapshots />
    </QueryClientProvider>,
  )
}

function manejadorCon(sobreSnapshots: ManejadorDeFetch, repos = REPOS): ManejadorDeFetch {
  return (llamada) => {
    if (llamada.ruta === '/api/v1/repositories') return { status: 200, data: repos }
    if (llamada.ruta === '/api/v1/snapshots') return sobreSnapshots(llamada)
    throw new Error(`ruta inesperada: ${llamada.method} ${llamada.ruta}`)
  }
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('pantalla de Snapshots', () => {
  it('lista las instantáneas del primer repositorio, con el hex completo y el aviso de recorte', async () => {
    const fetch = instalarFetchFalso(
      manejadorCon(() => ({
        status: 200,
        data: {
          snapshots: [snapshot({ id: HEX, label: null, files: 12, bytes: 2048 })],
          truncated: true,
        },
      })),
    )

    const { unmount } = montar()

    const tabla = await screen.findByRole('table', { name: 'Snapshots del repositorio' })
    expect(screen.getByText(HEX)).toBeInTheDocument()
    expect(screen.getByText('sin etiqueta')).toBeInTheDocument()
    expect(screen.getByText(/puede haber más instantáneas/i)).toBeInTheDocument()
    expect(screen.getByText('2 KiB')).toBeInTheDocument()

    expect(fetch.a('/api/v1/snapshots')[0]?.url).toContain('repository_id=r1')
    // No existe endpoint de purga: el panel no puede ofrecer el botón.
    expect(screen.queryByRole('button', { name: /purg/i })).not.toBeInTheDocument()
    expect(tabla).toBeInTheDocument()
    unmount()
  })

  it('al cambiar de repositorio pide sus instantáneas', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(({ url }) => {
        const id = new URL(url, 'http://localhost').searchParams.get('repository_id')
        return {
          status: 200,
          data: {
            snapshots: [snapshot({ id: id === 'r2' ? 'b'.repeat(64) : HEX })],
            truncated: false,
          },
        }
      }),
    )

    const { unmount } = montar()
    await screen.findByRole('table', { name: 'Snapshots del repositorio' })

    await usuario.click(screen.getByRole('combobox', { name: 'Repositorio' }))
    await usuario.click(await screen.findByTitle('glacier'))

    await waitFor(() => {
      expect(
        fetch.a('/api/v1/snapshots').some((llamada) => llamada.url.includes('repository_id=r2')),
      ).toBe(true)
    })
    unmount()
  })

  it('un repositorio sin instantáneas enseña un estado vacío', async () => {
    instalarFetchFalso(
      manejadorCon(() => ({
        status: 200,
        data: { snapshots: [], truncated: false },
      })),
    )

    const { unmount } = montar()

    expect(
      await screen.findByText('No hay instantáneas en este repositorio'),
    ).toBeInTheDocument()
    expect(
      screen.queryByRole('table', { name: 'Snapshots del repositorio' }),
    ).not.toBeInTheDocument()
    unmount()
  })

  it('sin repositorios enseña un estado vacío con su explicación', async () => {
    instalarFetchFalso(manejadorCon(() => ({ status: 200, data: { snapshots: [], truncated: false } }), []))

    const { unmount } = montar()

    expect(await screen.findByText('No hay repositorios configurados')).toBeInTheDocument()
    unmount()
  })

  it('mientras carga enseña un indicador visible', async () => {
    instalarFetchFalso(manejadorCon(() => new Promise<RespuestaSimulada>(() => {})))
    // El manejador de snapshots no se llega a usar: repositorios también espera.
    const { unmount } = montar()

    expect(await screen.findByTestId('cargando')).toBeInTheDocument()
    unmount()
  })

  it('un error de carga se explica y se puede reintentar', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(() => ({ status: 500, message: 'el repositorio no responde' })),
    )

    const { unmount } = montar()

    expect(
      await screen.findByText('No se pudieron cargar las instantáneas'),
    ).toBeInTheDocument()
    expect(screen.getByText('el repositorio no responde')).toBeInTheDocument()

    const antes = fetch.a('/api/v1/snapshots').length
    await usuario.click(screen.getByRole('button', { name: 'Reintentar' }))
    await waitFor(() => {
      expect(fetch.a('/api/v1/snapshots').length).toBeGreaterThan(antes)
    })
    unmount()
  })
})
