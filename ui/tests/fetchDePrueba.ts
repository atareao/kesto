/**
 * Doble de `fetch` para las pruebas de pantalla: responde por ruta y método, y
 * deja a la vista las llamadas —URL, método y cuerpo ya parseado— para poder
 * afirmar qué se envió.
 *
 * La respuesta imita la envoltura real de la API: `{status, message, data}` en
 * el éxito y `{status, message}` en el error, con el `status` del cuerpo igual
 * al HTTP. Así el cliente de verdad (`peticion`) se ejercita sin dobles.
 */

import { vi } from 'vitest'

/** Una llamada que hizo el panel. */
export interface LlamadaDePrueba {
  /** URL completa, con `/api/v1` y la consulta. */
  url: string
  /** Ruta sin origen ni consulta, p. ej. `/api/v1/jobs`. */
  ruta: string
  method: string
  /** Cuerpo JSON ya parseado, o `undefined` si la petición no llevaba. */
  cuerpo: unknown
}

/** Lo que devuelve el manejador para una llamada. */
export interface RespuestaSimulada {
  status: number
  data?: unknown
  message?: string
}

export type ManejadorDeFetch = (
  llamada: LlamadaDePrueba,
) => RespuestaSimulada | Promise<RespuestaSimulada>

export interface FetchFalso {
  llamadas: LlamadaDePrueba[]
  /** Las llamadas a una ruta concreta, en orden. */
  a: (ruta: string) => LlamadaDePrueba[]
}

function urlDe(entrada: RequestInfo | URL): string {
  if (typeof entrada === 'string') return entrada
  if (entrada instanceof URL) return entrada.toString()
  return entrada.url
}

/**
 * Instala el `fetch` falso. Hay que llamar a `vi.unstubAllGlobals()` al acabar
 * (o usar `afterEach`), como con el resto de dobles.
 */
export function instalarFetchFalso(manejador: ManejadorDeFetch): FetchFalso {
  const llamadas: LlamadaDePrueba[] = []

  const falso = vi.fn(async (entrada: RequestInfo | URL, init?: RequestInit) => {
    const url = urlDe(entrada)
    const ruta = new URL(url, 'http://localhost').pathname
    const method = init?.method ?? 'GET'
    const cuerpo = typeof init?.body === 'string' ? (JSON.parse(init.body) as unknown) : undefined

    const llamada: LlamadaDePrueba = { url, ruta, method, cuerpo }
    llamadas.push(llamada)

    const respuesta = await manejador(llamada)
    const { status } = respuesta
    const cuerpoDeRespuesta =
      status >= 400
        ? { status, message: respuesta.message ?? `la API contestó ${status}` }
        : { status, message: respuesta.message ?? 'ok', data: respuesta.data ?? null }

    return {
      status,
      ok: status >= 200 && status < 300,
      text: async () => JSON.stringify(cuerpoDeRespuesta),
    } as unknown as Response
  })

  vi.stubGlobal('fetch', falso)

  return {
    llamadas,
    a: (ruta) => llamadas.filter((llamada) => llamada.ruta === ruta),
  }
}
