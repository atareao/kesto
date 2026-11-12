import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../pruebas/fetchDePrueba'
import { repositorio, tarea, target } from '../../pruebas/fixtures'
import { ModalDeTarea } from './ModalDeTarea'

type Usuario = ReturnType<typeof userEvent.setup>

const TARGETS = [target({ id: 't1', name: 'web' })]
const REPOSITORIOS = [repositorio({ id: 'r1', name: 's3' })]

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
})
