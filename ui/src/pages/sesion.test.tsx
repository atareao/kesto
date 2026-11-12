import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router-dom'
import Sesion from './sesion'

/**
 * La pantalla de sesión es la de la §3.4: sus cuatro desenlaces los decide el
 * `auth_mode` de `/health` y el `200`/`401` de `/auth/session`. El `fetch` va de
 * mentira para poder afirmar **qué** rutas se llaman y cuáles no.
 */
function respuestaFalsa(estado: number, data: unknown): Response {
  const cuerpo = JSON.stringify(
    estado >= 400
      ? { status: estado, message: 'no hay sesión' }
      : { status: estado, message: 'ok', data },
  )
  return { status: estado, ok: estado < 400, text: async () => cuerpo } as unknown as Response
}

const fetchFalso = vi.fn<typeof fetch>()

function rutasLlamadas(): string[] {
  return fetchFalso.mock.calls.map(([entrada]) => String(entrada))
}

function montar() {
  const cliente = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <MemoryRouter initialEntries={['/sesion']}>
        <Sesion />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

/** `/health` siempre dice `oidc`; `/auth/session` y `/auth/logout` van aparte. */
function enrutar(manejador: (url: string) => Response | undefined) {
  fetchFalso.mockImplementation(async (entrada) => {
    const url = String(entrada)
    if (url.endsWith('/health')) {
      return respuestaFalsa(200, { version: '0.1.0', auth_mode: 'oidc' })
    }
    const respuesta = manejador(url)
    if (respuesta === undefined) throw new Error(`ruta inesperada: ${url}`)
    return respuesta
  })
}

beforeEach(() => {
  fetchFalso.mockReset()
  vi.stubGlobal('fetch', fetchFalso)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('pantalla de sesión', () => {
  it('modo disabled: entra directo, avisa y no llama a ninguna ruta de /auth', async () => {
    fetchFalso.mockResolvedValue(respuestaFalsa(200, { version: '0.1.0', auth_mode: 'disabled' }))

    montar()

    expect(await screen.findByText(/sin autenticación/i)).toBeInTheDocument()
    expect(rutasLlamadas()).toEqual(['/api/v1/health'])
    expect(screen.queryByRole('link', { name: /entrar/i })).not.toBeInTheDocument()
  })

  it('modo oidc con sesión: muestra sub y email, y ningún token', async () => {
    enrutar(() => respuestaFalsa(200, { sub: 'ana', email: 'ana@example.org' }))

    montar()

    expect(await screen.findByText('ana')).toBeInTheDocument()
    expect(screen.getByText('ana@example.org')).toBeInTheDocument()
    expect(screen.queryByText(/token/i)).not.toBeInTheDocument()
  })

  it('modo oidc sin sesión: ofrece entrar apuntando a /api/v1/auth/login', async () => {
    enrutar(() => respuestaFalsa(401, null))

    montar()

    const entrar = await screen.findByRole('link', { name: /entrar/i })
    expect(entrar).toHaveAttribute('href', '/api/v1/auth/login')
  })

  it('al salir llama a POST /auth/logout y vuelve a la pantalla de sesión', async () => {
    const usuario = userEvent.setup()
    let haySesion = true
    enrutar((url) => {
      if (!url.endsWith('/auth/logout')) {
        return haySesion
          ? respuestaFalsa(200, { sub: 'ana', email: 'ana@example.org' })
          : respuestaFalsa(401, null)
      }
      haySesion = false
      return respuestaFalsa(200, null)
    })

    montar()
    await screen.findByText('ana')

    await usuario.click(screen.getByRole('button', { name: /cerrar sesión/i }))

    expect(await screen.findByRole('link', { name: /entrar/i })).toBeInTheDocument()
    expect(fetchFalso).toHaveBeenCalledWith(
      '/api/v1/auth/logout',
      expect.objectContaining({ method: 'POST' }),
    )
  })

  it('mientras comprueba la sesión muestra carga y no el vacío', () => {
    fetchFalso.mockImplementation(() => new Promise<Response>(() => {}))

    montar()

    expect(screen.getByRole('status')).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: /entrar/i })).not.toBeInTheDocument()
  })

  it('si /health falla muestra el mensaje y ofrece reintentar', async () => {
    fetchFalso.mockRejectedValue(new TypeError('sin red'))

    montar()

    expect(await screen.findByText(/no se pudo contactar con el servidor/i)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /reintentar/i })).toBeInTheDocument()
  })

  it('no vuelve a pedir la sesión en bucle tras un 401', async () => {
    enrutar(() => respuestaFalsa(401, null))

    montar()
    await screen.findByRole('link', { name: /entrar/i })
    await waitFor(() => expect(rutasLlamadas()).toContain('/api/v1/auth/session'))

    const llamadasASesion = rutasLlamadas().filter((url) => url.endsWith('/auth/session'))
    expect(llamadasASesion).toHaveLength(1)
  })
})
