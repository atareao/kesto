/**
 * Cliente HTTP del panel.
 *
 * Habla siempre con `/api/v1` en el mismo origen —en desarrollo lo reenvía el
 * proxy de Vite (`vite.config.ts`)— y manda la cookie de sesión con
 * `credentials: "same-origin"`. Devuelve el `data` ya desenvuelto: quien llama
 * no ve la envoltura.
 *
 * Los desenlaces que cubre:
 *
 * - **OK:** parsea el JSON y desenvuelve.
 * - **Error de red:** `ErrorDeRed`, con un mensaje que se puede enseñar.
 * - **Error de la API:** `ErrorDeApi` con el `message` del backend —un `400`
 *   nombra el campo, un `409` dice que ya hay una copia en marcha—.
 * - **`401`:** `ErrorDeApi` con `status: 401`, que `esErrorDeSesion` reconoce
 *   como «sin sesión».
 * - **Cuerpo no-JSON:** `ErrorDeProtocolo`.
 * - **`204`:** no se le pide el cuerpo. Ojo: la API de Kesto **no** usa `204`
 *   —un borrado contesta `200` con `data` nulo—, así que esto es una red de
 *   seguridad para un intermediario, no la respuesta que se espera.
 */

import { ErrorDeApi, ErrorDeProtocolo, desenvolver, esErrorDeSesion } from './envelope'

/** Prefijo de todas las rutas de la API. */
export const RUTA_BASE = '/api/v1'

/** El servidor no está: red caída, proxy apagado o backend sin arrancar. */
export class ErrorDeRed extends Error {
  constructor(message: string, options?: { cause?: unknown }) {
    super(message, options)
    this.name = 'ErrorDeRed'
  }
}

/** Valor de un filtro de consulta. `undefined` y `null` se descartan. */
export type ValorDeConsulta = string | number | boolean | undefined | null

/** Filtros de consulta, en el orden en que se escriben en la URL. */
export type ConsultaDePeticion = Record<string, ValorDeConsulta>

export interface OpcionesDePeticion {
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE'
  /** Cuerpo de la petición; se serializa a JSON. */
  cuerpo?: unknown
  /** Filtros `?clave=valor`. */
  consulta?: ConsultaDePeticion
  signal?: AbortSignal
}

/** Compone `/api/v1` + la ruta + la consulta, respetando una ruta que ya lleve `?`. */
export function construirUrl(ruta: string, consulta?: ConsultaDePeticion): string {
  const base = `${RUTA_BASE}${ruta.startsWith('/') ? '' : '/'}${ruta}`

  const parametros = new URLSearchParams()
  for (const [clave, valor] of Object.entries(consulta ?? {})) {
    if (valor === undefined || valor === null) continue
    parametros.set(clave, String(valor))
  }

  const cadena = parametros.toString()
  if (cadena === '') return base
  return `${base}${base.includes('?') ? '&' : '?'}${cadena}`
}

function cabeceras(llevaCuerpo: boolean): HeadersInit {
  const cabeceras: Record<string, string> = { accept: 'application/json' }
  if (llevaCuerpo) cabeceras['content-type'] = 'application/json'
  return cabeceras
}

/**
 * Lanza una petición a la API y devuelve su `data`.
 *
 * @throws {ErrorDeRed} si no se pudo hablar con el servidor.
 * @throws {ErrorDeApi} si la API contestó un error, con su `message`.
 * @throws {ErrorDeProtocolo} si la respuesta no trae la envoltura.
 */
export async function peticion<T>(ruta: string, opciones: OpcionesDePeticion = {}): Promise<T> {
  const url = construirUrl(ruta, opciones.consulta)

  let respuesta: Response
  try {
    respuesta = await fetch(url, {
      method: opciones.method ?? 'GET',
      credentials: 'same-origin',
      headers: cabeceras(opciones.cuerpo !== undefined),
      body: opciones.cuerpo === undefined ? undefined : JSON.stringify(opciones.cuerpo),
      signal: opciones.signal,
    })
  } catch (causa) {
    // Un `AbortError` no es «el servidor no está»: se propaga tal cual para que
    // quien cancela lo distinga.
    if (causa instanceof DOMException && causa.name === 'AbortError') throw causa
    throw new ErrorDeRed('no se pudo contactar con el servidor', { cause: causa })
  }

  // Un `204` no tiene cuerpo por definición: pedirlo sería un error.
  if (respuesta.status === 204) return undefined as T

  const texto = await respuesta.text()
  if (texto.trim() === '') {
    throw new ErrorDeProtocolo(
      respuesta.status,
      `la respuesta (HTTP ${respuesta.status}) llegó sin cuerpo: la API siempre contesta la envoltura {status, message, data}`,
    )
  }

  let cuerpo: unknown
  try {
    cuerpo = JSON.parse(texto) as unknown
  } catch {
    throw new ErrorDeProtocolo(
      respuesta.status,
      `la respuesta (HTTP ${respuesta.status}) no es JSON`,
    )
  }

  try {
    return desenvolver<T>({ http: respuesta.status, cuerpo })
  } catch (error) {
    // La sesión ausente se reconoce por el código, no por la envoltura: si un
    // intermediario contesta un `401` a su manera, sigue siendo «sin sesión».
    if (respuesta.status === 401 && !esErrorDeSesion(error)) {
      throw new ErrorDeApi(401, error instanceof Error ? error.message : 'no hay sesión')
    }
    throw error
  }
}
