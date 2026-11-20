import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import { crearClienteDeConsultas } from '../services/clienteDeConsultas'
import { claves } from './claves'
import { REINTENTOS_DE_S3, useSnapshots } from './useSnapshots'

function envolver(cliente: QueryClient) {
  return function Envoltorio({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={cliente}>{children}</QueryClientProvider>
  }
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useSnapshots', () => {
  it('acota el reintento del S3: un repositorio mudo cae en error sin reintentos en bucle', async () => {
    const fetch = instalarFetchFalso(() => ({
      status: 503,
      message: 'el repositorio no responde',
    }))

    // Cliente con la política **de fábrica**: el tope del S3 tiene que ganarle.
    const cliente = new QueryClient({ defaultOptions: { queries: { gcTime: 0, staleTime: 0 } } })
    const { result } = renderHook(() => useSnapshots('r1'), { wrapper: envolver(cliente) })

    await waitFor(() => expect(result.current.isError).toBe(true), { timeout: 1500 })

    expect(cliente.getQueryCache().find({ queryKey: claves.snapshots('r1') })?.options.retry).toBe(
      REINTENTOS_DE_S3,
    )
    // Acotado de verdad: una sola tentativa, no las cuatro del `retry: 3`.
    expect(fetch.a('/api/v1/snapshots')).toHaveLength(1)
    // Y la política global no se ha tocado: el cliente sigue sin declarar `retry`.
    expect(cliente.getDefaultOptions().queries?.retry).toBeUndefined()
    expect(crearClienteDeConsultas().getDefaultOptions().queries?.retry).toBeUndefined()
  })

  it('no impone deadline: sin el corte del backend, una petición que nunca resuelve sigue cargando', async () => {
    // Caracteriza la dependencia documentada en el hook: el cliente HTTP no
    // tiene timeout propio, así que `retry: false` acota los **reintentos**, no
    // la **espera**. Una petición que nunca resuelve deja la consulta en
    // `pending`; el desenlace solo llega cuando el backend corta el destino a
    // los 10 s (`TOPE_DEL_DESTINO`) y rechaza el `fetch`. Si algún día se añade
    // un deadline de cliente, este test cambia de signo y hay que actualizar el
    // comentario de `useSnapshots`/`useEspacioEnS3`.
    instalarFetchFalso(() => new Promise<never>(() => {}))

    const cliente = new QueryClient({ defaultOptions: { queries: { gcTime: 0, staleTime: 0 } } })
    const { result } = renderHook(() => useSnapshots('r1'), { wrapper: envolver(cliente) })

    await waitFor(() => expect(result.current.fetchStatus).toBe('fetching'))
    expect(result.current.isPending).toBe(true)
    expect(result.current.isError).toBe(false)
  })

  it('propaga limit y offset y devuelve la página con total y el aviso de recorte', async () => {
    const fetch = instalarFetchFalso(() => ({
      status: 200,
      data: { snapshots: [{ id: 'a'.repeat(64) }], total: 45, truncated: true },
    }))

    const cliente = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
    })
    const { result } = renderHook(() => useSnapshots('r1', { limit: 10, offset: 10 }), {
      wrapper: envolver(cliente),
    })

    await waitFor(() => expect(result.current.isSuccess).toBe(true))

    // La paginación viaja a la API junto al repositorio.
    const peticion = fetch.a('/api/v1/snapshots')[0]
    expect(peticion?.url).toContain('repository_id=r1')
    expect(peticion?.url).toContain('limit=10')
    expect(peticion?.url).toContain('offset=10')

    // El total —ajeno a limit/offset— y el recorte los publica la respuesta.
    expect(result.current.data?.total).toBe(45)
    expect(result.current.data?.truncated).toBe(true)
  })

  it('conserva la página anterior mientras llega la siguiente, sin volver a «cargando»', async () => {
    const fetch = instalarFetchFalso((llamada) =>
      llamada.url.includes('offset=10')
        ? new Promise<never>(() => {}) // la segunda página no llega todavía
        : {
            status: 200,
            data: { snapshots: [{ id: 'a'.repeat(64) }], total: 45, truncated: false },
          },
    )

    const cliente = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
    })
    const { result, rerender } = renderHook(
      ({ offset }: { offset: number }) => useSnapshots('r1', { limit: 10, offset }),
      { wrapper: envolver(cliente), initialProps: { offset: 0 } },
    )

    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    expect(result.current.data?.snapshots[0]?.id).toBe('a'.repeat(64))

    rerender({ offset: 10 })
    await waitFor(() => expect(fetch.a('/api/v1/snapshots')).toHaveLength(2))

    expect(result.current.isPlaceholderData).toBe(true)
    expect(result.current.isPending).toBe(false)
    expect(result.current.data?.snapshots[0]?.id).toBe('a'.repeat(64))
  })
})
