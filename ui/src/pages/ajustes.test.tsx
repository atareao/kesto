import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../pruebas/fetchDePrueba'
import type { ManejadorDeFetch, RespuestaSimulada } from '../pruebas/fetchDePrueba'
import { repositorio, target } from '../pruebas/fixtures'
import Ajustes from './ajustes'

const TARGET_MARIADB = target({
  id: 't1',
  name: 'base',
  target_type: 'mariadb',
  container_name: 'db',
  db_user: 'kesto',
  has_db_password: true,
  db_name: 'kesto',
  host_path: null,
})

const REPO = repositorio({
  id: 'r1',
  name: 's3',
  has_secret_access_key: true,
  has_passphrase: true,
})

function montar() {
  const cliente = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <Ajustes />
    </QueryClientProvider>,
  )
}

function manejadorCon(sobre: Partial<Record<string, ManejadorDeFetch>> = {}): ManejadorDeFetch {
  return (llamada) => {
    const clave = `${llamada.method} ${llamada.ruta}`
    const especifico = sobre[clave]
    if (especifico !== undefined) return especifico(llamada)

    if (llamada.ruta === '/api/v1/health') {
      return { status: 200, data: { version: '0.4.1', auth_mode: 'disabled' } }
    }
    if (llamada.ruta === '/api/v1/targets' && llamada.method === 'GET') {
      return { status: 200, data: [TARGET_MARIADB] }
    }
    if (llamada.ruta === '/api/v1/repositories' && llamada.method === 'GET') {
      return { status: 200, data: [REPO] }
    }
    throw new Error(`ruta inesperada: ${clave}`)
  }
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('pantalla de Ajustes', () => {
  it('muestra la versión y el modo de autenticación', async () => {
    instalarFetchFalso(manejadorCon())

    const { unmount } = montar()

    expect(await screen.findByText('0.4.1')).toBeInTheDocument()
    expect(screen.getByText(/desactivada/i)).toBeInTheDocument()
    unmount()
  })

  it('al editar un target no pinta el secreto y omite el campo vacío del PUT', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon({
        'PUT /api/v1/targets/t1': () => ({ status: 200, data: TARGET_MARIADB }),
      }),
    )

    const { unmount } = montar()

    const tabla = await screen.findByRole('table', { name: 'Targets' })
    await usuario.click(within(tabla).getByRole('button', { name: 'Editar' }))

    const dialogo = await screen.findByRole('dialog')
    const secreto = within(dialogo).getByLabelText('Contraseña de la base de datos')
    expect(secreto).toHaveValue('')
    expect(within(dialogo).getByText(/ya hay una contraseña guardada/i)).toBeInTheDocument()

    const nombre = within(dialogo).getByLabelText('Nombre')
    await usuario.clear(nombre)
    await usuario.type(nombre, 'base2')
    await usuario.click(within(dialogo).getByRole('button', { name: 'Guardar' }))

    await waitFor(() => {
      expect(fetch.a('/api/v1/targets/t1')).toHaveLength(1)
    })
    const put = fetch.a('/api/v1/targets/t1')[0]
    expect(put?.method).toBe('PUT')
    expect(put?.cuerpo).toMatchObject({ name: 'base2', target_type: 'mariadb' })
    expect(put?.cuerpo).not.toHaveProperty('db_password')
    unmount()
  })

  it('al editar un repositorio omite los secretos vacíos del PUT', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon({
        'PUT /api/v1/repositories/r1': () => ({ status: 200, data: REPO }),
      }),
    )

    const { unmount } = montar()

    const tabla = await screen.findByRole('table', { name: 'Repositorios' })
    await usuario.click(within(tabla).getByRole('button', { name: 'Editar' }))

    const dialogo = await screen.findByRole('dialog')
    expect(within(dialogo).getByLabelText('Clave secreta')).toHaveValue('')
    expect(within(dialogo).getByLabelText('Passphrase')).toHaveValue('')
    expect(within(dialogo).getByText(/ya hay una clave secreta guardada/i)).toBeInTheDocument()
    expect(within(dialogo).getByText(/ya hay una passphrase guardada/i)).toBeInTheDocument()

    await usuario.click(within(dialogo).getByRole('button', { name: 'Guardar' }))

    await waitFor(() => {
      expect(fetch.a('/api/v1/repositories/r1')).toHaveLength(1)
    })
    const put = fetch.a('/api/v1/repositories/r1')[0]
    expect(put?.cuerpo).not.toHaveProperty('secret_access_key')
    expect(put?.cuerpo).not.toHaveProperty('passphrase')
    unmount()
  })

  it('un target de directorio pide la ruta y no los campos de base de datos', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(manejadorCon())

    const { unmount } = montar()
    await screen.findByRole('table', { name: 'Targets' })

    await usuario.click(screen.getByRole('button', { name: 'Nuevo target' }))
    const dialogo = await screen.findByRole('dialog')

    expect(within(dialogo).getByLabelText('Ruta del host')).toBeInTheDocument()
    expect(
      within(dialogo).queryByLabelText('Contenedor'),
    ).not.toBeInTheDocument()
    unmount()
  })

  it('sin targets ni repositorios enseña estados vacíos', async () => {
    instalarFetchFalso(
      manejadorCon({
        'GET /api/v1/targets': () => ({ status: 200, data: [] }),
        'GET /api/v1/repositories': () => ({ status: 200, data: [] }),
      }),
    )

    const { unmount } = montar()

    expect(await screen.findByText('No hay targets configurados')).toBeInTheDocument()
    expect(screen.getByText('No hay repositorios configurados')).toBeInTheDocument()
    unmount()
  })

  it('mientras carga enseña un indicador visible', async () => {
    instalarFetchFalso(() => new Promise<RespuestaSimulada>(() => {}))

    const { unmount } = montar()

    expect((await screen.findAllByTestId('cargando')).length).toBeGreaterThan(0)
    unmount()
  })

  it('un error de carga se explica y se puede reintentar', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon({
        'GET /api/v1/targets': () => ({ status: 500, message: 'la base de datos no responde' }),
      }),
    )

    const { unmount } = montar()

    expect(await screen.findByText('No se pudieron cargar los targets')).toBeInTheDocument()
    expect(screen.getByText('la base de datos no responde')).toBeInTheDocument()

    const antes = fetch.a('/api/v1/targets').length
    await usuario.click(screen.getByRole('button', { name: 'Reintentar' }))
    await waitFor(() => {
      expect(fetch.a('/api/v1/targets').length).toBeGreaterThan(antes)
    })
    unmount()
  })
})
