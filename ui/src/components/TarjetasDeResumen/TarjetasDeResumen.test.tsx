import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../../tests/fetchDePrueba'
import { repositorio, snapshot, tarea } from '../../../tests/fixtures'
import { TarjetasDeResumen } from './TarjetasDeResumen'

function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <TarjetasDeResumen />
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('TarjetasDeResumen', () => {
  it('cuenta dos repositorios, las tareas activas y suma el espacio', async () => {
    instalarFetchFalso(({ ruta, url }) => {
      if (ruta === '/api/v1/repositories') {
        return { status: 200, data: [repositorio({ id: 'r1' }), repositorio({ id: 'r2' })] }
      }
      if (ruta === '/api/v1/jobs') {
        return {
          status: 200,
          data: [tarea({ id: 'j1' }), tarea({ id: 'j2' }), tarea({ id: 'j3', enabled: false })],
        }
      }
      if (ruta === '/api/v1/snapshots') {
        const id = new URL(url, 'http://localhost').searchParams.get('repository_id')
        return {
          status: 200,
          data: {
            snapshots:
              id === 'r1'
                ? [snapshot({ id: 's1', bytes: 3 * 1024 })]
                : [snapshot({ id: 's2', bytes: 5 * 1024 })],
            truncated: false,
          },
        }
      }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar()

    expect(await screen.findByTestId('tarjeta-repositorios')).toBeInTheDocument()
    expect(within(screen.getByTestId('tarjeta-repositorios')).getByText('2')).toBeInTheDocument()
    expect(within(screen.getByTestId('tarjeta-tareas')).getByText('2')).toBeInTheDocument()
    expect(within(screen.getByTestId('tarjeta-espacio')).getByText('8 KiB')).toBeInTheDocument()
  })

  it('con un listado recortado la cifra se presenta como mínimo', async () => {
    instalarFetchFalso(({ ruta, url }) => {
      if (ruta === '/api/v1/repositories') {
        return { status: 200, data: [repositorio({ id: 'r1' }), repositorio({ id: 'r2' })] }
      }
      if (ruta === '/api/v1/jobs') return { status: 200, data: [] }
      if (ruta === '/api/v1/snapshots') {
        const id = new URL(url, 'http://localhost').searchParams.get('repository_id')
        return {
          status: 200,
          data: {
            snapshots: [snapshot({ id: `s-${id}`, bytes: 1024 })],
            truncated: id === 'r2',
          },
        }
      }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar()

    const tarjeta = await screen.findByTestId('tarjeta-espacio')
    expect(within(tarjeta).getByText(/al menos/i)).toBeInTheDocument()
    expect(within(tarjeta).queryByText(/^Espacio en S3$/)).not.toBeInTheDocument()
  })
})
