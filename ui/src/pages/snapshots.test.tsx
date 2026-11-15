import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import type { ManejadorDeFetch, RespuestaSimulada } from '../../tests/fetchDePrueba'
import {
  EventSourceFalso,
  conexiones,
  exigirTodoCerrado,
  instalarEventSourceFalso,
} from '../../tests/eventosDePrueba'
import { repositorio, snapshot } from '../../tests/fixtures'
import { ProveedorDeEventos } from '../services/events'
import { NOMBRE_DEL_EVENTO_DE_RESTAURACION } from '../services/useEventStream'
import type { Snapshot } from '../services/api/types'
import Snapshots, { PLAZO_DE_CONFIRMACION_MS } from './snapshots'

const HEX = '0123456789abcdef'.repeat(4)
const OTRO_HEX = 'fedcba9876543210'.repeat(4)
const REPOS = [repositorio({ id: 'r1', name: 's3' }), repositorio({ id: 'r2', name: 'glacier' })]

/**
 * Monta la pantalla **con** el proveedor del stream: la ruta SSE de la
 * restauración es parte de lo que se prueba —la fase que refleja el panel llega
 * por ahí—, así que la pantalla aislada no la ejercería.
 */
function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <ProveedorDeEventos>
        <Snapshots />
      </ProveedorDeEventos>
    </QueryClientProvider>,
  )
}

/** `POST /api/v1/snapshots/{id}/restore` (§specs/api-resources). */
const RUTA_DE_RESTAURACION = /^\/api\/v1\/snapshots\/[^/]+\/restore$/

function manejadorCon(
  sobreSnapshots: ManejadorDeFetch,
  repos = REPOS,
  // La restauración se deja opcional: las pruebas que no la ejercitan no
  // esperan esa ruta y avisan si aparece.
  sobreRestauracion?: ManejadorDeFetch,
): ManejadorDeFetch {
  return (llamada) => {
    if (llamada.ruta === '/api/v1/repositories') return { status: 200, data: repos }
    if (llamada.ruta === '/api/v1/snapshots') return sobreSnapshots(llamada)
    if (RUTA_DE_RESTAURACION.test(llamada.ruta)) {
      if (sobreRestauracion === undefined) {
        throw new Error(`la restauración no se esperaba: ${llamada.method} ${llamada.ruta}`)
      }
      return sobreRestauracion(llamada)
    }
    throw new Error(`ruta inesperada: ${llamada.method} ${llamada.ruta}`)
  }
}

/** La tabla del listado, que es la señal de que la pantalla ya tiene datos. */
function esperarLaTabla() {
  return screen.findByRole('table', { name: 'Snapshots del repositorio' })
}

