import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import type { ManejadorDeFetch, RespuestaSimulada } from '../../tests/fetchDePrueba'
import { restauracionRegistrada, target } from '../../tests/fixtures'
import { abrirSelect, elegirEnSelect, limpiarDesplegables } from '../../tests/selectDeAntd'
import {
  INTERVALO_DE_REFRESCO_MS,
  hayRestauracionesSinCerrar,
} from '../hooks/useRestauraciones'
import type { Estado, ListadoDeRestauraciones, Target } from '../services/api/types'
import { formatearFecha } from '../services/format'
import Restauraciones, { TAMANO_DE_PAGINA } from './restauraciones'

const HEX = '0123456789abcdef'.repeat(4)

/**
 * Monta la pantalla. El histórico **no** consume el stream SSE —el estado viene
 * del listado, no de `restore_progress`—, así que no hace falta el proveedor de
 * eventos; basta con el cliente de React Query. Devuelve también el cliente para
 * poder forzar un refetch —el catálogo de targets, p. ej.— desde el caso.
 */
function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  const resultado = render(
    <QueryClientProvider client={cliente}>
      <Restauraciones />
    </QueryClientProvider>,
  )
  return { ...resultado, cliente }
}

/**
 * Rutas comunes: el catálogo que etiqueta el selector de target y el listado del
 * histórico. `sobreRestauraciones` decide qué devuelve el listado.
 */
