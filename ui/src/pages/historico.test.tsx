import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  EventSourceFalso,
  eventoDePrueba,
  instalarEventSourceFalso,
} from '../../tests/eventosDePrueba'
import { ProveedorDeEventos } from '../services/events'
import { NOMBRE_DEL_EVENTO } from '../services/useEventStream'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import type { ManejadorDeFetch, RespuestaSimulada } from '../../tests/fetchDePrueba'
import { ejecucion, tarea } from '../../tests/fixtures'
import {
  TAMANO_DE_PAGINA_POR_DEFECTO as TAMANO,
  TEXTO_DE_PAGINA_VACIA,
} from '../components/Paginador/Paginador'
import Historico from './historico'

type Usuario = ReturnType<typeof userEvent.setup>

const J1 = tarea({
  id: 'j1',
  target_id: 't1',
  repository_id: 'r1',
  name: 'copia nocturna',
})
const TAREAS = [J1]

function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return {
    ...render(
      <QueryClientProvider client={cliente}>
        <ProveedorDeEventos>
          <Historico />
        </ProveedorDeEventos>
      </QueryClientProvider>,
    ),
    cliente,
  }
}

function lote(cantidad: number, desde = 0) {
  return Array.from({ length: cantidad }, (_valor, indice) =>
    ejecucion({ id: `e${desde + indice}`, job_id: 'j1', status: 'success' }),
  )
}

/**
 * La página que devuelve el servidor: las filas de `lote` envueltas en el
 * listado con su `total`, que es de donde sale el número de páginas del
 * paginador. El panel **no** recorta en cliente, así que el total es dato de la
 * respuesta, no el tamaño de la lista.
 */
function sobre(executions: ReturnType<typeof lote>, total: number): RespuestaSimulada {
  return { status: 200, data: { executions, total } }
}

/** Rutas comunes a todas las pruebas: las tareas que etiquetan el filtro. */
function manejadorCon(sobreEjecuciones: ManejadorDeFetch): ManejadorDeFetch {
  return (llamada) => {
    if (llamada.ruta === '/api/v1/jobs') return { status: 200, data: TAREAS }
    if (llamada.ruta === '/api/v1/executions') return sobreEjecuciones(llamada)
    throw new Error(`ruta inesperada: ${llamada.method} ${llamada.ruta}`)
  }
}

async function elegir(usuario: Usuario, etiqueta: string, opcion: string) {
  await usuario.click(screen.getByRole('combobox', { name: etiqueta }))
  await usuario.click(await screen.findByTitle(opcion))
}

