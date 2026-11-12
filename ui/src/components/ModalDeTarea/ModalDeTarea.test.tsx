import { useState } from 'react'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../pruebas/fetchDePrueba'
import { repositorio, tarea, target } from '../../pruebas/fixtures'
import type { Job } from '../../services/api/types'
import { ModalDeTarea } from './ModalDeTarea'

type Usuario = ReturnType<typeof userEvent.setup>

const TARGETS = [target({ id: 't1', name: 'web' })]
const REPOSITORIOS = [repositorio({ id: 'r1', name: 's3' })]

/**
 * Arnés que reproduce el ciclo real de la pantalla de Tareas: abrir edición de
 * una fila, cerrar y abrir la de otra. La instancia de `ModalDeTarea` no se
 * desmonta entre filas, que es justo el escenario del hallazgo B1.
 */
function Arnes({ filas }: { filas: readonly Job[] }) {
  const [abierto, setAbierto] = useState(false)
  const [enEdicion, setEnEdicion] = useState<Job | null>(null)

  return (
    <>
      {filas.map((fila) => (
        <button
          key={fila.id}
          type="button"
          onClick={() => {
            setEnEdicion(fila)
            setAbierto(true)
          }}
        >
          {`Editar ${fila.id}`}
        </button>
      ))}
      <ModalDeTarea
        abierto={abierto}
        tarea={enEdicion}
        targets={TARGETS}
        repositorios={REPOSITORIOS}
        onCerrar={() => setAbierto(false)}
        onGuardado={() => setAbierto(false)}
      />
    </>
  )
}

function montarArnes(filas: readonly Job[]) {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <Arnes filas={filas} />
    </QueryClientProvider>,
  )
}

function montar({ tareaEditada = null }: { tareaEditada?: ReturnType<typeof tarea> | null } = {}) {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  })
  const onCerrar = vi.fn()
  const onGuardado = vi.fn()

  render(
    <QueryClientProvider client={cliente}>
      <ModalDeTarea
        abierto
        tarea={tareaEditada}
        targets={TARGETS}
        repositorios={REPOSITORIOS}
        onCerrar={onCerrar}
        onGuardado={onGuardado}
      />
    </QueryClientProvider>,
  )

  return { onCerrar, onGuardado }
}

