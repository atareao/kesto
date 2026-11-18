import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { App as AntApp } from 'antd'
import { MemoryRouter, useLocation } from 'react-router-dom'
import App from './App'
import { instalarFetchFalso } from '../tests/fetchDePrueba'
import { instalarEventSourceFalso } from '../tests/eventosDePrueba'
import { ProveedorDeEventos } from './services/events'
import { CLAVE_DE_SESION } from './services/session'
import { ProveedorDeTema } from './services/tema'

/**
 * Pruebas de las rutas del panel (§3.1/§3.2): cada entrada del menú abre su
 * pantalla, la ruta activa se marca y una ruta desconocida conserva el menú.
 *
 * El `Ubicacion` es una sonda: pinta el `pathname` actual para poder afirmar que
 * la navegación cambió de ruta sin recargar el documento (es un `MemoryRouter`,
 * así que no hay recarga posible: si la ruta cambia, fue el router).
 *
 * Las rutas van bajo la guarda de sesión (§3.4), así que `/health` se dobla en
 * modo `disabled`: es el modo de desarrollo y el que deja pasar a las pantallas
 * sin capa de sesión. El `App` de antd envuelve el árbol porque el cierre de
 * sesión avisa de sus fallos con `App.useApp().message`.
 */
function Ubicacion() {
  const { pathname } = useLocation()
  return <span data-testid="ubicacion">{pathname}</span>
}

type ModoDePrueba = 'disabled' | 'oidc'
type EstadoDeSesion = 'con' | 'sin'

const SESION = { sub: 'ana', email: 'ana@example.org' } as const

function montar(
  entrada = '/',
  { modo = 'disabled', sesion = 'sin' }: { modo?: ModoDePrueba; sesion?: EstadoDeSesion } = {},
) {
  const cliente = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  })

  // El servidor «recuerda» si hay sesión: la salida la invalida, así que la
  // relectura posterior contesta `401`.
  let haySesion = sesion === 'con'
  const fetch = instalarFetchFalso(({ ruta }) => {
    if (ruta === '/api/v1/health') {
      return { status: 200, data: { version: '0.1.0', auth_mode: modo } }
    }
    if (ruta === '/api/v1/auth/session') {
      return haySesion
        ? { status: 200, data: SESION }
        : { status: 401, message: 'no hay sesión' }
    }
    if (ruta === '/api/v1/auth/logout') {
      haySesion = false
      return { status: 200, data: null }
    }
    throw new Error(`ruta inesperada: ${ruta}`)
  })

  const resultado = render(
    <ProveedorDeTema>
      <QueryClientProvider client={cliente}>
        <AntApp>
          <ProveedorDeEventos>
            <MemoryRouter initialEntries={[entrada]}>
              <Ubicacion />
              <App />
            </MemoryRouter>
          </ProveedorDeEventos>
        </AntApp>
      </QueryClientProvider>
    </ProveedorDeTema>,
  )
  return { ...resultado, cliente, fetch }
}

/** El pie de la barra lateral, donde viven los mandos del chasis. */
function pieDelPanel(): HTMLElement {
  const pie = document.querySelector<HTMLElement>('.pie-del-panel')
  expect(pie, 'no se encontró el pie de la barra lateral (.pie-del-panel)').not.toBeNull()
  if (pie === null) throw new Error('no se encontró el pie de la barra lateral (.pie-del-panel)')
  return pie
}

beforeEach(() => {
  instalarEventSourceFalso()
})

afterEach(() => {
  // Se desmonta **antes** de retirar los dobles: el proveedor SSE no debe agendar
  // una reconexión contra un `EventSource` ya retirado.
  cleanup()
  vi.unstubAllGlobals()
})

const ENTRADAS: ReadonlyArray<{ etiqueta: string; ruta: string }> = [
  { etiqueta: 'Dashboard', ruta: '/' },
  { etiqueta: 'Tareas', ruta: '/tareas' },
  { etiqueta: 'Histórico', ruta: '/historico' },
  { etiqueta: 'Snapshots', ruta: '/snapshots' },
  { etiqueta: 'Restauraciones', ruta: '/restauraciones' },
  { etiqueta: 'Ajustes', ruta: '/ajustes' },
]

