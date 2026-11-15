import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import type { ManejadorDeFetch, RespuestaSimulada } from '../../tests/fetchDePrueba'
import { restauracionRegistrada } from '../../tests/fixtures'
import { elegirEnSelect, limpiarDesplegables } from '../../tests/selectDeAntd'
import {
  INTERVALO_DE_REFRESCO_MS,
  hayRestauracionesSinCerrar,
} from '../hooks/useRestauraciones'
import type { Estado, ListadoDeRestauraciones } from '../services/api/types'
import { formatearFecha } from '../services/format'
import Restauraciones, { TAMANO_DE_PAGINA } from './restauraciones'

const HEX = '0123456789abcdef'.repeat(4)

/**
 * Monta la pantalla. El histórico **no** consume el stream SSE —el estado viene
 * del listado, no de `restore_progress`—, así que no hace falta el proveedor de
 * eventos; basta con el cliente de React Query.
 */
function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <Restauraciones />
    </QueryClientProvider>,
  )
}

/** Rutas comunes: solo el listado del histórico. */
function manejadorCon(sobreRestauraciones: ManejadorDeFetch): ManejadorDeFetch {
  return (llamada) => {
    if (llamada.ruta === '/api/v1/restorations') return sobreRestauraciones(llamada)
    throw new Error(`ruta inesperada: ${llamada.method} ${llamada.ruta}`)
  }
}

function lote(cantidad: number, desde = 0) {
  return Array.from({ length: cantidad }, (_valor, indice) =>
    restauracionRegistrada({ id: `r${desde + indice}` }),
  )
}

/** El listado paginado que devuelve la página pedida, en `data`. */
function pagina(restorations: ReturnType<typeof lote>, total: number): RespuestaSimulada {
  return { status: 200, data: { restorations, total } }
}

async function esperarLaTabla() {
  return await screen.findByRole('table', { name: 'Restauraciones' })
}