async function elegir(usuario: Usuario, etiqueta: string, opcion: string) {
  await usuario.click(screen.getByRole('combobox', { name: etiqueta }))
  await usuario.click(await screen.findByTitle(opcion))
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('ModalDeTarea', () => {
  it('el alta envía el cuerpo del contrato y avisa de la tarea creada', async () => {
    const usuario = userEvent.setup()
    const creada = tarea({ id: 'j9', target_id: 't1', repository_id: 'r1' })
    const fetch = instalarFetchFalso(({ ruta, method }) => {
      if (ruta === '/api/v1/jobs' && method === 'POST') return { status: 201, data: creada }
      throw new Error(`ruta inesperada: ${method} ${ruta}`)
    })

    const { onGuardado, onCerrar } = montar()

    await screen.findByRole('dialog')
    await elegir(usuario, 'Target', 'web')
    await elegir(usuario, 'Repositorio', 's3')
    await usuario.click(screen.getByRole('button', { name: 'Guardar' }))

    await waitFor(() => expect(onGuardado).toHaveBeenCalledWith(creada))

    const alta = fetch.a('/api/v1/jobs').find((llamada) => llamada.method === 'POST')
    expect(alta?.cuerpo).toEqual({
      target_id: 't1',
      repository_id: 'r1',
      cron_expression: '0 0 3 * * *',
      keep_daily: 7,
      keep_weekly: 4,
      keep_monthly: 6,
      enabled: true,
    })
    expect(onCerrar).toHaveBeenCalled()
  })

  it('no envía una tarea con la retención a 0/0/0 y lo explica', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(() => {
      throw new Error('no debería llamarse a fetch')
    })

    montar({ tareaEditada: tarea({ id: 'j1', target_id: 't1', repository_id: 'r1' }) })

    await screen.findByRole('dialog')
    for (const etiqueta of ['Diarias', 'Semanales', 'Mensuales']) {
      const campo = screen.getByRole('spinbutton', { name: etiqueta })
      await usuario.clear(campo)
      await usuario.type(campo, '0')
    }

    await usuario.click(screen.getByRole('button', { name: 'Guardar' }))

    expect(
      await screen.findByText(/al menos una regla de retención tiene que conservar algo/i),
    ).toBeInTheDocument()
    expect(fetch.llamadas).toHaveLength(0)
  })

  it('un 400 del servidor se pinta junto a `cron_expression` y no cierra el modal', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(({ ruta, method }) =>
      ruta === '/api/v1/jobs' && method === 'POST'
        ? { status: 400, message: 'cron_expression: la expresión no es válida' }
        : (() => {
            throw new Error(`ruta inesperada: ${method} ${ruta}`)
          })(),
    )

    const { onCerrar, onGuardado } = montar()

    await screen.findByRole('dialog')
    await elegir(usuario, 'Target', 'web')
    await elegir(usuario, 'Repositorio', 's3')
    await usuario.click(screen.getByRole('button', { name: 'Guardar' }))

    expect(await screen.findByText(/la expresión no es válida/i)).toBeInTheDocument()
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(onCerrar).not.toHaveBeenCalled()
    expect(onGuardado).not.toHaveBeenCalled()
  })

  it('al editar carga los datos de la fila y envía un PUT con el mismo id', async () => {
    const usuario = userEvent.setup()
    const original = tarea({
      id: 'j7',
      target_id: 't1',
      repository_id: 'r1',
      cron_expression: '5 4 3 * * *',
      keep_daily: 1,
      keep_weekly: 2,
      keep_monthly: 3,
    })
    const fetch = instalarFetchFalso(({ ruta, method }) =>
      ruta === '/api/v1/jobs/j7' && method === 'PUT'
        ? { status: 200, data: original }
        : (() => {
            throw new Error(`ruta inesperada: ${method} ${ruta}`)
          })(),
    )

    montar({ tareaEditada: original })

    await screen.findByRole('dialog')
    expect(screen.getByRole('textbox', { name: 'Expresión cron' })).toHaveValue('5 4 3 * * *')
    expect(screen.getByRole('spinbutton', { name: 'Diarias' })).toHaveValue('1')

    await usuario.click(screen.getByRole('button', { name: 'Guardar' }))

    await waitFor(() => expect(fetch.a('/api/v1/jobs/j7')).toHaveLength(1))
    expect(fetch.a('/api/v1/jobs/j7')[0]?.method).toBe('PUT')
  })

  it('al cerrar y editar otra fila, el formulario muestra los valores de la nueva fila', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(() => {
      throw new Error('no debería llamarse a fetch')
    })

    const fila1 = tarea({
      id: 'j1',
      target_id: 't1',
      repository_id: 'r1',
      cron_expression: '1 1 1 * * *',
      keep_daily: 1,
    })
    const fila2 = tarea({
      id: 'j2',
      target_id: 't1',
      repository_id: 'r1',
      cron_expression: '2 2 2 * * *',
      keep_daily: 2,
    })

    montarArnes([fila1, fila2])

    // Editar la fila 1: se cambia un campo para que el estado quede "sucio".
    await usuario.click(screen.getByRole('button', { name: 'Editar j1' }))
    const cron1 = await screen.findByRole('textbox', { name: 'Expresión cron' })
    await usuario.clear(cron1)
    await usuario.type(cron1, '9 9 9 * * *')

    // Cerrar y abrir la edición de la fila 2.
    await usuario.click(screen.getByRole('button', { name: 'Cancelar' }))
    await usuario.click(screen.getByRole('button', { name: 'Editar j2' }))

    expect(await screen.findByRole('textbox', { name: 'Expresión cron' })).toHaveValue('2 2 2 * * *')
    expect(screen.getByRole('spinbutton', { name: 'Diarias' })).toHaveValue('2')
  })
})
