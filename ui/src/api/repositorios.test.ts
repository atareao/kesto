import { instalarFetchFalso } from '../../tests/fetchDePrueba'
import { comprobarSaludDeRepositorio, comprobarSaludDeTodos } from './repositorios'

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('cliente de repositorios — comprobación de salud', () => {
  it('comprobar la salud de un repositorio va por el POST de ese repositorio y devuelve el desenlace', async () => {
    const fetch = instalarFetchFalso(() => ({
      status: 200,
      data: { health: 'ok', health_checked_at: '2026-01-02T03:04:05Z', health_detail: null },
    }))

    const salud = await comprobarSaludDeRepositorio('r1')

    const llamada = fetch.a('/api/v1/repositories/r1/health')[0]
    expect(llamada?.method).toBe('POST')
    expect(salud).toEqual({
      health: 'ok',
      health_checked_at: '2026-01-02T03:04:05Z',
      health_detail: null,
    })
  })

  it('comprobar todos va por el POST global y devuelve un elemento por repositorio', async () => {
    const fetch = instalarFetchFalso(() => ({
      status: 200,
      data: [
        { health: 'inaccesible', health_checked_at: '2026-01-02T03:04:05Z', health_detail: 'no responde' },
        { health: 'ok', health_checked_at: '2026-01-02T03:04:06Z', health_detail: null },
      ],
    }))

    const saludes = await comprobarSaludDeTodos()

    const llamada = fetch.a('/api/v1/repositories/health')[0]
    expect(llamada?.method).toBe('POST')
    expect(saludes.map((salud) => salud.health)).toEqual(['inaccesible', 'ok'])
  })

  it('un fallo de la comprobación se propaga como error legible, no como JSON crudo', async () => {
    instalarFetchFalso(() => ({ status: 500, message: 'no se pudo hablar con el destino' }))

    await expect(comprobarSaludDeRepositorio('r1')).rejects.toThrow(
      'no se pudo hablar con el destino',
    )
  })
})
