import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../pruebas/fetchDePrueba'
import { repositorio, tarea, target } from '../pruebas/fixtures'
import Tareas from './tareas'

type Usuario = ReturnType<typeof userEvent.setup>

const J1 = tarea({ id: 'j1', target_id: 't1', repository_id: 'r1' })
const J2 = tarea({ id: 'j2', target_id: 't2', repository_id: 'r1' })
const TARGETS = [target({ id: 't1', name: 'web' }), target({ id: 't2', name: 'api' })]
const REPOSITORIOS = [repositorio({ id: 'r1', name: 's3' })]

function montar() {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 }, mutations: { retry: false } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <Tareas />
    </QueryClientProvider>,
  )
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

describe('pantalla de Tareas', () => {
  it('lista las tareas y un 202 muestra la ejecución encolada', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(({ ruta, method }) => {
      if (ruta === '/api/v1/jobs' && method === 'GET') return { status: 200, data: [J1] }
      if (ruta === '/api/v1/targets') return { status: 200, data: TARGETS }
      if (ruta === '/api/v1/repositories') return { status: 200, data: REPOSITORIOS }
      if (ruta === '/api/v1/executions') return { status: 200, data: [] }
      if (ruta === '/api/v1/jobs/j1/run' && method === 'POST') {
        return { status: 202, data: { id: 'e1', job_id: 'j1', status: 'pending' } }
      }
      throw new Error(`ruta inesperada: ${method} ${ruta}`)
    })

    montar()

    expect(await screen.findByText('web')).toBeInTheDocument()
    await usuario.click(screen.getByRole('button', { name: 'Ejecutar' }))

    expect(await screen.findByText(/ejecución encolada \(pendiente\)/i)).toBeInTheDocument()
    expect(fetch.a('/api/v1/jobs/j1/run')[0]?.method).toBe('POST')
  })

  it('un 409 explica que ya hay una ejecución en curso', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(({ ruta, method }) => {
      if (ruta === '/api/v1/jobs' && method === 'GET') return { status: 200, data: [J1] }
      if (ruta === '/api/v1/targets') return { status: 200, data: TARGETS }
      if (ruta === '/api/v1/repositories') return { status: 200, data: REPOSITORIOS }
      if (ruta === '/api/v1/jobs/j1/run' && method === 'POST') {
        return { status: 409, message: 'ya hay una ejecución en curso' }
      }
      throw new Error(`ruta inesperada: ${method} ${ruta}`)
    })

    montar()

    await screen.findByText('web')
    await usuario.click(screen.getByRole('button', { name: 'Ejecutar' }))

    expect(
      await screen.findByText(/ya hay una ejecución en curso para esta tarea/i),
    ).toBeInTheDocument()
  })

  it('un 404 refresca la lista y la tarea desaparece', async () => {
    const usuario = userEvent.setup()
    let borrada = false
    instalarFetchFalso(({ ruta, method }) => {
      if (ruta === '/api/v1/jobs' && method === 'GET') {
        return { status: 200, data: borrada ? [] : [J1] }
      }
      if (ruta === '/api/v1/targets') return { status: 200, data: TARGETS }
      if (ruta === '/api/v1/repositories') return { status: 200, data: REPOSITORIOS }
      if (ruta === '/api/v1/jobs/j1/run' && method === 'POST') {
        borrada = true
        return { status: 404, message: 'la tarea no existe' }
      }
      throw new Error(`ruta inesperada: ${method} ${ruta}`)
    })

    montar()

    await screen.findByText('web')
    await usuario.click(screen.getByRole('button', { name: 'Ejecutar' }))

    expect(await screen.findByText(/la tarea ya no existe/i)).toBeInTheDocument()
    await waitFor(() => expect(screen.queryByText('web')).not.toBeInTheDocument())
  })

  it('un 201 añade la fila nueva a la lista', async () => {
    const usuario = userEvent.setup()
    let creada = false
    const fetch = instalarFetchFalso(({ ruta, method }) => {
      if (ruta === '/api/v1/jobs' && method === 'GET') {
        return { status: 200, data: creada ? [J1, J2] : [J1] }
      }
      if (ruta === '/api/v1/jobs' && method === 'POST') {
        creada = true
        return { status: 201, data: J2 }
      }
      if (ruta === '/api/v1/targets') return { status: 200, data: TARGETS }
      if (ruta === '/api/v1/repositories') return { status: 200, data: REPOSITORIOS }
      throw new Error(`ruta inesperada: ${method} ${ruta}`)
    })

    montar()

    await screen.findByText('web')
    await usuario.click(screen.getByRole('button', { name: 'Nueva tarea' }))
    await screen.findByRole('dialog')

    await elegir(usuario, 'Target', 'api')
    await elegir(usuario, 'Repositorio', 's3')
    await usuario.click(screen.getByRole('button', { name: 'Guardar' }))

    const tabla = screen.getByRole('table', { name: 'Tareas programadas' })
    expect(await within(tabla).findByText('api')).toBeInTheDocument()

    const alta = fetch.a('/api/v1/jobs').find((llamada) => llamada.method === 'POST')
    expect(alta?.cuerpo).toMatchObject({ target_id: 't2', repository_id: 'r1' })
  })
})