describe('rutas del panel', () => {
  it('muestra el nombre del panel en el layout', async () => {
    montar()

    // La marca vive en la barra lateral: el logo y, junto a él, el nombre como
    // texto. No es un encabezado —el `h1` lo pone la pantalla activa—, así que
    // se busca dentro de la marca y no como texto suelto en el documento (donde
    // también casa el `h1` «Kesto» de la portada pública).
    const logo = await screen.findByTestId('logo-del-panel')
    expect(within(logo.parentElement as HTMLElement).getByText('Kesto')).toBeInTheDocument()
  })

  it.each(ENTRADAS)(
    'la entrada «$etiqueta» abre su pantalla y cambia la ruta',
    async ({ etiqueta, ruta }) => {
      const usuario = userEvent.setup()
      montar('/')

      const nav = await screen.findByRole('navigation', { name: 'Principal' })
      await usuario.click(within(nav).getByRole('link', { name: etiqueta }))

      expect(screen.getByTestId('ubicacion')).toHaveTextContent(ruta)
      expect(await screen.findByRole('heading', { level: 1, name: etiqueta })).toBeInTheDocument()
    },
  )

  it('marca como activa solo la entrada de la ruta actual', async () => {
    montar('/historico')

    const nav = await screen.findByRole('navigation', { name: 'Principal' })
    expect(within(nav).getByRole('link', { name: 'Histórico' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    expect(within(nav).getByRole('link', { name: 'Ajustes' })).not.toHaveAttribute('aria-current')
  })

  it('una ruta desconocida muestra «no encontrada» dentro del menú', async () => {
    montar('/no-existe')

    expect(await screen.findByRole('heading', { level: 1, name: /no encontrada/i })).toBeInTheDocument()

    const nav = await screen.findByRole('navigation', { name: 'Principal' })
    expect(nav).toBeInTheDocument()
    // Las seis pantallas del panel siguen alcanzables (ya no hay ruta de sesión).
    expect(within(nav).getAllByRole('link')).toHaveLength(6)
  })
})

describe('guarda de sesión de las rutas protegidas', () => {
  it('sin sesión una ruta protegida desvía a /', async () => {
    montar('/tareas', { modo: 'oidc', sesion: 'sin' })

    expect(await screen.findByRole('heading', { level: 1, name: 'Kesto' })).toBeInTheDocument()
    expect(screen.getByTestId('ubicacion')).toHaveTextContent('/')
    expect(screen.queryByRole('heading', { level: 1, name: 'Tareas' })).not.toBeInTheDocument()
  })
})

describe('ruta índice', () => {
  it('en modo oidc sin sesión enseña la portada pública', async () => {
    montar('/', { modo: 'oidc', sesion: 'sin' })

    expect(await screen.findByRole('heading', { level: 1, name: 'Kesto' })).toBeInTheDocument()
    expect(screen.getByText(/copias de seguridad/i)).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /entrar/i })).toHaveAttribute(
      'href',
      '/api/v1/auth/login',
    )
    expect(screen.queryByRole('heading', { level: 1, name: 'Dashboard' })).not.toBeInTheDocument()
  })

  it('en modo disabled entra directa al panel', async () => {
    montar('/', { modo: 'disabled' })

    expect(await screen.findByRole('heading', { level: 1, name: 'Dashboard' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 1, name: 'Kesto' })).not.toBeInTheDocument()
  })

  it('en modo oidc con sesión entra al panel', async () => {
    montar('/', { modo: 'oidc', sesion: 'con' })

    expect(await screen.findByRole('heading', { level: 1, name: 'Dashboard' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 1, name: 'Kesto' })).not.toBeInTheDocument()
  })
})

describe('la antigua ruta de sesión', () => {
  it('ya no existe: /sesion muestra «no encontrada»', async () => {
    montar('/sesion', { modo: 'disabled' })

    expect(
      await screen.findByRole('heading', { level: 1, name: /no encontrada/i }),
    ).toBeInTheDocument()
    expect(screen.getByTestId('ubicacion')).toHaveTextContent('/sesion')
  })
})

describe('cierre de sesión desde el panel', () => {
  it('manda el POST, descarta la caché de la sesión y vuelve a la portada pública', async () => {
    const usuario = userEvent.setup()
    const { cliente, fetch } = montar('/', { modo: 'oidc', sesion: 'con' })

    // Con sesión, el panel ofrece el cierre en el pie de la barra lateral.
    await screen.findByRole('navigation', { name: 'Principal' })
    await usuario.click(within(pieDelPanel()).getByRole('button', { name: 'Cerrar sesión' }))

    await waitFor(() => expect(fetch.a('/api/v1/auth/logout')).toHaveLength(1))
    expect(fetch.a('/api/v1/auth/logout')[0]?.method).toBe('POST')

    // La entrada de la sesión queda descartada: se marca invalidada y la
    // relectura (ya `401`) deja de servirla.
    await waitFor(() =>
      expect(
        cliente.getQueryCache().find({ queryKey: CLAVE_DE_SESION })?.state.isInvalidated,
      ).toBe(true),
    )
    await waitFor(() => expect(fetch.a('/api/v1/auth/session').length).toBeGreaterThan(1))

    // Y el panel vuelve a la portada pública, sin barra lateral ni identidad.
    expect(screen.getByTestId('ubicacion')).toHaveTextContent('/')
    expect(await screen.findByRole('link', { name: /entrar/i })).toHaveAttribute(
      'href',
      '/api/v1/auth/login',
    )
    expect(screen.queryByRole('navigation', { name: 'Principal' })).toBeNull()
    expect(screen.queryByTestId('identidad-de-la-sesion')).toBeNull()
  })
})