beforeEach(() => {
  instalarEventSourceFalso()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('pantalla de Histórico', () => {
  it('el paginador se alimenta del total y pide la página al servidor', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(() => sobre(lote(TAMANO), 120)))

    const { unmount } = montar()
    await screen.findByRole('table', { name: 'Ejecuciones' })

    // El total sale de la API: 120 ejecuciones a 10 por página son 12 páginas.
    const primera = fetch.a('/api/v1/executions')[0]
    expect(primera?.url).toContain(`limit=${TAMANO}`)
    expect(primera?.url).toContain('offset=0')
    expect(screen.getByText('120 elementos')).toBeInTheDocument()
    expect(screen.getByTitle('12')).toBeInTheDocument()

    // Cambiar de página pide **esa** página al servidor, no recorta en cliente.
    await usuario.click(screen.getByTitle('3'))
    await waitFor(() => {
      expect(
        fetch.a('/api/v1/executions').some((llamada) => llamada.url.includes('offset=20')),
      ).toBe(true)
    })
    unmount()
  })

  it('cambiar el tamaño de página pide la primera con el nuevo limit', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(() => sobre(lote(TAMANO), 120)))

    const { unmount } = montar()
    await screen.findByRole('table', { name: 'Ejecuciones' })

    await usuario.click(screen.getByTitle('3'))
    await waitFor(() => {
      expect(
        fetch.a('/api/v1/executions').some((llamada) => llamada.url.includes('offset=20')),
      ).toBe(true)
    })

    await usuario.click(screen.getByRole('combobox', { name: 'Filas por página' }))
    await usuario.click(await screen.findByTitle('50 por página'))

    await waitFor(() => {
      const ultima = fetch.a('/api/v1/executions').at(-1)
      expect(ultima?.url).toContain('limit=50')
      // Otro tamaño es otra rejilla: se vuelve a la primera, no se hereda la 3.
      expect(ultima?.url).toContain('offset=0')
    })
    unmount()
  })

  it('los filtros viajan en la consulta y vuelven a la primera página', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(() => sobre(lote(3), 25)))

    const { unmount } = montar()
    await screen.findByRole('table', { name: 'Ejecuciones' })

    // Primero se sale de la primera página: sin ese `offset` heredado, exigir
    // `offset=0` después no demostraría que el filtro reinicia la paginación.
    await usuario.click(screen.getByTitle('2'))
    await waitFor(() => {
      expect(
        fetch.a('/api/v1/executions').some((llamada) => llamada.url.includes('offset=10')),
      ).toBe(true)
    })

    await elegir(usuario, 'Tarea', 'copia nocturna')
    await waitFor(() => {
      expect(fetch.a('/api/v1/executions').at(-1)?.url).toContain('job_id=j1')
    })

    await elegir(usuario, 'Estado', 'Fallida')
    await waitFor(() => {
      const ultima = fetch.a('/api/v1/executions').at(-1)
      expect(ultima?.url).toContain('status=failed')
      expect(ultima?.url).toContain('offset=0')
    })
    unmount()
  })

  it('la consola solo recibe los eventos de la ejecución elegida y avisa si el stream cae', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(
      manejadorCon(() =>
        sobre(
          [ejecucion({ id: 'e1', job_id: 'j1' }), ejecucion({ id: 'e2', job_id: 'j1' })],
          2,
        ),
      ),
    )

    const { unmount } = montar()
    await screen.findByRole('table', { name: 'Ejecuciones' })

    await usuario.click(screen.getAllByRole('button', { name: 'Ver consola' })[0] as HTMLElement)

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e2', current_action: 'otra-ejecucion' }),
    )
    expect(screen.queryByText(/otra-ejecucion/)).not.toBeInTheDocument()

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e1', current_action: 'subiendo-datos' }),
    )
    expect(screen.getByText(/subiendo-datos/)).toBeInTheDocument()

    EventSourceFalso.ultima().emitir('error')
    expect(
      screen.getByText(/se ha perdido la conexión con el servidor/i),
    ).toBeInTheDocument()
    unmount()
  })

  it('al refrescar, la consola de una ejecución que pasa a fallida muestra su error_message', async () => {
    const usuario = userEvent.setup()
    let terminada = false
    instalarFetchFalso(
      manejadorCon(() =>
        sobre(
          [
            terminada
              ? ejecucion({
                  id: 'e1',
                  job_id: 'j1',
                  status: 'failed',
                  error_message: 'el repositorio no responde',
                  finished_at: '2026-01-02T03:05:00Z',
                })
              : ejecucion({ id: 'e1', job_id: 'j1', status: 'running', finished_at: null }),
          ],
          1,
        ),
      ),
    )

    const { cliente, unmount } = montar()
    await screen.findByRole('table', { name: 'Ejecuciones' })

    await usuario.click(screen.getAllByRole('button', { name: 'Ver consola' })[0] as HTMLElement)
    expect(screen.getByText(/todavía no hay eventos/i)).toBeInTheDocument()

    // La ejecución termina y el refresco trae la fila nueva: la consola debe
    // pintar el resumen con el `error_message`, no quedarse con la foto vieja.
    terminada = true
    await cliente.invalidateQueries()
    expect(await screen.findByText('el repositorio no responde')).toBeInTheDocument()
    unmount()
  })

  it('sin ejecuciones enseña un estado vacío, no una tabla vacía', async () => {
    instalarFetchFalso(manejadorCon(() => sobre([], 0)))

    const { unmount } = montar()

    expect(
      await screen.findByText('No hay ejecuciones para estos filtros'),
    ).toBeInTheDocument()
    expect(screen.queryByRole('table', { name: 'Ejecuciones' })).not.toBeInTheDocument()
    unmount()
  })

  it('si la retención purga el listado estando en una página alta, el paginador sigue ahí para volver a la primera', async () => {
    const usuario = userEvent.setup()
    let purgado = false
    const fetch = instalarFetchFalso(
      manejadorCon(() => (purgado ? sobre([], 0) : sobre(lote(TAMANO), 120))),
    )

    const { cliente, unmount } = montar()
    await screen.findByRole('table', { name: 'Ejecuciones' })

    // El operador se va a la página 2: eso pide **su** `offset` al servidor.
    await usuario.click(screen.getByTitle('2'))
    await waitFor(() => {
      expect(
        fetch.a('/api/v1/executions').some((llamada) => llamada.url.includes(`offset=${TAMANO}`)),
      ).toBe(true)
    })

    // La purga/retención vacía el conjunto mientras sigue ahí: el `total` cae a
    // cero y la página se queda sin filas, pero la actual sigue siendo la 2.
    purgado = true
    await cliente.invalidateQueries()
    expect(await screen.findByText(TEXTO_DE_PAGINA_VACIA)).toBeInTheDocument()

    // Sin `total > 0`, ¿dónde queda el control de vuelta? El paginador se pinta
    // igual porque hay una página que abandonar; sin él, el operador quedaría
    // encerrado en una página vacía.
    await usuario.click(screen.getByTitle('1'))
    await waitFor(() => {
      expect(fetch.a('/api/v1/executions').at(-1)?.url).toContain('offset=0')
    })
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
      await screen.findByText('No se pudieron cargar las ejecuciones'),
    ).toBeInTheDocument()
    expect(screen.getByText('el motor no responde')).toBeInTheDocument()

    const antes = fetch.a('/api/v1/executions').length
    await usuario.click(screen.getByRole('button', { name: 'Reintentar' }))
    await waitFor(() => {
      expect(fetch.a('/api/v1/executions').length).toBeGreaterThan(antes)
    })
    unmount()
  })
})

