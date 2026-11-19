import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ProveedorDeTema } from '../services/tema'
import { CLAVE_DE_TEMA } from '../services/useTema'
import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import type { ManejadorDeFetch, RespuestaSimulada } from '../../tests/fetchDePrueba'
import Portada from './portada'
import emblema from '../assets/kesto-emblema.png'

vi.mock('./dashboard', () => ({
  default: () => <div data-testid="dashboard-de-prueba">Dashboard</div>,
}))

const SALUD_OIDC = { version: '0.1.0', auth_mode: 'oidc' } as const
const SALUD_DISABLED = { version: '0.1.0', auth_mode: 'disabled' } as const
const SESION = { sub: 'ana', email: 'ana@example.org' } as const

type ModoDePrueba = 'disabled' | 'oidc' | 'pendiente'
type EstadoDeSesion = 'con' | 'sin' | 'pendiente'

function manejador({
  modo,
  sesion,
}: {
  modo: ModoDePrueba
  sesion: EstadoDeSesion
}): ManejadorDeFetch {
  return ({ ruta }) => {
    if (ruta === '/api/v1/health') {
      if (modo === 'pendiente') return new Promise<RespuestaSimulada>(() => {})
      return { status: 200, data: modo === 'disabled' ? SALUD_DISABLED : SALUD_OIDC }
    }
    if (ruta === '/api/v1/auth/session') {
      if (sesion === 'pendiente') return new Promise<RespuestaSimulada>(() => {})
      if (sesion === 'con') return { status: 200, data: SESION }
      return { status: 401, message: 'no hay sesión' }
    }
    throw new Error(`ruta inesperada: ${ruta}`)
  }
}

function montar({
  modo = 'oidc',
  sesion = 'sin',
  tema,
}: {
  modo?: ModoDePrueba
  sesion?: EstadoDeSesion
  tema?: 'claro' | 'oscuro'
} = {}) {
  // El tema lo lee `ProveedorDeTema` al montar: se fija antes de renderizar.
  if (tema !== undefined) window.localStorage.setItem(CLAVE_DE_TEMA, tema)

  const fetch = instalarFetchFalso(manejador({ modo, sesion }))
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  const resultado = render(
    <ProveedorDeTema>
      <QueryClientProvider client={cliente}>
        <Portada />
      </QueryClientProvider>
    </ProveedorDeTema>,
  )
  return { ...resultado, fetch }
}

afterEach(() => {
  vi.unstubAllGlobals()
  window.localStorage.clear()
})

describe('Portada', () => {
  it('en modo oidc sin sesión pinta la portada pública y no el panel', async () => {
    const { fetch } = montar({ modo: 'oidc', sesion: 'sin' })

    expect(await screen.findByRole('heading', { level: 1, name: 'Kesto' })).toBeInTheDocument()
    expect(screen.getByText(/copias de seguridad/i)).toBeInTheDocument()
    expect(screen.queryByTestId('dashboard-de-prueba')).not.toBeInTheDocument()
    expect(fetch.llamadas.map((llamada) => llamada.ruta)).toEqual([
      '/api/v1/health',
      '/api/v1/auth/session',
    ])
  })

  it('el botón de entrada navega a /api/v1/auth/login sin pedir nada a la API', async () => {
    const { fetch } = montar({ modo: 'oidc', sesion: 'sin' })

    const entrar = await screen.findByRole('link', { name: /entrar/i })
    expect(entrar).toHaveAttribute('href', '/api/v1/auth/login')
    expect(fetch.llamadas).toHaveLength(2)
  })

  it('muestra el logo de la marca y el nombre «Kesto»', async () => {
    montar({ modo: 'oidc', sesion: 'sin' })

    expect(await screen.findByTestId('logo-de-la-portada')).toBeInTheDocument()
    expect(screen.getByRole('heading', { level: 1, name: 'Kesto' })).toBeInTheDocument()
  })

  it('el logo es un único asset, el mismo con el tema claro y con el oscuro', async () => {
    const claro = montar({ modo: 'oidc', sesion: 'sin', tema: 'claro' })
    const srcClaro = (await screen.findByTestId('logo-de-la-portada')).getAttribute('src')
    claro.unmount()

    montar({ modo: 'oidc', sesion: 'sin', tema: 'oscuro' })
    const srcOscuro = (await screen.findByTestId('logo-de-la-portada')).getAttribute('src')

    // El `src` es el del asset declarado en el código: sin variantes por tema.
    expect(srcClaro).toBe(emblema)
    expect(srcOscuro).toBe(srcClaro)
  })

  it('declara el tamaño intrínseco del logo para no provocar saltos de diseño', async () => {
    montar({ modo: 'oidc', sesion: 'sin' })

    const logo = await screen.findByTestId('logo-de-la-portada')
    expect(logo).toHaveAttribute('width', '256')
    expect(logo).toHaveAttribute('height', '256')
  })

  it('en modo disabled entra directa al panel y no pinta la portada', async () => {
    const { fetch } = montar({ modo: 'disabled', sesion: 'sin' })

    expect(await screen.findByTestId('dashboard-de-prueba')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 1, name: 'Kesto' })).not.toBeInTheDocument()
    expect(fetch.llamadas.map((llamada) => llamada.ruta)).toEqual(['/api/v1/health'])
  })

  it('en modo oidc con sesión entra al panel y no pinta la portada', async () => {
    const { fetch } = montar({ modo: 'oidc', sesion: 'con' })

    expect(await screen.findByTestId('dashboard-de-prueba')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 1, name: 'Kesto' })).not.toBeInTheDocument()
    expect(fetch.llamadas.map((llamada) => llamada.ruta)).toEqual([
      '/api/v1/health',
      '/api/v1/auth/session',
    ])
  })

  it('mientras /health no contesta muestra la carga y no la portada', () => {
    montar({ modo: 'pendiente' })

    expect(screen.getByTestId('cargando')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 1, name: 'Kesto' })).not.toBeInTheDocument()
  })

  it('mientras la sesión no contesta muestra la carga y no la portada', () => {
    montar({ modo: 'oidc', sesion: 'pendiente' })

    expect(screen.getByTestId('cargando')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 1, name: 'Kesto' })).not.toBeInTheDocument()
  })

  it('si /health falla muestra el error y no la portada', async () => {
    const fetch = instalarFetchFalso(() => {
      throw new TypeError('sin red')
    })
    const cliente = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
    })
    render(
      <ProveedorDeTema>
        <QueryClientProvider client={cliente}>
          <Portada />
        </QueryClientProvider>
      </ProveedorDeTema>,
    )

    expect(await screen.findByText(/no se pudo contactar con el servidor/i)).toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 1, name: 'Kesto' })).not.toBeInTheDocument()
    expect(fetch.llamadas).toHaveLength(1)
  })
})
