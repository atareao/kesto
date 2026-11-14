import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { QueryClientProvider } from '@tanstack/react-query'
import App from '../../App'
import { instalarFetchFalso } from '../../pruebas/fetchDePrueba'
import { crearClienteDeConsultas } from '../../services/clienteDeConsultas'
import { instalarEventSourceFalso } from '../../services/eventosDePrueba'
import { ProveedorDeEventos } from '../../services/events'
import { ProveedorDeTema } from '../../services/tema'

/**
 * La capa de sesión que exige la spec (§3.4):
 *
 * - `openspec/changes/ui-panel/specs/ui/spec.md:246` — sin sesión, al cargar el
 *   panel en modo `oidc`, se ofrece entrar.
 * - `:60` — un `401` de una petición cualquiera devuelve al usuario a la pantalla
 *   de sesión.
 *
 * Se monta `App` entero (rutas y layout incluidos): es la integración lo que
 * estas pruebas afirman, no el detalle de la guarda.
 */
function montar(entrada: string) {
  const cliente = crearClienteDeConsultas()
  cliente.setDefaultOptions({
    queries: { retry: false, gcTime: 0, staleTime: 0 },
    mutations: { retry: false },
  })
  return render(
    <ProveedorDeTema>
      <QueryClientProvider client={cliente}>
        <ProveedorDeEventos>
          <MemoryRouter initialEntries={[entrada]}>
            <App />
          </MemoryRouter>
        </ProveedorDeEventos>
      </QueryClientProvider>
    </ProveedorDeTema>,
  )
}

const SALUD_OIDC = { version: '0.1.0', auth_mode: 'oidc' } as const
const SALUD_DISABLED = { version: '0.1.0', auth_mode: 'disabled' } as const
const SESION = { sub: 'ana', email: 'ana@example.org' } as const

function hayAuth(fetch: ReturnType<typeof instalarFetchFalso>): boolean {
  return fetch.llamadas.some((llamada) => llamada.ruta.startsWith('/api/v1/auth'))
}

beforeEach(() => {
  instalarEventSourceFalso()
})

afterEach(() => {
  // Se desmonta **antes** de retirar los dobles: si no, el proveedor SSE puede
  // agendar una reconexión con el `EventSource` ya retirado y lanzar un
  // `ReferenceError` que no tiene que ver con lo que se está midiendo.
  cleanup()
  vi.unstubAllGlobals()
})

describe('capa de sesión en las rutas', () => {
  it('en modo oidc sin sesión, cargar el panel lleva a la pantalla de sesión', async () => {
    const fetch = instalarFetchFalso(({ ruta }) => {
      if (ruta === '/api/v1/health') return { status: 200, data: SALUD_OIDC }
      if (ruta === '/api/v1/auth/session') return { status: 401, message: 'no hay sesión' }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar('/')

    const entrar = await screen.findByRole('link', { name: /entrar/i })
    expect(entrar).toHaveAttribute('href', '/api/v1/auth/login')
    // La pantalla protegida no se pinta sin sesión.
    expect(screen.queryByRole('heading', { level: 1, name: 'Dashboard' })).not.toBeInTheDocument()
    // Y la petición de sesión no se repite en bucle.
    await waitFor(() => expect(fetch.a('/api/v1/auth/session')).toHaveLength(1))
    await new Promise((listo) => setTimeout(listo, 50))
    expect(fetch.a('/api/v1/auth/session')).toHaveLength(1)
  })

  it('un 401 de una petición cualquiera devuelve a la pantalla de sesión', async () => {
    // La sesión vale al cargar; el servidor la invalida al contestar el listado.
    let sesionValida = true
    const fetch = instalarFetchFalso(({ ruta }) => {
      if (ruta === '/api/v1/health') return { status: 200, data: SALUD_OIDC }
      if (ruta === '/api/v1/auth/session') {
        return sesionValida
          ? { status: 200, data: SESION }
          : { status: 401, message: 'no hay sesión' }
      }
      if (ruta === '/api/v1/jobs') {
        sesionValida = false
        return { status: 401, message: 'no hay sesión' }
      }
      if (ruta === '/api/v1/targets') return { status: 200, data: [] }
      if (ruta === '/api/v1/repositories') return { status: 200, data: [] }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar('/tareas')

    // Primero se pinta la pantalla protegida con la sesión todavía válida.
    expect(await screen.findByRole('heading', { level: 1, name: 'Tareas' })).toBeInTheDocument()

    // El 401 del listado descarta la caché de sesión y devuelve a la de sesión.
    expect(await screen.findByRole('link', { name: /entrar/i })).toHaveAttribute(
      'href',
      '/api/v1/auth/login',
    )
    expect(screen.queryByRole('heading', { level: 1, name: 'Tareas' })).not.toBeInTheDocument()

    // La relectura por el `401` es una sola: el manejador no se realimenta.
    await waitFor(() => expect(fetch.a('/api/v1/auth/session')).toHaveLength(2))
    await new Promise((listo) => setTimeout(listo, 50))
    expect(fetch.a('/api/v1/auth/session')).toHaveLength(2)
  })

  it('en modo disabled entra directo y no toca ninguna ruta de /auth', async () => {
    const fetch = instalarFetchFalso(({ ruta }) => {
      if (ruta === '/api/v1/health') return { status: 200, data: SALUD_DISABLED }
      throw new Error(`ruta inesperada: ${ruta}`)
    })

    montar('/')

    expect(await screen.findByRole('heading', { level: 1, name: 'Dashboard' })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: /entrar/i })).not.toBeInTheDocument()
    expect(hayAuth(fetch)).toBe(false)
  })
})
