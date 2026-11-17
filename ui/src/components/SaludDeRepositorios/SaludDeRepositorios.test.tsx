import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../../tests/fetchDePrueba'
import { repositorio } from '../../../tests/fixtures'
import { SaludDeRepositorios } from './SaludDeRepositorios'

function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <SaludDeRepositorios />
    </QueryClientProvider>,
  )
}

function filaDe(nombre: string): HTMLElement {
  const fila = screen.getByText(nombre).closest('tr')
  if (fila === null) throw new Error(`no hay fila para ${nombre}`)
  return fila as HTMLElement
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('SaludDeRepositorios', () => {
  it('muestra el estado persistido de cada repositorio sin disparar ninguna comprobación', async () => {
    const fetch = instalarFetchFalso(({ ruta }) => {
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
            repositorio({ id: 'r2', name: 's3-dos', health: 'desconocido' }),
          ],
        }
      }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar()

    const tabla = await screen.findByRole('table', { name: 'Salud de los repositorios' })
    expect(within(filaDe('s3-uno')).getByText('Correcto')).toBeInTheDocument()
    expect(within(filaDe('s3-dos')).getByText('Desconocido')).toBeInTheDocument()

    // La salud que se pinta es la **persistida**: el Dashboard no comprueba nada.
    expect(fetch.llamadas.every((llamada) => llamada.method === 'GET')).toBe(true)
    expect(fetch.a('/api/v1/repositories/health')).toHaveLength(0)
    expect(fetch.a('/api/v1/repositories/r1/health')).toHaveLength(0)
    expect(fetch.a('/api/v1/repositories/r2/health')).toHaveLength(0)
    expect(tabla).toBeInTheDocument()
  })

  it('un repositorio sin comprobar no se pinta como sano', async () => {
    instalarFetchFalso(() => ({
      status: 200,
      data: [repositorio({ id: 'r1', name: 'nuevo', health: 'desconocido' })],
    }))

    montar()

    await screen.findByText('nuevo')
    const fila = filaDe('nuevo')
    expect(within(fila).getByText('Desconocido')).toBeInTheDocument()
    expect(within(fila).queryByText('Correcto')).not.toBeInTheDocument()
    // Y el distintivo va en el gris neutro: un cambio que lo pintase en verde
    // haría pasar «no se pinta como sano» sin ser cierto.
    const distintivo = fila.querySelector('.ant-badge')
    expect(distintivo?.querySelector('.ant-badge-status-default')).not.toBeNull()
    expect(distintivo?.querySelector('.ant-badge-status-success')).toBeNull()
  })

  it('un repositorio caído no oculta la información de los sanos', async () => {
    instalarFetchFalso(() => ({
      status: 200,
      data: [
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
      ],
    }))

    montar()

    await screen.findByRole('table', { name: 'Salud de los repositorios' })
    const sano = filaDe('sano')
    const caido = filaDe('caido')

    expect(within(sano).getByText('Correcto')).toBeInTheDocument()
    expect(within(caido).getByText('Inaccesible')).toBeInTheDocument()
    expect(within(caido).getByText('el destino no responde')).toBeInTheDocument()
  })

  it('sin repositorios enseña un estado vacío, no una tabla huérfana', async () => {
    instalarFetchFalso(({ ruta }) => {
      if (ruta === '/api/v1/repositories') return { status: 200, data: [] }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar()

    expect(await screen.findByText('No hay repositorios configurados')).toBeInTheDocument()
    expect(screen.queryByRole('table')).not.toBeInTheDocument()
  })

  it('un error de carga se explica con reintento, no como JSON crudo', async () => {
    instalarFetchFalso(() => ({ status: 500, message: 'la base de datos no responde' }))

    montar()

    expect(
      await screen.findByText('No se pudo cargar la salud de los repositorios'),
    ).toBeInTheDocument()
    expect(screen.getByText('la base de datos no responde')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Reintentar' })).toBeInTheDocument()
  })
})