beforeEach(() => {
  vi.unstubAllGlobals()
  instalarEventSourceFalso()
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('pantalla de Snapshots', () => {
  it('lista las instantáneas del primer repositorio, con el hex completo y el aviso de recorte', async () => {
    const fetch = instalarFetchFalso(
      manejadorCon(() => ({
        status: 200,
        data: {
          snapshots: [snapshot({ id: HEX, label: null, files: 12, bytes: 2048 })],
          truncated: true,
        },
      })),
    )

    const { unmount } = montar()

    const tabla = await esperarLaTabla()
    expect(screen.getByText(HEX)).toBeInTheDocument()
    expect(screen.getByText('sin etiqueta')).toBeInTheDocument()
    expect(screen.getByText(/puede haber más instantáneas/i)).toBeInTheDocument()
    expect(screen.getByText('2 KiB')).toBeInTheDocument()

    expect(fetch.a('/api/v1/snapshots')[0]?.url).toContain('repository_id=r1')
    // No existe endpoint de purga: el panel no puede ofrecer el botón.
    expect(screen.queryByRole('button', { name: /purg/i })).not.toBeInTheDocument()
    expect(tabla).toBeInTheDocument()
    unmount()
  })

  it('al cambiar de repositorio pide sus instantáneas', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(({ url }) => {
        const id = new URL(url, 'http://localhost').searchParams.get('repository_id')
        return {
          status: 200,
          data: {
            snapshots: [snapshot({ id: id === 'r2' ? 'b'.repeat(64) : HEX })],
            truncated: false,
          },
        }
      }),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    await usuario.click(screen.getByRole('combobox', { name: 'Repositorio' }))
    await usuario.click(await screen.findByTitle('glacier'))

    await waitFor(() => {
      expect(
        fetch.a('/api/v1/snapshots').some((llamada) => llamada.url.includes('repository_id=r2')),
      ).toBe(true)
    })
    unmount()
  })

  it('un repositorio sin instantáneas enseña un estado vacío', async () => {
    instalarFetchFalso(
      manejadorCon(() => ({
        status: 200,
        data: { snapshots: [], truncated: false },
      })),
    )

    const { unmount } = montar()

    expect(
      await screen.findByText('No hay instantáneas en este repositorio'),
    ).toBeInTheDocument()
    expect(
      screen.queryByRole('table', { name: 'Snapshots del repositorio' }),
    ).not.toBeInTheDocument()
    unmount()
  })

  it('sin repositorios enseña un estado vacío con su explicación', async () => {
    instalarFetchFalso(manejadorCon(() => ({ status: 200, data: { snapshots: [], truncated: false } }), []))

    const { unmount } = montar()

    expect(await screen.findByText('No hay repositorios configurados')).toBeInTheDocument()
    unmount()
  })

  it('mientras carga enseña un indicador visible', async () => {
    instalarFetchFalso(manejadorCon(() => new Promise<RespuestaSimulada>(() => {})))
    // El manejador de snapshots no se llega a usar: repositorios también espera.
    const { unmount } = montar()

    expect(await screen.findByTestId('cargando')).toBeInTheDocument()
    unmount()
  })

  it('un error de carga se explica y se puede reintentar', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(() => ({ status: 500, message: 'el repositorio no responde' })),
    )

    const { unmount } = montar()

    expect(
      await screen.findByText('No se pudieron cargar las instantáneas'),
    ).toBeInTheDocument()
    expect(screen.getByText('el repositorio no responde')).toBeInTheDocument()

    const antes = fetch.a('/api/v1/snapshots').length
    await usuario.click(screen.getByRole('button', { name: 'Reintentar' }))
    await waitFor(() => {
      expect(fetch.a('/api/v1/snapshots').length).toBeGreaterThan(antes)
    })
    unmount()
  })
})

/**
 * Restaurar un snapshot (spec `ui`, requirement «El panel ofrece restaurar un
 * snapshot con confirmación y progreso»).
 *
 * El contrato que se fija aquí: la fila ofrece una acción que **nombra su
 * instantánea**, la confirmación nombra el **target** sobre el que se restaura
 * (de ahí la petición no sale hasta confirmar), la acción no se ofrece cuando el
 * target no se conoce o no es de directorio, el avance llega por SSE sin
 * recargar el listado y el resultado se anuncia **al cerrar la fase**, no al
 * aceptar el `POST`.
 *
 * Y tres cosas más que cierran los agujeros de seguirlo todo con una única
 * operación en vuelo: **una restauración cada vez** —la segunda no se lanza, y
 * las demás filas lo explican—, el **plazo de confirmación** —si el desenlace no
 * llega, el aviso deja de afirmar que sigue en curso y se puede cerrar— y la
 * **normalización** de una fase que el panel no conozca.
 */
