import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { EventSourceFalso, eventoDePrueba, instalarEventSourceFalso } from '../../../tests/eventosDePrueba'
import { ProveedorDeEventos } from '../../services/events'
import { NOMBRE_DEL_EVENTO } from '../../services/useEventStream'
import { instalarFetchFalso } from '../../../tests/fetchDePrueba'
import type { FetchFalso } from '../../../tests/fetchDePrueba'
import { ejecucion, tarea } from '../../../tests/fixtures'
import type { Job } from '../../services/api/types'
import { TablaDeTareasEnCurso } from './TablaDeTareasEnCurso'

function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <ProveedorDeEventos>
        <TablaDeTareasEnCurso />
      </ProveedorDeEventos>
    </QueryClientProvider>,
  )
}

function filaDe(nombre: string): HTMLElement {
  const celda = screen.getByText(nombre)
  const fila = celda.closest('tr')
  if (fila === null) throw new Error(`no hay fila para ${nombre}`)
  return fila as HTMLElement
}

/**
 * Un `Job` cuyo `name` puede llegar nulo o en blanco: valores que el contrato ya
 * no admite, pero que un backend anterior todavía podría publicar. El tipo
 * ensancha solo en la prueba, sin recurrir a un cast.
 */
type TareaConNombreAusente = Omit<Job, 'name'> & { name: string | null }

function tareaSinNombre(campos: { id: string; target_id: string }, name: string | null): TareaConNombreAusente {
  return { ...tarea(campos), name }
}

let fetch: FetchFalso

beforeEach(() => {
  instalarEventSourceFalso()
  // La tabla no depende de los targets: si algo los pidiera, esta ruta inesperada
  // lo haría saltar.
  fetch = instalarFetchFalso(({ ruta }) => {
    if (ruta === '/api/v1/jobs') {
      return {
        status: 200,
        data: [
          tarea({ id: 'j1', target_id: 't1', name: 'copia nocturna' }),
          tarea({ id: 'j2', target_id: 't2', name: 'copia de api' }),
        ],
      }
    }
    if (ruta === '/api/v1/executions') {
      return {
        status: 200,
        data: { executions: [ejecucion({ id: 'e1', job_id: 'j1', status: 'success' })], total: 1 },
      }
    }
    throw new Error(`ruta inesperada: ${ruta}`)
  })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('TablaDeTareasEnCurso', () => {
  it('el estado es el de la última ejecución y el SSE lo actualiza en vivo', async () => {
    const { unmount } = montar()

    await screen.findByText('copia nocturna')
    expect(within(filaDe('copia nocturna')).getByText('Correcta')).toBeInTheDocument()

    EventSourceFalso.ultima().emitir(
      NOMBRE_DEL_EVENTO,
      eventoDePrueba({ execution_id: 'e2', job_id: 'j1', status: 'running', percentage: 42 }),
    )

    expect(within(filaDe('copia nocturna')).getByText('En curso')).toBeInTheDocument()
    expect(within(filaDe('copia nocturna')).getByText('42%')).toBeInTheDocument()
    unmount()
  })

  it('una tarea sin ejecuciones se pinta como «sin ejecuciones», no como éxito', async () => {
    const { unmount } = montar()

    await screen.findByText('copia de api')

    expect(within(filaDe('copia de api')).getByText('Sin ejecuciones')).toBeInTheDocument()
    expect(within(filaDe('copia de api')).queryByText('Correcta')).not.toBeInTheDocument()
    unmount()
  })

  it('no pide los targets: el nombre de la tarea no necesita resolver el del target', async () => {
    montar()

    await screen.findByText('copia nocturna')

    expect(fetch.a('/api/v1/targets')).toHaveLength(0)
  })

  it('la columna «Tarea» muestra el nombre de la tarea y nunca el del target', async () => {
    montar()

    await screen.findByText('copia nocturna')
    const fila = filaDe('copia nocturna')

    // La celda enseña el nombre de la tarea...
    expect(within(fila).getByText('copia nocturna')).toBeInTheDocument()
    // ...y ni el nombre del target («t1») ni su identificador.
    expect(within(fila).queryByText('t1')).not.toBeInTheDocument()
  })

  it('la columna «Tarea» nunca queda en blanco: un nombre nulo o en blanco pinta «sin nombre»', async () => {
    fetch = instalarFetchFalso(({ ruta }) => {
      if (ruta === '/api/v1/jobs') {
        return {
          status: 200,
          data: [
            tareaSinNombre({ id: 'j3', target_id: 't2' }, null),
            tareaSinNombre({ id: 'j4', target_id: 't3' }, '   '),
          ],
        }
      }
      if (ruta === '/api/v1/executions') return { status: 200, data: { executions: [], total: 0 } }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar()

    const marcadores = await screen.findAllByText('sin nombre')
    expect(marcadores).toHaveLength(2)

    for (const marcador of marcadores) {
      // El marcador es explícito y atenuado, y la fila no enseña ningún target.
      expect(marcador).toHaveClass('ant-typography-secondary')
      const fila = marcador.closest('tr') as HTMLElement
      expect(within(fila).queryByText('t2')).not.toBeInTheDocument()
      expect(within(fila).queryByText('t3')).not.toBeInTheDocument()
    }
  })
})
