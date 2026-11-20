import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import { ejecucion } from '../../tests/fixtures'
import { useEjecuciones } from './useEjecuciones'

function envolver(cliente: QueryClient) {
  return function Envoltorio({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={cliente}>{children}</QueryClientProvider>
  }
}

/** Cliente sin reintentos: un fallo es un desenlace, no una espera. */
function cliente() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useEjecuciones', () => {
  it('propaga limit y offset y devuelve la página con su total', async () => {
    const fetch = instalarFetchFalso(() => ({
      status: 200,
      data: { executions: [ejecucion({ id: 'e1', job_id: 'j1' })], total: 120 },
    }))

    const { result } = renderHook(() => useEjecuciones({ limit: 10, offset: 20 }), {
      wrapper: envolver(cliente()),
    })

    await waitFor(() => expect(result.current.isSuccess).toBe(true))

    // La paginación viaja a la API: es el contrato de la petición.
    const peticion = fetch.a('/api/v1/executions')[0]
    expect(peticion?.url).toContain('limit=10')
    expect(peticion?.url).toContain('offset=20')

    // Y el total —ajeno a limit/offset— es lo que alimenta el paginador.
    expect(result.current.data?.total).toBe(120)
    expect(result.current.data?.executions).toHaveLength(1)
  })

  it('dos páginas distintas piden al servidor por separado', async () => {
    const fetch = instalarFetchFalso(() => ({
      status: 200,
      data: { executions: [], total: 5 },
    }))

    // Mismo cliente: si las dos páginas compartieran clave, React Query
    // deduplicaría y solo habría una petición.
    const compartido = cliente()
    const primera = renderHook(() => useEjecuciones({ limit: 10, offset: 0 }), {
      wrapper: envolver(compartido),
    })
    const segunda = renderHook(() => useEjecuciones({ limit: 10, offset: 10 }), {
      wrapper: envolver(compartido),
    })

    await waitFor(() => expect(primera.result.current.isSuccess).toBe(true))
    await waitFor(() => expect(segunda.result.current.isSuccess).toBe(true))

    expect(fetch.a('/api/v1/executions')).toHaveLength(2)
  })

  it('conserva la página anterior mientras llega la siguiente, sin volver a «cargando»', async () => {
    const fetch = instalarFetchFalso((llamada) =>
      llamada.url.includes('offset=10')
        ? new Promise<never>(() => {}) // la segunda página no llega todavía
        : {
            status: 200,
            data: { executions: [ejecucion({ id: 'e1', job_id: 'j1' })], total: 120 },
          },
    )

    const { result, rerender } = renderHook(
      ({ offset }: { offset: number }) => useEjecuciones({ limit: 10, offset }),
      { wrapper: envolver(cliente()), initialProps: { offset: 0 } },
    )

    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    expect(result.current.data?.executions[0]?.id).toBe('e1')

    rerender({ offset: 10 })
    await waitFor(() => expect(fetch.a('/api/v1/executions')).toHaveLength(2))

    expect(result.current.isPlaceholderData).toBe(true)
    expect(result.current.isPending).toBe(false)
    expect(result.current.data?.executions[0]?.id).toBe('e1')
    expect(result.current.data?.total).toBe(120)
  })
})