function manejadorCon(sobreRestauraciones: ManejadorDeFetch): ManejadorDeFetch {
  return (llamada) => {
    if (llamada.ruta === '/api/v1/targets') return { status: 200, data: CATALOGO }
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

/** El catálogo de targets que etiqueta el selector de target. */
const TARGET_WEB = target({ id: 't1', name: 'web' })
const TARGET_FACTURACION = target({ id: 't2', name: 'facturación' })
const CATALOGO = [TARGET_WEB, TARGET_FACTURACION]

type Usuario = ReturnType<typeof userEvent.setup>

/**
 * Elige una opción en el selector de «Target». A diferencia de los selectores de
 * opciones fijas, las suyas llegan con `GET /api/v1/targets`, así que la opción
 * hay que esperarla, no darla por hecha al abrir el desplegable.
 */
async function elegirTarget(usuario: Usuario, opcion: string): Promise<void> {
  const desplegable = await abrirSelect(usuario, 'Target')
  await usuario.click(await within(desplegable).findByTitle(opcion))
}

/** Limpia el selector de «Target» con la cruz de `allowClear`. */
async function limpiarTarget(usuario: Usuario): Promise<void> {
  const combobox = screen.getByRole('combobox', { name: 'Target' })
  const selector = combobox.closest('.ant-select')
  if (selector === null) throw new Error('no se encontró el selector de target')
  const cruz = selector.querySelector<HTMLElement>('.ant-select-clear')
  if (cruz === null) throw new Error('el selector de target no ofrece limpiar')
  await usuario.click(cruz)
}

/** El `target_id` con el que se pidió el listado, o `null` si no llevaba. */
function targetIdPedido(url: string): string | null {
  return new URL(url, 'http://localhost').searchParams.get('target_id')
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

describe('filtro por target en Restauraciones', () => {
  it('el selector de target ofrece el catálogo y acota el listado con su total', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(({ url }) =>
        targetIdPedido(url) === TARGET_FACTURACION.id
          ? pagina([restauracionRegistrada({ id: 'b1', target_id: 't2', target_name: 'facturación' })], 1)
          : pagina(lote(TAMANO_DE_PAGINA), TAMANO_DE_PAGINA * 2),
      ),
    )

    const { unmount } = montar()
    await esperarLaTabla()
    // Con las dos páginas del listado completo.
    expect(screen.getByText(/Página 1 de 2/)).toBeInTheDocument()

    await elegirTarget(usuario, 'facturación')

    await waitFor(() => {
      expect(fetch.a('/api/v1/restorations').at(-1)?.url).toContain(
        `target_id=${TARGET_FACTURACION.id}`,
      )
    })
    await waitFor(() => {
      const tabla = screen.getByRole('table', { name: 'Restauraciones' })
      expect(within(tabla).getByText('facturación')).toBeInTheDocument()
      expect(within(tabla).queryByText('web')).not.toBeInTheDocument()
    })
    // El total ya no es el del listado completo: solo hay una página.
    expect(await screen.findByText(/Página 1 de 1/)).toBeInTheDocument()
    unmount()
  })

  it('cambiar el filtro de target reinicia la paginación a la primera página', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(() => pagina(lote(TAMANO_DE_PAGINA), TAMANO_DE_PAGINA * 2 + 5)),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    await usuario.click(screen.getByRole('button', { name: 'Siguiente' }))
    await waitFor(() => {
      expect(
        fetch
          .a('/api/v1/restorations')
          .some((llamada) => llamada.url.includes(`offset=${TAMANO_DE_PAGINA}`)),
      ).toBe(true)
    })

    await elegirTarget(usuario, 'facturación')

    await waitFor(() => {
      const ultima = fetch.a('/api/v1/restorations').at(-1)
      expect(ultima?.url).toContain(`target_id=${TARGET_FACTURACION.id}`)
      expect(ultima?.url).toContain('offset=0')
    })
    unmount()
  })

  it('limpiar el selector de target pide el listado sin target_id y vuelve al completo', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(({ url }) =>
        targetIdPedido(url) === null
          ? pagina(
              [
                restauracionRegistrada({ id: 'a1', target_id: 't1', target_name: 'web' }),
                restauracionRegistrada({ id: 'b1', target_id: 't2', target_name: 'facturación' }),
              ],
              2,
            )
          : pagina(
              [restauracionRegistrada({ id: 'b1', target_id: 't2', target_name: 'facturación' })],
              1,
            ),
      ),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    await elegirTarget(usuario, 'facturación')
    await waitFor(() => {
      expect(fetch.a('/api/v1/restorations').at(-1)?.url).toContain(
        `target_id=${TARGET_FACTURACION.id}`,
      )
    })

    await limpiarTarget(usuario)

    await waitFor(() => {
      expect(fetch.a('/api/v1/restorations').at(-1)?.url).not.toContain('target_id=')
    })
    await waitFor(() => {
      const tabla = screen.getByRole('table', { name: 'Restauraciones' })
      expect(within(tabla).getByText('web')).toBeInTheDocument()
      expect(within(tabla).getByText('facturación')).toBeInTheDocument()
    })
    unmount()
  })

  it('elegir target y estado envía ambos parámetros', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(() =>
        pagina(
          [
            restauracionRegistrada({
              id: 'b1',
              target_id: 't2',
              target_name: 'facturación',
              status: 'failed',
              error_message: 'sin espacio en el destino',
            }),
          ],
          1,
        ),
      ),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    await elegirTarget(usuario, 'facturación')
    await waitFor(() => {
      expect(fetch.a('/api/v1/restorations').at(-1)?.url).toContain(
        `target_id=${TARGET_FACTURACION.id}`,
      )
    })

    await elegirEnSelect(usuario, 'Estado', 'Fallida')

    await waitFor(() => {
      const ultima = fetch.a('/api/v1/restorations').at(-1)
      expect(ultima?.url).toContain(`target_id=${TARGET_FACTURACION.id}`)
      expect(ultima?.url).toContain('status=failed')
    })
    unmount()
  })

  it('un target que desaparece del catálogo no rompe la pantalla: conserva el filtro y su vacío', async () => {
    const usuario = userEvent.setup()
    // El catálogo se puede quedar sin el target elegido —se borró en otra
    // pestaña—: la pantalla no debe reconciliar opciones ni romperse.
    let catalogo: Target[] = CATALOGO
    const fetch = instalarFetchFalso((llamada) => {
      if (llamada.ruta === '/api/v1/targets') return { status: 200, data: catalogo }
      if (llamada.ruta === '/api/v1/restorations') {
        return targetIdPedido(llamada.url) === TARGET_FACTURACION.id
          ? pagina([], 0)
          : pagina(
              [restauracionRegistrada({ id: 'a1' }), restauracionRegistrada({ id: 'a2' })],
              2,
            )
      }
      throw new Error(`ruta inesperada: ${llamada.method} ${llamada.ruta}`)
    })

    const { cliente, unmount } = montar()
    await esperarLaTabla()

    await elegirTarget(usuario, 'facturación')
    expect(
      await screen.findByText('No hay restauraciones para estos filtros'),
    ).toBeInTheDocument()

    catalogo = [TARGET_WEB]
    await cliente.refetchQueries()

    // Al refrescar sigue pidiendo su `target_id` y pinta el vacío: ni error ni
    // carga infinita.
    await usuario.click(screen.getByRole('button', { name: 'Actualizar' }))
    await waitFor(() => {
      expect(fetch.a('/api/v1/restorations').at(-1)?.url).toContain(
        `target_id=${TARGET_FACTURACION.id}`,
      )
    })
    expect(screen.queryByText('No se pudieron cargar las restauraciones')).not.toBeInTheDocument()
    expect(screen.queryByTestId('cargando')).not.toBeInTheDocument()
    expect(screen.getByText('No hay restauraciones para estos filtros')).toBeInTheDocument()
    expect(screen.queryByRole('table', { name: 'Restauraciones' })).not.toBeInTheDocument()
    unmount()
  })

  it('cambiar de target consulta de nuevo en vez de servir la caché', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(({ url }) => {
        const targetId = targetIdPedido(url)
        if (targetId === 't2')
          return pagina(
            [restauracionRegistrada({ id: 'b1', target_id: 't2', target_name: 'facturación' })],
            1,
          )
        if (targetId === 't1')
          return pagina([restauracionRegistrada({ id: 'a1', target_id: 't1', target_name: 'web' })], 1)
        return pagina(
          [
            restauracionRegistrada({ id: 'a1', target_id: 't1', target_name: 'web' }),
            restauracionRegistrada({ id: 'b1', target_id: 't2', target_name: 'facturación' }),
          ],
          2,
        )
      }),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    await elegirTarget(usuario, 'facturación')
    await waitFor(() => {
      const tabla = screen.getByRole('table', { name: 'Restauraciones' })
      expect(within(tabla).getByText('facturación')).toBeInTheDocument()
      expect(within(tabla).queryByText('web')).not.toBeInTheDocument()
    })

    await elegirTarget(usuario, 'web')
    await waitFor(() => {
      expect(fetch.a('/api/v1/restorations').at(-1)?.url).toContain('target_id=t1')
    })
    await waitFor(() => {
      const tabla = screen.getByRole('table', { name: 'Restauraciones' })
      expect(within(tabla).getByText('web')).toBeInTheDocument()
      expect(within(tabla).queryByText('facturación')).not.toBeInTheDocument()
    })
    unmount()
  })
})
