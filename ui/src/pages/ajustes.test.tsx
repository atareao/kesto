import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import type { ManejadorDeFetch, RespuestaSimulada } from '../../tests/fetchDePrueba'
import { repositorio, target } from '../../tests/fixtures'
import { elegirEnSelect, limpiarDesplegables } from '../../tests/selectDeAntd'
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
  // Restos de desplegables de Antd colgados en `document.body` por otro caso.
  limpiarDesplegables()
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

  it('da de alta un target sqlite y lo enseña en la tabla', async () => {
    const usuario = userEvent.setup()
    const creado = target({
      id: 't9',
      name: 'kesto-db',
      target_type: 'sqlite',
      container_name: 'kesto',
      host_path: null,
      db_path: '/var/lib/kesto/kesto.db',
    })
    let listado = [TARGET_MARIADB]
    const fetch = instalarFetchFalso(
      manejadorCon({
        'GET /api/v1/targets': () => ({ status: 200, data: listado }),
        'POST /api/v1/targets': () => {
          listado = [...listado, creado]
          return { status: 201, data: creado }
        },
      }),
    )

    const { unmount } = montar()
    await screen.findByRole('table', { name: 'Targets' })

    await usuario.click(screen.getByRole('button', { name: 'Nuevo target' }))
    const dialogo = await screen.findByRole('dialog')
    await usuario.type(within(dialogo).getByLabelText('Nombre'), 'kesto-db')
    await elegirEnSelect(usuario, 'Tipo', 'SQLite', dialogo)
    await usuario.type(within(dialogo).getByLabelText('Contenedor'), 'kesto')
    await usuario.type(within(dialogo).getByLabelText(/ruta del \.db/i), '/var/lib/kesto/kesto.db')
    await usuario.click(within(dialogo).getByRole('button', { name: 'Guardar' }))

    await waitFor(() => {
      expect(fetch.a('/api/v1/targets').filter((llamada) => llamada.method === 'POST')).toHaveLength(1)
    })
    const alta = fetch.a('/api/v1/targets').find((llamada) => llamada.method === 'POST')
    expect(alta?.cuerpo).toMatchObject({
      name: 'kesto-db',
      target_type: 'sqlite',
      container_name: 'kesto',
      db_path: '/var/lib/kesto/kesto.db',
    })
    // Un sqlite no manda ni ruta de host ni credenciales de base de datos.
    expect(alta?.cuerpo).not.toHaveProperty('host_path')

    // La tabla se recarga y la fila nueva queda a la vista con su destino.
    const tabla = screen.getByRole('table', { name: 'Targets' })
    expect(await within(tabla).findByText('kesto-db')).toBeInTheDocument()
    expect(within(tabla).getByText('kesto · /var/lib/kesto/kesto.db')).toBeInTheDocument()
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

/**
 * La salud es **por repositorio**: la columna la nombra con su texto —el color
 * no es el único portador— y trae la fecha de la última comprobación; la fila
 * ofrece comprobar ese repositorio y la sección, comprobar todos.
 */
describe('pantalla de Ajustes — salud por repositorio', () => {
  function filaDe(nombre: string): HTMLElement {
    const fila = screen.getByText(nombre).closest('tr')
    if (fila === null) throw new Error(`no hay fila para ${nombre}`)
    return fila as HTMLElement
  }

  it('pinta el estado con su texto y la fecha de cada repositorio', async () => {
    instalarFetchFalso(
      manejadorCon({
        'GET /api/v1/repositories': () => ({
          status: 200,
          data: [
            repositorio({
              id: 'r1',
              name: 's3-uno',
              health: 'ok',
              health_checked_at: '2026-01-02T03:04:05Z',
            }),
            repositorio({ id: 'r2', name: 's3-dos', health: 'desconocido' }),
          ],
        }),
      }),
    )

    const { unmount } = montar()

    const tabla = await screen.findByRole('table', { name: 'Repositorios' })
    // El texto acompaña al color en los dos estados.
    expect(within(filaDe('s3-uno')).getByText('Correcto')).toBeInTheDocument()
    expect(within(filaDe('s3-uno')).getByText(/última comprobación/i)).toHaveTextContent('2026')
    // Sin comprobar: «Desconocido» con el guion del panel en la fecha.
    expect(within(filaDe('s3-dos')).getByText('Desconocido')).toBeInTheDocument()
    expect(within(filaDe('s3-dos')).getByText(/última comprobación/i)).toHaveTextContent('—')
    expect(tabla).toBeInTheDocument()
    unmount()
  })

  it('comprobar un repositorio pide su comprobación y el indicador refleja el estado devuelto', async () => {
    const usuario = userEvent.setup()
    const comprobado = repositorio({
      id: 'r1',
      // El mismo nombre: comprobar un repositorio no lo renombra, y así la fila
      // se localiza igual antes y después del refresco.
      name: 's3',
      health: 'inaccesible',
      health_checked_at: '2026-01-02T03:04:05Z',
      health_detail: 'el destino no responde',
    })
    let listado = [REPO]
    const fetch = instalarFetchFalso(
      manejadorCon({
        'GET /api/v1/repositories': () => ({ status: 200, data: listado }),
        'POST /api/v1/repositories/r1/health': () => {
          listado = [comprobado]
          return {
            status: 200,
            data: {
              health: 'inaccesible',
              health_checked_at: '2026-01-02T03:04:05Z',
              health_detail: 'el destino no responde',
            },
          }
        },
      }),
    )

    const { unmount } = montar()

    await screen.findByRole('table', { name: 'Repositorios' })
    await usuario.click(within(filaDe('s3')).getByRole('button', { name: 'Comprobar' }))

    await waitFor(() => {
      expect(fetch.a('/api/v1/repositories/r1/health')).toHaveLength(1)
    })
    expect(await within(filaDe('s3')).findByText('Inaccesible')).toBeInTheDocument()
    expect(within(filaDe('s3')).getByText('el destino no responde')).toBeInTheDocument()
    unmount()
  })

  it('el botón global comprueba todos los repositorios', async () => {
    const usuario = userEvent.setup()
    const fetch = instalarFetchFalso(
      manejadorCon({
        'POST /api/v1/repositories/health': () => ({
          status: 200,
          data: [{ health: 'ok', health_checked_at: '2026-01-02T03:04:05Z', health_detail: null }],
        }),
      }),
    )

    const { unmount } = montar()

    await screen.findByRole('table', { name: 'Repositorios' })
    await usuario.click(screen.getByRole('button', { name: 'Comprobar todos' }))

    await waitFor(() => {
      expect(fetch.a('/api/v1/repositories/health')).toHaveLength(1)
    })
    expect(fetch.a('/api/v1/repositories/health')[0]?.method).toBe('POST')
    unmount()
  })

  it('una comprobación en curso no bloquea el resto de la tabla', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(
      manejadorCon({
        'GET /api/v1/repositories': () => ({
          status: 200,
          data: [repositorio({ id: 'r1', name: 's3-uno' }), repositorio({ id: 'r2', name: 's3-dos' })],
        }),
        // La comprobación de `s3-uno` no termina: lo que se mira pasa en curso.
        'POST /api/v1/repositories/r1/health': () => new Promise<RespuestaSimulada>(() => {}),
      }),
    )

    const { unmount } = montar()

    await screen.findByRole('table', { name: 'Repositorios' })
    await usuario.click(within(filaDe('s3-uno')).getByRole('button', { name: 'Comprobar' }))

    // La fila que se está comprobando lo anuncia: el botón queda cargando
    // (Antd le añade al nombre accesible el del icono, por eso se busca por
    // coincidencia parcial)…
    await waitFor(() => {
      expect(within(filaDe('s3-uno')).getByRole('button', { name: /comprobar/i })).toHaveClass(
        'ant-btn-loading',
      )
    })
    // …y la otra fila sigue intacta y operable.
    expect(within(filaDe('s3-dos')).getByText(/última comprobación/i)).toBeInTheDocument()
    expect(within(filaDe('s3-dos')).getByRole('button', { name: 'Editar' })).toBeEnabled()
    unmount()
  })

  it('mientras comprueba, la fila conserva el estado persistido: el panel no adelanta un desenlace', async () => {
    const usuario = userEvent.setup()
    const persistido = repositorio({
      id: 'r1',
      name: 's3-uno',
      health: 'sin_permiso',
      health_checked_at: '2026-01-02T03:04:05Z',
      health_detail: 'faltan permisos de lectura',
    })
    instalarFetchFalso(
      manejadorCon({
        'GET /api/v1/repositories': () => ({ status: 200, data: [persistido] }),
        // La comprobación nunca llega a responder: lo que se afirma es el
        // estado que se enseña **durante** la espera.
        'POST /api/v1/repositories/r1/health': () => new Promise<RespuestaSimulada>(() => {}),
      }),
    )

    const { unmount } = montar()

    await screen.findByRole('table', { name: 'Repositorios' })
    expect(within(filaDe('s3-uno')).getByText('Sin permiso de lectura')).toBeInTheDocument()

    await usuario.click(within(filaDe('s3-uno')).getByRole('button', { name: 'Comprobar' }))
    await waitFor(() => {
      expect(within(filaDe('s3-uno')).getByRole('button', { name: /comprobar/i })).toHaveClass(
        'ant-btn-loading',
      )
    })

    // Sigue lo **persistido**: ni un «Correcto» optimista ni un estado en blanco.
    const fila = filaDe('s3-uno')
    expect(within(fila).getByText('Sin permiso de lectura')).toBeInTheDocument()
    expect(within(fila).getByText('faltan permisos de lectura')).toBeInTheDocument()
    expect(within(fila).getByText(/última comprobación/i)).toHaveTextContent('2026')
    expect(within(fila).queryByText('Correcto')).not.toBeInTheDocument()
    unmount()
  })
})
