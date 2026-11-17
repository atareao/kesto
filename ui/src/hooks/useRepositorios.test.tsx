import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import type { RespuestaSimulada } from '../../tests/fetchDePrueba'
import type { Repository } from '../services/api/types'
import { repositorio } from '../../tests/fixtures'
import { claves } from './claves'
import {
  useComprobarSaludDeRepositorio,
  useComprobarSaludDeTodos,
  useRepositorios,
} from './useRepositorios'

function clienteDePrueba(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } } })
}

function envolver(cliente: QueryClient) {
  return function Envoltorio({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={cliente}>{children}</QueryClientProvider>
  }
}

/** Lo que montan las pruebas: el listado y las dos comprobaciones, a la vez. */
function useRepositoriosYComprobaciones() {
  return {
    listado: useRepositorios(),
    comprobarUno: useComprobarSaludDeRepositorio(),
    comprobarTodos: useComprobarSaludDeTodos(),
  }
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useComprobarSaludDeRepositorio', () => {
  it('comprueba el repositorio e invalida el listado para que el indicador refleje lo persistido', async () => {
    let listado: Repository[] = [repositorio({ id: 'r1', name: 's3-uno' })]
    const fetch = instalarFetchFalso(({ ruta, method }) => {
      if (ruta === '/api/v1/repositories' && method === 'GET') return { status: 200, data: listado }
      if (ruta === '/api/v1/repositories/r1/health') {
        listado = [
          repositorio({
            id: 'r1',
            name: 's3-uno',
            health: 'inaccesible',
            health_checked_at: '2026-01-02T03:04:05Z',
            health_detail: 'el destino no responde',
          }),
        ]
        return {
          status: 200,
          data: {
            health: 'inaccesible',
            health_checked_at: '2026-01-02T03:04:05Z',
            health_detail: 'el destino no responde',
          },
        }
      }
      throw new Error(`ruta inesperada: ${method} ${ruta}`)
    })

    const cliente = clienteDePrueba()
    const { result } = renderHook(() => useRepositoriosYComprobaciones(), { wrapper: envolver(cliente) })

    await waitFor(() => expect(result.current.listado.data).toHaveLength(1))

    await result.current.comprobarUno.mutateAsync('r1')

    // La comprobación se pidió por su ruta y el listado se volvió a pedir: el
    // indicador enseña lo que quedó **persistido**, no una suposición del panel.
    expect(fetch.a('/api/v1/repositories/r1/health')).toHaveLength(1)
    await waitFor(() => expect(fetch.a('/api/v1/repositories')).toHaveLength(2))
    await waitFor(() =>
      expect(result.current.listado.data?.[0]?.health).toBe('inaccesible'),
    )
  })

  it('mientras una comprobación está en curso, la identifica por repositorio y no bloquea el listado', async () => {
    // La comprobación de `r1` no termina hasta que la prueba lo decide: así se
    // puede afirmar lo que pasa **mientras** está en curso.
    const pendiente: { resolver?: (respuesta: RespuestaSimulada) => void } = {}
    instalarFetchFalso(({ ruta, method }) => {
      if (ruta === '/api/v1/repositories' && method === 'GET') {
        return { status: 200, data: [repositorio({ id: 'r1' }), repositorio({ id: 'r2' })] }
      }
      if (ruta === '/api/v1/repositories/r1/health') {
        return new Promise<RespuestaSimulada>((resolver) => {
          pendiente.resolver = resolver
        })
      }
      throw new Error(`ruta inesperada: ${method} ${ruta}`)
    })

    const cliente = clienteDePrueba()
    const { result } = renderHook(() => useRepositoriosYComprobaciones(), { wrapper: envolver(cliente) })

    await waitFor(() => expect(result.current.listado.data).toHaveLength(2))

    // No se espera la promesa: lo que se afirma pasa mientras está en curso.
    void result.current.comprobarUno.mutateAsync('r1').catch(() => undefined)

    await waitFor(() => expect(result.current.comprobarUno.isPending).toBe(true))
    expect(result.current.comprobarUno.variables).toBe('r1')
    // El listado sigue resuelto: una comprobación en curso no deja la pantalla a oscuras.
    expect(result.current.listado.isSuccess).toBe(true)
    expect(result.current.listado.data?.[1]?.name).toBe('r2')

    pendiente.resolver?.({
      status: 200,
      data: { health: 'ok', health_checked_at: null, health_detail: null },
    })
    await waitFor(() => expect(result.current.comprobarUno.isPending).toBe(false))
  })

  it('un fallo de la comprobación no invalida el listado y deja el error a la vista', async () => {
    const fetch = instalarFetchFalso(({ ruta, method }) => {
      if (ruta === '/api/v1/repositories' && method === 'GET') {
        return { status: 200, data: [repositorio({ id: 'r1' })] }
      }
      if (ruta === '/api/v1/repositories/r1/health') {
        return { status: 500, message: 'no se pudo hablar con el destino' }
      }
      throw new Error(`ruta inesperada: ${method} ${ruta}`)
    })

    const cliente = clienteDePrueba()
    const { result } = renderHook(() => useRepositoriosYComprobaciones(), { wrapper: envolver(cliente) })

    await waitFor(() => expect(result.current.listado.data).toHaveLength(1))

    await expect(result.current.comprobarUno.mutateAsync('r1')).rejects.toThrow(
      'no se pudo hablar con el destino',
    )
    await waitFor(() => expect(result.current.comprobarUno.error).not.toBeNull())
    // El listado se pidió una sola vez: el fallo no dispara una recarga inútil.
    expect(fetch.a('/api/v1/repositories')).toHaveLength(1)
  })
})

describe('useComprobarSaludDeTodos', () => {
  it('comprueba todos e invalida el listado', async () => {
    const fetch = instalarFetchFalso(({ ruta, method }) => {
      if (ruta === '/api/v1/repositories' && method === 'GET') {
        return { status: 200, data: [repositorio({ id: 'r1' }), repositorio({ id: 'r2' })] }
      }
      if (ruta === '/api/v1/repositories/health') {
        return {
          status: 200,
          data: [
            { health: 'ok', health_checked_at: '2026-01-02T03:04:05Z', health_detail: null },
            { health: 'sin_permiso', health_checked_at: '2026-01-02T03:04:05Z', health_detail: 'faltan permisos' },
          ],
        }
      }
      throw new Error(`ruta inesperada: ${method} ${ruta}`)
    })

    const cliente = clienteDePrueba()
    const { result } = renderHook(() => useRepositoriosYComprobaciones(), { wrapper: envolver(cliente) })

    await waitFor(() => expect(result.current.listado.data).toHaveLength(2))

    await result.current.comprobarTodos.mutateAsync()

    expect(fetch.a('/api/v1/repositories/health')).toHaveLength(1)
    await waitFor(() => expect(fetch.a('/api/v1/repositories')).toHaveLength(2))
    expect(
      cliente.getQueryCache().find({ queryKey: claves.repositorios })?.state.data,
    ).toHaveLength(2)
  })
})
