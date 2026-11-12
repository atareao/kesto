import { ErrorDeApi, ErrorDeProtocolo } from './envelope'
import { ErrorDeRed, RUTA_BASE, peticion } from './client'

// Respuesta mínima: el cliente lee `status` y el texto crudo del cuerpo, nunca
// las cabeceras, así que basta con eso para cubrir los cinco desenlaces.
function respuestaFalsa(estado: number, cuerpo: string): Response {
  return {
    status: estado,
    ok: estado >= 200 && estado < 300,
    text: async () => cuerpo,
  } as unknown as Response
}

function envoltura(estado: number, message: string, data: unknown): string {
  return JSON.stringify({ status: estado, message, data })
}

const fetchFalso = vi.fn<typeof fetch>()

beforeEach(() => {
  fetchFalso.mockReset()
  vi.stubGlobal('fetch', fetchFalso)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

/** La llamada a `fetch`, que siempre tiene que ser la única del test. */
function llamada(): [string, RequestInit] {
  expect(fetchFalso).toHaveBeenCalledTimes(1)
  return fetchFalso.mock.calls[0] as [string, RequestInit]
}

describe('peticion', () => {
  it('devuelve el `data` ya desenvuelto y habla con /api/v1 con la cookie de la sesión', async () => {
    fetchFalso.mockResolvedValue(
      respuestaFalsa(200, envoltura(200, 'ok', [{ id: 'a' }])),
    )

    await expect(peticion<Array<{ id: string }>>('/targets')).resolves.toEqual([{ id: 'a' }])

    const [url, opciones] = llamada()
    expect(url).toBe(`${RUTA_BASE}/targets`)
    expect(opciones.credentials).toBe('same-origin')
    expect(opciones.method).toBe('GET')
  })

  it('manda el cuerpo como JSON cuando la petición lo lleva', async () => {
    fetchFalso.mockResolvedValue(respuestaFalsa(201, envoltura(201, 'alta', { id: 'a' })))

    await peticion('/targets', { method: 'POST', cuerpo: { name: 'web' } })

    const [, opciones] = llamada()
    expect(opciones.method).toBe('POST')
    expect(opciones.body).toBe(JSON.stringify({ name: 'web' }))
    expect(new Headers(opciones.headers).get('content-type')).toBe('application/json')
  })

  it('serializa los filtros de consulta y descarta los que no van', async () => {
    fetchFalso.mockResolvedValue(respuestaFalsa(200, envoltura(200, 'ok', [])))

    await peticion('/executions', {
      consulta: { status: 'failed', limit: 50, offset: 0, job_id: undefined },
    })

    expect(llamada()[0]).toBe(`${RUTA_BASE}/executions?status=failed&limit=50&offset=0`)
  })

  it('lanza un error de red legible cuando no hay servidor', async () => {
    fetchFalso.mockRejectedValue(new TypeError('Failed to fetch'))

    const error = await peticion('/targets').catch((causa: unknown) => causa)

    expect(error).toBeInstanceOf(ErrorDeRed)
    expect((error as ErrorDeRed).message).toContain('servidor')
  })

  it('propaga el `message` del backend en un 400 que nombra el campo', async () => {
    fetchFalso.mockResolvedValue(
      respuestaFalsa(400, JSON.stringify({ status: 400, message: 'el campo `limit` de la consulta tiene que ser un número entre 1 y 200' })),
    )

    const error = await peticion('/executions').catch((causa: unknown) => causa)

    expect(error).toBeInstanceOf(ErrorDeApi)
    expect((error as ErrorDeApi).status).toBe(400)
    expect((error as ErrorDeApi).message).toContain('el campo `limit`')
  })

  it('propaga un 401 como error de sesión, sin envoltura de por medio', async () => {
    fetchFalso.mockResolvedValue(
      respuestaFalsa(401, JSON.stringify({ status: 401, message: 'no hay sesión' })),
    )

    const error = await peticion('/auth/session').catch((causa: unknown) => causa)

    expect(error).toBeInstanceOf(ErrorDeApi)
    expect((error as ErrorDeApi).status).toBe(401)
    expect((error as ErrorDeApi).message).toBe('no hay sesión')
  })

  it('lanza un error de protocolo si el cuerpo no es JSON', async () => {
    fetchFalso.mockResolvedValue(respuestaFalsa(500, '<html>502 Bad Gateway</html>'))

    const error = await peticion('/targets').catch((causa: unknown) => causa)

    expect(error).toBeInstanceOf(ErrorDeProtocolo)
    expect((error as ErrorDeProtocolo).status).toBe(500)
  })

  it('no intenta leer el cuerpo de un 204', async () => {
    const text = vi.fn<() => Promise<string>>(() => {
      throw new Error('un 204 no tiene cuerpo: no se puede leer')
    })
    fetchFalso.mockResolvedValue({ status: 204, ok: true, text } as unknown as Response)

    await expect(peticion('/targets/a', { method: 'DELETE' })).resolves.toBeUndefined()

    expect(text).not.toHaveBeenCalled()
  })

  it('trata una respuesta atendida sin cuerpo como error de protocolo', async () => {
    fetchFalso.mockResolvedValue(respuestaFalsa(200, ''))

    await expect(peticion('/targets')).rejects.toBeInstanceOf(ErrorDeProtocolo)
  })
})