beforeEach(() => {
  limpiarDesplegables()
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('pantalla de Restauraciones', () => {
  it('El histórico lista las restauraciones con su estado, target, snapshot y fechas', async () => {
    const fetch = instalarFetchFalso(
      manejadorCon(() =>
        pagina(
          [
            restauracionRegistrada({
              id: 'x1',
              target_name: 'web',
              snapshot_id: HEX,
              status: 'success',
              started_at: '2026-01-02T03:00:00Z',
              finished_at: '2026-01-02T03:05:00Z',
            }),
          ],
          1,
        ),
      ),
    )

    const { unmount } = montar()

    const tabla = await esperarLaTabla()
    const fila = within(tabla).getByText('web').closest('tr')
    if (fila === null) throw new Error('no se encontró la fila de la restauración')

    const celdas = within(fila)
    expect(celdas.getByText(HEX)).toBeInTheDocument()
    expect(celdas.getByText('Correcta')).toBeInTheDocument()
    // Las fechas se leen en la hora local del navegador: la prueba calcula el
    // mismo texto con el mismo formateador en vez de fijar una cadena.
    expect(celdas.getByText(formatearFecha('2026-01-02T03:00:00Z'))).toBeInTheDocument()
    expect(celdas.getByText(formatearFecha('2026-01-02T03:05:00Z'))).toBeInTheDocument()

    // Pide la primera página con el tope de filas.
    const primera = fetch.a('/api/v1/restorations')[0]
    expect(primera?.url).toContain(`limit=${TAMANO_DE_PAGINA}`)
    expect(primera?.url).toContain('offset=0')
    unmount()
  })

  it('El filtro por estado acota el listado y vuelve a la primera página', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(() => pagina([restauracionRegistrada({ id: 'x1', status: 'failed' })], 1)),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    await elegirEnSelect(usuario, 'Estado', 'Fallida')

    await waitFor(() => {
      const ultima = fetch.a('/api/v1/restorations').at(-1)
      expect(ultima?.url).toContain('status=failed')
      expect(ultima?.url).toContain('offset=0')
    })
    unmount()
  })

  it('Una restauración fallida muestra su motivo', async () => {
    const motivo = 'el disco de destino está lleno'
    instalarFetchFalso(
      manejadorCon(() =>
        pagina(
          [
            restauracionRegistrada({
              id: 'x1',
              status: 'failed',
              error_message: motivo,
              finished_at: '2026-01-02T03:01:00Z',
            }),
          ],
          1,
        ),
      ),
    )

    const { unmount } = montar()

    const tabla = await esperarLaTabla()
    expect(within(tabla).getByText('Fallida')).toBeInTheDocument()
    expect(within(tabla).getByText(motivo)).toBeInTheDocument()
    unmount()
  })

  it('pagina con el total: «Siguiente» se apaga en la última página', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(({ url }) => {
        const offset = Number(new URL(url, 'http://localhost').searchParams.get('offset') ?? 0)
        return offset === 0
          ? pagina(lote(TAMANO_DE_PAGINA), TAMANO_DE_PAGINA + 5)
          : pagina(lote(5, TAMANO_DE_PAGINA), TAMANO_DE_PAGINA + 5)
      }),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    const siguiente = screen.getByRole('button', { name: 'Siguiente' })
    expect(siguiente).toBeEnabled()

    await usuario.click(siguiente)

    await waitFor(() => {
      expect(
        fetch
          .a('/api/v1/restorations')
          .some((llamada) => llamada.url.includes(`offset=${TAMANO_DE_PAGINA}`)),
      ).toBe(true)
    })
    // En la última página —`offset` + filas ya no llega al total— se apaga.
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Siguiente' })).toBeDisabled()
    })
    expect(screen.getByRole('button', { name: 'Anterior' })).toBeEnabled()
    unmount()
  })

  it('sin restauraciones enseña un estado vacío, no una tabla vacía', async () => {
    instalarFetchFalso(manejadorCon(() => pagina([], 0)))

    const { unmount } = montar()

    expect(
      await screen.findByText('No hay restauraciones para estos filtros'),
    ).toBeInTheDocument()
    expect(screen.queryByRole('table', { name: 'Restauraciones' })).not.toBeInTheDocument()
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
      await screen.findByText('No se pudieron cargar las restauraciones'),
    ).toBeInTheDocument()
    expect(screen.getByText('el motor no responde')).toBeInTheDocument()

    const antes = fetch.a('/api/v1/restorations').length
    await usuario.click(screen.getByRole('button', { name: 'Reintentar' }))
    await waitFor(() => {
      expect(fetch.a('/api/v1/restorations').length).toBeGreaterThan(antes)
    })
    unmount()
  })

  it('se refresca sin recargar la página con «Actualizar»', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(() => pagina([restauracionRegistrada({ id: 'x1' })], 1)),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    const antes = fetch.a('/api/v1/restorations').length
    await usuario.click(screen.getByRole('button', { name: 'Actualizar' }))
    await waitFor(() => {
      expect(fetch.a('/api/v1/restorations').length).toBeGreaterThan(antes)
    })
    unmount()
  })

  it('mientras hay filas sin cerrar el listado se vuelve a pedir solo', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const fetch = instalarFetchFalso(
      manejadorCon(() =>
        pagina([restauracionRegistrada({ id: 'x1', status: 'running', finished_at: null })], 1),
      ),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    const antes = fetch.a('/api/v1/restorations').length

    // Pasado el intervalo, el refresco automático vuelve a pedir el listado
    // —sin recargar la página— porque hay una fila `running`.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(INTERVALO_DE_REFRESCO_MS)
    })
    await waitFor(() => {
      expect(fetch.a('/api/v1/restorations').length).toBeGreaterThan(antes)
    })
    unmount()
  })
})

describe('refresco del histórico', () => {
  function conEstados(estados: readonly Estado[]): ListadoDeRestauraciones {
    return {
      restorations: estados.map((status, indice) => restauracionRegistrada({ id: `x${indice}`, status })),
      total: estados.length,
    }
  }

  it('hayRestauracionesSinCerrar reconoce pending y running y descarta las cerradas', () => {
    expect(hayRestauracionesSinCerrar(undefined)).toBe(false)
    expect(hayRestauracionesSinCerrar(conEstados(['success', 'failed']))).toBe(false)
    expect(hayRestauracionesSinCerrar(conEstados(['success', 'pending']))).toBe(true)
    expect(hayRestauracionesSinCerrar(conEstados(['failed', 'running']))).toBe(true)
  })
})
