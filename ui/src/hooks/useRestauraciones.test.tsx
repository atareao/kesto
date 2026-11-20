import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import { restauracionRegistrada } from '../../tests/fixtures'
import { useRestauraciones } from './useRestauraciones'

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

describe('useRestauraciones', () => {
  it('conserva la página anterior mientras llega la siguiente, sin volver a «cargando»', async () => {
    const fetch = instalarFetchFalso((llamada) =>
      llamada.url.includes('offset=10')
        ? new Promise<never>(() => {}) // la segunda página no llega todavía
        : {
            status: 200,
            data: { restorations: [restauracionRegistrada({ id: 'x1' })], total: 120 },
          },
    )

    const { result, rerender } = renderHook(
      ({ offset }: { offset: number }) => useRestauraciones({ limit: 10, offset }),
      { wrapper: envolver(cliente()), initialProps: { offset: 0 } },
    )

    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    expect(result.current.data?.restorations[0]?.id).toBe('x1')

    rerender({ offset: 10 })
    await waitFor(() => expect(fetch.a('/api/v1/restorations')).toHaveLength(2))

    expect(result.current.isPlaceholderData).toBe(true)
    expect(result.current.isPending).toBe(false)
    expect(result.current.data?.restorations[0]?.id).toBe('x1')
    expect(result.current.data?.total).toBe(120)
  })
})