describe('tabla de ejecuciones', () => {
  it('pinta la duración de las terminadas y «en curso» para las vivas', async () => {
    instalarFetchFalso(
      manejadorCon(() =>
        sobre(
          [
            ejecucion({
              id: 'e1',
              job_id: 'j1',
              status: 'success',
              started_at: '2026-01-02T03:00:00Z',
              finished_at: '2026-01-02T03:05:00Z',
            }),
            ejecucion({
              id: 'e2',
              job_id: 'j1',
              status: 'running',
              started_at: '2026-01-02T03:00:00Z',
              finished_at: null,
            }),
          ],
          2,
        ),
      ),
    )

    const { unmount } = montar()

    const tabla = await screen.findByRole('table', { name: 'Ejecuciones' })
    expect(within(tabla).getByText('5 min')).toBeInTheDocument()
    expect(within(tabla).getByText('en curso')).toBeInTheDocument()
    unmount()
  })
})

describe('nombre de la tarea en el Histórico', () => {
  it('el filtro «Tarea» etiqueta las opciones con el nombre de la tarea, no con el del target', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(() => sobre(lote(2), 2)))

    const { unmount } = montar()
    await screen.findByRole('table', { name: 'Ejecuciones' })

    await usuario.click(screen.getByRole('combobox', { name: 'Tarea' }))

    // La opción se lee por el nombre de la tarea; el nombre del target es
    // justo lo que hacía indistinguibles dos tareas sobre el mismo target.
    expect(await screen.findByTitle('copia nocturna')).toBeInTheDocument()
    expect(screen.queryByTitle('web')).not.toBeInTheDocument()

    await usuario.click(screen.getByTitle('copia nocturna'))
    await waitFor(() => {
      expect(fetch.a('/api/v1/executions').at(-1)?.url).toContain('job_id=j1')
    })
    unmount()
  })

  it('la tabla muestra una columna «Tarea» con el `job_name` y el identificador cuando no hay nombre', async () => {
    instalarFetchFalso(
      manejadorCon(() =>
        sobre(
          [
            ejecucion({ id: 'e1', job_id: 'j1', job_name: 'copia nocturna' }),
            ejecucion({ id: 'e2', job_id: 'j2', job_name: null }),
          ],
          2,
        ),
      ),
    )

    const { unmount } = montar()

    const tabla = await screen.findByRole('table', { name: 'Ejecuciones' })

    const encabezados = within(tabla)
      .getAllByRole('columnheader')
      .map((celda) => (celda.textContent ?? '').trim())
    const columna = encabezados.indexOf('Tarea')
    expect(columna).toBeGreaterThanOrEqual(0)

    /** Las celdas de la fila `indice`, en el orden en que llegan del listado. */
    const celdasDe = (indice: number) => {
      const boton = screen.getAllByRole('button', { name: 'Ver consola' })[indice]
      const fila = boton.closest('tr')
      if (fila === null) throw new Error(`no se encontró la fila ${indice}`)
      return within(fila).getAllByRole('cell')
    }

    expect(celdasDe(0)[columna]?.textContent).toBe('copia nocturna')
    // Sin nombre cae al identificador de la tarea, nunca al del target.
    expect(celdasDe(1)[columna]?.textContent).toBe('j2')
    expect(within(tabla).queryByText('web')).not.toBeInTheDocument()
    unmount()
  })
})