describe('pantalla de Snapshots — restaurar', () => {
  const RUTA = `/api/v1/snapshots/${HEX}/restore`

  /** El listado con una sola instantánea; `campos` decide si es restaurable. */
  const listadoConUna = (campos: Partial<Snapshot> = {}): ManejadorDeFetch => () => ({
    status: 200,
    data: { snapshots: [snapshot({ id: HEX, ...campos })], truncated: false },
  })

  /** El listado con dos instantáneas restaurables, para probar la exclusión. */
  const listadoConDos = (): ManejadorDeFetch => () => ({
    status: 200,
    data: { snapshots: [snapshot({ id: HEX }), snapshot({ id: OTRO_HEX })], truncated: false },
  })

  const restauracionAceptada = {
    status: 200,
    message: 'restauración lanzada',
    data: { execution_id: 'e1', snapshot_id: HEX, target_id: 't1', target_name: 'web' },
  }

  /**
   * El `data` literal de `restore_progress`: siete campos y los cuatro
   * contadores a `0`, que es lo que el motor publica en esta versión.
   */
  function eventoDeRestauracion(campos: Record<string, unknown> = {}): Record<string, unknown> {
    return {
      execution_id: 'e1',
      snapshot_id: HEX,
      phase: 'restaurando',
      files_done: 0,
      files_total: 0,
      bytes_done: 0,
      bytes_total: 0,
      ...campos,
    }
  }

  /**
   * El botón de la fila. Su nombre accesible **nombra la instantánea**, porque
   * «Restaurar» a secas se repetiría en cada fila sin decir cuál.
   */
  function botonDeFila(etiqueta: string = HEX) {
    return screen.findByRole('button', { name: `Restaurar ${etiqueta}` })
  }

  /**
   * Pulsa el botón de la fila y espera a la **confirmación**: Antd la pinta como
   * `Popconfirm` o como `Modal.confirm` según la elección del panel.
   */
  async function abrirConfirmacion(usuario: ReturnType<typeof userEvent.setup>) {
    await usuario.click(await botonDeFila())
    return await waitFor(() => {
      const popup =
        document.querySelector<HTMLElement>('.ant-popconfirm') ??
        document.querySelector<HTMLElement>('.ant-modal-confirm')
      if (popup === null) throw new Error('no apareció la confirmación de restauración')
      return popup
    })
  }

  function confirmar(confirmacion: HTMLElement) {
    return within(confirmacion).getByRole('button', {
      name: /restaurar|confirmar|aceptar|sí|ok/i,
    })
  }

  /** Confirma y espera a que el `POST` haya salido. */
  async function restaurar(
    usuario: ReturnType<typeof userEvent.setup>,
    fetch: ReturnType<typeof instalarFetchFalso>,
  ) {
    const confirmacion = await abrirConfirmacion(usuario)
    await usuario.click(confirmar(confirmacion))
    await waitFor(() => {
      expect(fetch.a(RUTA)).toHaveLength(1)
    })
  }

  it('la confirmación nombra el target y no lanza la restauración hasta confirmarla', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(listadoConUna(), REPOS, () => restauracionAceptada))

    const { unmount } = montar()
    await esperarLaTabla()

    const confirmacion = await abrirConfirmacion(usuario)

    // Pedir confirmación con el destino delante es lo que evita restaurar en el
    // sitio equivocado: ni el nombre ni el tipo del target pueden faltar.
    expect(confirmacion).toHaveTextContent('web')
    expect(confirmacion).toHaveTextContent('directory')
    expect(fetch.a(RUTA)).toHaveLength(0)
    unmount()
  })

  it('al confirmar, restaura con el repository_id y muestra la fase en curso, sin resultado todavía', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(listadoConUna(), REPOS, () => restauracionAceptada))

    const { unmount } = montar()
    await esperarLaTabla()
    await restaurar(usuario, fetch)

    const peticion = fetch.a(RUTA)[0]
    expect(peticion?.method).toBe('POST')
    expect(peticion?.url).toContain(`/api/v1/snapshots/${HEX}/restore`)
    expect(peticion?.cuerpo).toMatchObject({ repository_id: 'r1' })

    // El `POST` solo **acepta** el trabajo: el resultado llega por SSE, así que
    // aquí no puede anunciarse nada. El aviso es el de la fase en curso, con el
    // target de origen del listado.
    const progreso = await screen.findByTestId('progreso-de-la-restauracion')
    expect(progreso).toHaveTextContent(/Restaurando «web» — en curso/)
    expect(screen.queryByTestId('resultado-de-la-restauracion')).not.toBeInTheDocument()
    unmount()
  })

  it('refleja la fase que llega por SSE sin recargar y termina con el desenlace', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(listadoConUna(), REPOS, () => restauracionAceptada))

    const { unmount } = montar()
    await esperarLaTabla()

    // La conexión ya está viva **antes** del `POST`: el stream no guarda replay,
    // así que el primer aviso de fase se perdería si se abriera después.
    expect(conexiones()).toHaveLength(1)

    await restaurar(usuario, fetch)

    const conexion = EventSourceFalso.ultima()
    conexion.emitir(NOMBRE_DEL_EVENTO_DE_RESTAURACION, eventoDeRestauracion())

    expect(screen.getByTestId('progreso-de-la-restauracion')).toHaveTextContent(
      /Restaurando «web» — en curso/,
    )
    // Los contadores del evento van a `0` por diseño: no hay porcentaje ni barra
    // que pintar, solo la fase.
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument()
    expect(screen.queryByText(/%/)).not.toBeInTheDocument()
    // El progreso llegó por el stream: el listado no se ha vuelto a pedir.
    expect(fetch.a('/api/v1/snapshots')).toHaveLength(1)

    conexion.emitir(NOMBRE_DEL_EVENTO_DE_RESTAURACION, eventoDeRestauracion({ phase: 'terminada' }))

    const resultado = await screen.findByTestId('resultado-de-la-restauracion')
    expect(resultado).toHaveTextContent('Restauración terminada sobre «web»')
    // La fase terminal cierra el aviso de progreso: no conviven los dos.
    expect(screen.queryByTestId('progreso-de-la-restauracion')).not.toBeInTheDocument()

    // Y el aviso se puede cerrar, que es lo que lo hace un aviso y no un cartel.
    await usuario.click(within(resultado).getByRole('button'))
    await waitFor(() => {
      expect(screen.queryByTestId('resultado-de-la-restauracion')).not.toBeInTheDocument()
    })

    unmount()
    exigirTodoCerrado()
  })

  it('una fase fallida anuncia el desenlace con el mensaje de la API', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(listadoConUna(), REPOS, () => restauracionAceptada))

    const { unmount } = montar()
    await esperarLaTabla()
    await restaurar(usuario, fetch)

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO_DE_RESTAURACION,
      eventoDeRestauracion({ phase: 'fallida', motivo: 'el disco de destino está lleno' }),
    )

    expect(
      await screen.findByText('el disco de destino está lleno'),
    ).toBeInTheDocument()
    expect(screen.queryByTestId('progreso-de-la-restauracion')).not.toBeInTheDocument()
    unmount()
  })

  it('avisa cuando el stream se cae a mitad de la restauración', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(listadoConUna(), REPOS, () => restauracionAceptada))

    const { unmount } = montar()
    await esperarLaTabla()
    await restaurar(usuario, fetch)

    expect(screen.getByTestId('progreso-de-la-restauracion')).not.toHaveTextContent(
      /se ha perdido la conexión/i,
    )

    EventSourceFalso.ultima().emitir('error')

    // Un aviso de fase congelado y sin explicación parece una restauración
    // parada: el operador tiene que saber que la conexión se cayó.
    await waitFor(() => {
      expect(screen.getByTestId('progreso-de-la-restauracion')).toHaveTextContent(
        /se ha perdido la conexión/i,
      )
    })
    unmount()
  })

  it('bloquea el botón de la fila mientras la petición está en vuelo', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(
      manejadorCon(listadoConUna(), REPOS, () => new Promise<RespuestaSimulada>(() => {})),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    const confirmacion = await abrirConfirmacion(usuario)
    await usuario.click(confirmar(confirmacion))

    // Sin la guarda, un doble clic lanzaría dos restauraciones sobre el mismo
    // destino.
    await waitFor(() => {
      expect(screen.getByRole('button', { name: `Restaurar ${HEX}` })).toBeDisabled()
    })
    unmount()
  })

  it('no deja lanzar una segunda restauración mientras la primera está en vuelo, y lo explica', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(listadoConDos(), REPOS, () => restauracionAceptada))

    const { unmount } = montar()
    await esperarLaTabla()
    await restaurar(usuario, fetch)

    // El panel sigue **una** restauración: mientras la primera no cierre, la
    // segunda fila no ofrece lanzar otra, y lo dice. Sin la exclusión, el aviso
    // de la segunda reemplazaría al de la primera y su desenlace no se
    // anunciaría nunca.
    const otra = await botonDeFila(OTRO_HEX)
    expect(otra).toBeDisabled()
    // Las dos filas explican por qué no pueden lanzar (la que está en vuelo
    // tampoco puede repetir).
    expect(screen.getAllByText(/hay una restauración en curso sobre «web»/i)).toHaveLength(2)

    // Deshabilitada de verdad: pulsarla no lanza nada ni abre confirmación.
    await usuario.click(otra)
    expect(fetch.a(RUTA)).toHaveLength(1)
    expect(document.querySelector('.ant-popconfirm')).toBeNull()

    // El desenlace cierra la operación y las filas vuelven a ofrecerse.
    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO_DE_RESTAURACION,
      eventoDeRestauracion({ phase: 'terminada' }),
    )
    expect(await screen.findByTestId('resultado-de-la-restauracion')).toHaveTextContent(
      'Restauración terminada sobre «web»',
    )
    expect(await botonDeFila(OTRO_HEX)).toBeEnabled()
    unmount()
  })

  it('si el desenlace no llega, el aviso deja de afirmar que sigue en curso pasados los plazos', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const usuario = userEvent.setup({ advanceTimers: vi.advanceTimersByTimeAsync })
    const fetch = instalarFetchFalso(manejadorCon(listadoConUna(), REPOS, () => restauracionAceptada))

    const { unmount } = montar()
    await esperarLaTabla()
    await restaurar(usuario, fetch)

    expect(screen.getByTestId('progreso-de-la-restauracion')).toHaveTextContent(/en curso/)

    // El stream se queda mudo —el evento terminal se perdió y el stream no
    // guarda replay—: pasado el plazo el panel no puede seguir afirmando que la
    // restauración está en curso, porque no lo sabe.
    act(() => {
      vi.advanceTimersByTime(PLAZO_DE_CONFIRMACION_MS)
    })

    const aviso = screen.getByTestId('progreso-de-la-restauracion')
    expect(screen.getByText(/sin confirmación del servidor para «web»/i)).toBeInTheDocument()
    // Ya no afirma que siga en curso.
    expect(screen.queryByText(/Restaurando «web»/)).not.toBeInTheDocument()
    expect(aviso).toHaveTextContent(`${PLAZO_DE_CONFIRMACION_MS / 60_000} minutos`)

    // Y se puede cerrar: cerrarlo suelta la operación, así que las filas vuelven
    // a ofrecerse en vez de quedarse bloqueadas esperando un desenlace que
    // quizá no llegue.
    await usuario.click(within(aviso).getByRole('button'))
    await waitFor(() => {
      expect(screen.queryByTestId('progreso-de-la-restauracion')).not.toBeInTheDocument()
    })
    expect(await botonDeFila()).toBeEnabled()
    unmount()
  })

  it('con el `POST` sin contestar, el plazo cierra la espera y el desenlace retenido se anuncia al correlacionarlo', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const usuario = userEvent.setup({ advanceTimers: vi.advanceTimersByTimeAsync })

    // El `POST` acepta el trabajo pero no contesta: el panel no tiene con qué
    // correlacionar nada, ni siquiera el evento que ya llegó.
    let contestar: (respuesta: RespuestaSimulada) => void = () => {}
    const sinContestar = new Promise<RespuestaSimulada>((resolver) => {
      contestar = resolver
    })
    const fetch = instalarFetchFalso(manejadorCon(listadoConUna(), REPOS, () => sinContestar))

    const { unmount } = montar()
    await esperarLaTabla()
    await restaurar(usuario, fetch)

    // El desenlace llega **antes** que la respuesta y el proveedor lo retiene.
    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO_DE_RESTAURACION,
      eventoDeRestauracion({ phase: 'terminada' }),
    )

    // El plazo vale también por este camino: sin respuesta no hay correlación.
    act(() => {
      vi.advanceTimersByTime(PLAZO_DE_CONFIRMACION_MS)
    })
    expect(screen.getByTestId('progreso-de-la-restauracion')).toHaveTextContent(/sin confirmación/i)

    // Cuando el `POST` contesta, el evento retenido se correlaciona y el
    // desenlace se anuncia: no se había perdido, solo estaba sin dueño.
    await act(async () => {
      contestar(restauracionAceptada)
      await Promise.resolve()
    })
    expect(await screen.findByTestId('resultado-de-la-restauracion')).toHaveTextContent(
      'Restauración terminada sobre «web»',
    )
    expect(screen.queryByTestId('progreso-de-la-restauracion')).not.toBeInTheDocument()
    unmount()
  })

  it('normaliza una fase que el panel no conoce en vez de pintar el literal del backend', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(manejadorCon(listadoConUna(), REPOS, () => restauracionAceptada))

    const { unmount } = montar()
    await esperarLaTabla()
    await restaurar(usuario, fetch)

    // El contrato puede crecer sin que este panel se entere: una fase
    // desconocida se lee con un texto propio, no con la palabra suelta del
    // backend.
    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO_DE_RESTAURACION,
      eventoDeRestauracion({ phase: 'preparando' }),
    )

    const aviso = screen.getByTestId('progreso-de-la-restauracion')
    expect(aviso).toHaveTextContent(/estado desconocido/)
    expect(aviso).not.toHaveTextContent(/preparando/)
    unmount()
  })

  it('no ofrece restaurar una instantánea sin target conocido, y lo explica', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(
        listadoConUna({ target_id: null, target_name: null, target_type: null }),
        REPOS,
      ),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    const boton = await botonDeFila()
    expect(boton).toBeDisabled()
    expect(screen.getByText(/no se pudo resolver el target de origen/i)).toBeInTheDocument()

    // Deshabilitada de verdad: pulsarla no lanza la petición ni abre confirmación.
    await usuario.click(boton)
    expect(fetch.a(RUTA)).toHaveLength(0)
    expect(document.querySelector('.ant-popconfirm')).toBeNull()
    unmount()
  })

  it('no ofrece restaurar una instantánea de un target que no es de directorio, y lo explica', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon(
        listadoConUna({ target_id: 't2', target_name: 'bbdd', target_type: 'mariadb' }),
        REPOS,
      ),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    const boton = await botonDeFila()
    expect(boton).toBeDisabled()
    expect(screen.getByText(/solo se restauran targets de directorio/i)).toBeInTheDocument()
    expect(screen.getByText(/mariadb/)).toBeInTheDocument()

    await usuario.click(boton)
    expect(fetch.a(RUTA)).toHaveLength(0)
    expect(document.querySelector('.ant-popconfirm')).toBeNull()
    unmount()
  })

  it('si la API rechaza la restauración, muestra su mensaje', async () => {
    const usuario = userEvent.setup()
    const mensaje = 'el repositorio no responde'
    instalarFetchFalso(
      manejadorCon(listadoConUna(), REPOS, () => ({ status: 500, message: mensaje })),
    )

    const { unmount } = montar()
    await esperarLaTabla()

    const confirmacion = await abrirConfirmacion(usuario)
    await usuario.click(confirmar(confirmacion))

    expect(await screen.findByText(mensaje)).toBeInTheDocument()
    // El fallo del `POST` cierra la operación: no queda un aviso de fase en curso.
    expect(screen.queryByTestId('progreso-de-la-restauracion')).not.toBeInTheDocument()
    unmount()
  })
})
