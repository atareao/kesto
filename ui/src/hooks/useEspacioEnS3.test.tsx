import type { ReactNode } from 'react'
import { act, renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import { repositorio, snapshot } from '../../tests/fixtures'
import { crearClienteDeConsultas } from '../services/clienteDeConsultas'
import { claves } from './claves'
import { REINTENTOS_DE_S3 } from './useSnapshots'
import { useEspacioEnS3 } from './useEspacioEnS3'

/**
 * Un cliente con la política **de fábrica** de React Query —`retry: 3` incluido—
 * para que la prueba demuestre que el tope vive en **cada consulta del S3** y no
 * en el cliente: con el cliente recortado, el recorte del hook quedaría tapado.
 */
function clienteDeFabrica(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { gcTime: 0, staleTime: 0 } } })
}

function envolver(cliente: QueryClient) {
  return function Envoltorio({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={cliente}>{children}</QueryClientProvider>
  }
}

/** La lista de instantáneas de un repositorio, con los bytes que se le digan. */
function listado(bytes: number, id: string) {
  return { status: 200, data: { snapshots: [snapshot({ id, bytes })], total: 1, truncated: false } }
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useEspacioEnS3', () => {
  it('con un repositorio caído conserva lo del sano, lo nombra y declara un mínimo', async () => {
    instalarFetchFalso(({ ruta, url }) => {
      if (ruta !== '/api/v1/snapshots') throw new Error(`ruta inesperada: ${ruta}`)
      const id = new URL(url, 'http://localhost').searchParams.get('repository_id')
      if (id === 'caido') return { status: 503, message: 'el repositorio no responde' }
      return listado(2048, 's1')
    })

    const cliente = clienteDeFabrica()
    const { result } = renderHook(
      () => useEspacioEnS3([repositorio({ id: 'sano', name: 'sano' }), repositorio({ id: 'caido', name: 'caido' })]),
      { wrapper: envolver(cliente) },
    )

    await waitFor(() => expect(result.current.repositoriosCaidos).toEqual(['caido']))

    expect(result.current.esMinimo).toBe(true)
    expect(result.current.bytes).toBe(2048)
    expect(result.current.cargando).toBe(false)

    // El recorte va **en la consulta** que depende del S3: una sola tentativa,
    // no las cuatro del `retry: 3` de fábrica. La clave lleva el `limit: 200`
    // del `queryFn`, así que se busca con ese filtro.
    expect(
      cliente.getQueryCache().find({ queryKey: claves.snapshots('caido', { limit: 200 }) })?.options
        .retry,
    ).toBe(REINTENTOS_DE_S3)
    // Y el sano sí resolvió: su información no se pierde con el fallo del otro.
    expect(
      cliente.getQueryCache().find({ queryKey: claves.snapshots('sano', { limit: 200 }) })?.state
        .data,
    ).toBeDefined()
  })

  it('con todos los repositorios sanos no hay caídos y la suma es un total', async () => {
    instalarFetchFalso(({ ruta, url }) => {
      if (ruta !== '/api/v1/snapshots') throw new Error(`ruta inesperada: ${ruta}`)
      const id = new URL(url, 'http://localhost').searchParams.get('repository_id')
      return listado(id === 'r1' ? 1024 : 2048, `s-${id}`)
    })

    const cliente = clienteDeFabrica()
    const { result } = renderHook(
      () => useEspacioEnS3([repositorio({ id: 'r1' }), repositorio({ id: 'r2' })]),
      { wrapper: envolver(cliente) },
    )

    await waitFor(() => expect(result.current.bytes).toBe(3072))

    expect(result.current.repositoriosCaidos).toEqual([])
    expect(result.current.esMinimo).toBe(false)
    expect(result.current.cargando).toBe(false)
  })

  it('un listado que no cubre el total basta para que la cifra sea un mínimo, sin caídos', async () => {
    instalarFetchFalso(() => ({
      status: 200,
      data: { snapshots: [snapshot({ id: 's1', bytes: 1024 })], total: 500, truncated: false },
    }))

    const cliente = clienteDeFabrica()
    const { result } = renderHook(() => useEspacioEnS3([repositorio({ id: 'r1' })]), {
      wrapper: envolver(cliente),
    })

    await waitFor(() => expect(result.current.bytes).toBe(1024))

    // Se leyeron menos instantáneas que las que el repositorio tiene: la suma es
    // parcial. Ni se piden más páginas ni se recorta en cliente: se declara el
    // mínimo con `snapshots.length < total`.
    expect(result.current.esMinimo).toBe(true)
    expect(result.current.repositoriosCaidos).toEqual([])
  })

  it('un listado truncado con la página completa también declara la cifra como mínimo', async () => {
    instalarFetchFalso(() => ({
      status: 200,
      // La página cabe entera —`snapshots.length === total`— pero el backend
      // declara que hay más: `truncated` es la señal explícita. Sin mirarla, la
      // suma se presentaría como total y estaría mintiendo.
      data: { snapshots: [snapshot({ id: 's1', bytes: 1024 })], total: 1, truncated: true },
    }))

    const cliente = clienteDeFabrica()
    const { result } = renderHook(() => useEspacioEnS3([repositorio({ id: 'r1' })]), {
      wrapper: envolver(cliente),
    })

    await waitFor(() => expect(result.current.bytes).toBe(1024))

    expect(result.current.esMinimo).toBe(true)
    expect(result.current.repositoriosCaidos).toEqual([])
  })

  it('reintentar invalida la clave con su limit: 200 (prefijo de array)', async () => {
    const fetch = instalarFetchFalso(() => listado(1024, 's1'))

    const cliente = clienteDeFabrica()
    const { result } = renderHook(() => useEspacioEnS3([repositorio({ id: 'r1' })]), {
      wrapper: envolver(cliente),
    })

    await waitFor(() => expect(result.current.bytes).toBe(1024))
    expect(
      cliente.getQueryCache().find({ queryKey: claves.snapshots('r1', { limit: 200 }) }),
    ).toBeDefined()
    const antes = fetch.a('/api/v1/snapshots').length

    // El `reintentar` invalida por prefijo `['snapshots', id]`, que alcanza la
    // clave completa de aquí (`['snapshots', id, { limit: 200 }]`).
    act(() => result.current.reintentar())

    await waitFor(() => expect(fetch.a('/api/v1/snapshots').length).toBeGreaterThan(antes))
  })

  it('no cambia la política de reintento global del cliente', () => {
    // La puerta del 401 no declara `retry`: el recorte del S3 es cosa de cada
    // consulta, no del cliente entero.
    expect(crearClienteDeConsultas().getDefaultOptions().queries?.retry).toBeUndefined()
  })
})
