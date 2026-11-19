import { render, screen, waitFor, within } from '@testing-library/react'
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
            total: 1,
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
            truncated: false,
            total: id === 'r2' ? 500 : 1,
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

/**
 * El resumen no es **todo-o-nada**: el fallo de un repositorio no puede borrar
 * la información de los que sí responden. Las tres tarjetas siguen ahí, el aviso
 * **nombra** el repositorio que falla y la cifra de espacio se declara mínimo.
 */
describe('TarjetasDeResumen — un repositorio caído no colapsa el resumen', () => {
  it('conserva las cifras que no dependen del S3 y nombra el repositorio que falla', async () => {
    instalarFetchFalso(({ ruta, url }) => {
      if (ruta === '/api/v1/repositories') {
        return {
          status: 200,
          data: [repositorio({ id: 'r1', name: 'sano' }), repositorio({ id: 'r2', name: 'caido' })],
        }
      }
      if (ruta === '/api/v1/jobs') {
        return {
          status: 200,
          data: [tarea({ id: 'j1' }), tarea({ id: 'j2' }), tarea({ id: 'j3', enabled: false })],
        }
      }
      if (ruta === '/api/v1/snapshots') {
        const id = new URL(url, 'http://localhost').searchParams.get('repository_id')
        if (id === 'r2') return { status: 503, message: 'el repositorio no responde' }
        return { status: 200, data: { snapshots: [snapshot({ id: 's1', bytes: 2048 })], total: 1, truncated: false } }
      }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar()

    // Ninguna tarjeta se sustituye por un error global.
    expect(within(await screen.findByTestId('tarjeta-repositorios')).getByText('2')).toBeInTheDocument()
    expect(within(screen.getByTestId('tarjeta-tareas')).getByText('2')).toBeInTheDocument()
    expect(screen.queryByText('No se pudo cargar el resumen')).not.toBeInTheDocument()

    // El aviso nombra al caído, no al sano.
    const aviso = await screen.findByTestId('aviso-de-repositorios-caidos')
    expect(aviso).toHaveTextContent('caido')
    expect(aviso).not.toHaveTextContent('sano')

    // La cifra de espacio es un **mínimo** y trae su acción de reintentar.
    const tarjeta = screen.getByTestId('tarjeta-espacio')
    expect(await within(tarjeta).findByText(/al menos/i)).toBeInTheDocument()
    expect(within(aviso).getByRole('button', { name: 'Reintentar' })).toBeInTheDocument()
  })

  it('con el único repositorio caído, el espacio cae en error con reintentar y no en carga perpetua', async () => {
    instalarFetchFalso(({ ruta }) => {
      if (ruta === '/api/v1/repositories') {
        return { status: 200, data: [repositorio({ id: 'r1', name: 'caido' })] }
      }
      if (ruta === '/api/v1/jobs') return { status: 200, data: [] }
      if (ruta === '/api/v1/snapshots') return { status: 503, message: 'el repositorio no responde' }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar()

    expect(await screen.findByTestId('aviso-de-repositorios-caidos')).toHaveTextContent('caido')
    // La consulta acotada alcanza un desenlace: no queda ningún esqueleto de carga.
    expect(document.querySelectorAll('.ant-skeleton')).toHaveLength(0)
    expect(screen.queryByTestId('cargando')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Reintentar' })).toBeInTheDocument()
    // Y las tres tarjetas siguen presentes, con el espacio declarado mínimo.
    expect(screen.getByTestId('tarjeta-repositorios')).toBeInTheDocument()
    expect(within(screen.getByTestId('tarjeta-espacio')).getByText(/al menos/i)).toBeInTheDocument()
  })

  it('con todos los repositorios sanos no aparece ningún aviso y la suma es el total', async () => {
    instalarFetchFalso(({ ruta }) => {
      if (ruta === '/api/v1/repositories') {
        return { status: 200, data: [repositorio({ id: 'r1', name: 'sano' })] }
      }
      if (ruta === '/api/v1/jobs') return { status: 200, data: [] }
      if (ruta === '/api/v1/snapshots') {
        return { status: 200, data: { snapshots: [snapshot({ id: 's1', bytes: 1024 })], total: 1, truncated: false } }
      }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar()

    const tarjeta = await screen.findByTestId('tarjeta-espacio')
    expect(await within(tarjeta).findByText('1 KiB')).toBeInTheDocument()
    expect(within(tarjeta).getByText(/^Espacio en S3$/)).toBeInTheDocument()
    expect(screen.queryByTestId('aviso-de-repositorios-caidos')).not.toBeInTheDocument()
  })

  it('mientras el espacio carga, la tarjeta lo anuncia de forma accesible', async () => {
    instalarFetchFalso(({ ruta }) => {
      if (ruta === '/api/v1/repositories') {
        return { status: 200, data: [repositorio({ id: 'r1', name: 'sano' })] }
      }
      if (ruta === '/api/v1/jobs') return { status: 200, data: [] }
      // El listado nunca resuelve: la tarjeta de espacio se queda cargando.
      if (ruta === '/api/v1/snapshots') return new Promise<never>(() => {})
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar()

    const tarjeta = await screen.findByTestId('tarjeta-espacio')
    await waitFor(() => expect(tarjeta).toHaveAttribute('aria-busy', 'true'))

    // El esqueleto mudo no basta: una región `status` con su texto para lectores
    // de pantalla es lo que hace anunciable la carga.
    const region = within(tarjeta).getByRole('status')
    expect(region).toHaveAttribute('aria-busy', 'true')
    expect(within(region).getByText(/cargando el espacio/i)).toHaveClass('sr-only')
  })
})
