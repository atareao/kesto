/**
 * Desarrollo de la envoltura única de la API de Kesto.
 *
 * Todas las respuestas de `/api/v1/*` —menos el stream SSE, que no es JSON—
 * viajan con la misma forma:
 *
 * - **Éxito:** `{ "status": <número>, "message": <texto>, "data": <T|null> }`.
 *   El `status` del cuerpo **duplica** el código HTTP.
 * - **Error:** `{ "status": <número>, "message": <texto> }`, **sin** `data`.
 *
 * Dos garantías que el panel no puede dar por supuestas:
 *
 * 1. Si el `status` del cuerpo y el de la cabecera no coinciden, la respuesta no
 *    es de fiar: es un error de **protocolo**, no un dato a medio leer.
 * 2. Un cuerpo sin envoltura (un proxy, un HTML de un `502`) tampoco lo es.
 *
 * Ojo con el `204`: la API **no** lo usa. Un borrado contesta `200` con `data`
 * nulo (ver `src/api/repositories.rs`, que lo afirma explícitamente), así que un
 * `204` real sería un cuerpo sin envoltura. `desenvolver` no lo trata aparte; es
 * el cliente quien evita pedirle el cuerpo a un `204`.
 */

/** Forma de la respuesta antes de desenvolverla: el código HTTP y el cuerpo crudo. */
export interface RespuestaEnBruto {
  /** Código HTTP de la cabecera. */
  http: number
  /** Cuerpo ya parseado, tal cual (puede no ser un objeto). */
  cuerpo: unknown
}

/** La respuesta es un error de la API: `{status, message}` sin `data`. */
export class ErrorDeApi extends Error {
  readonly status: number

  constructor(status: number, message: string) {
    super(message)
    this.name = 'ErrorDeApi'
    this.status = status
  }
}

/**
 * La respuesta incumple el protocolo: le falta la envoltura o su `status` no
 * cuadra con el HTTP. `status` es el que se pudo leer (el del cuerpo si lo hay).
 */
export class ErrorDeProtocolo extends Error {
  readonly status: number

  constructor(status: number, message: string) {
    super(message)
    this.name = 'ErrorDeProtocolo'
    this.status = status
  }
}

/** Código con el que la API dice «no hay sesión». */
export const SIN_SESION = 401

/**
 * ¿El error es un `401` de la API? Es la señal de «sin sesión»: el panel la usa
 * para no pedir la sesión otra vez.
 */
export function esErrorDeSesion(error: unknown): boolean {
  return error instanceof ErrorDeApi && error.status === SIN_SESION
}

function esObjeto(valor: unknown): valor is Record<string, unknown> {
  return typeof valor === 'object' && valor !== null && !Array.isArray(valor)
}

/** El `message` del cuerpo si es un texto legible; si no, `null`. */
function mensajeDelCuerpo(cuerpo: Record<string, unknown>): string | null {
  return typeof cuerpo.message === 'string' && cuerpo.message !== '' ? cuerpo.message : null
}

/**
 * Quita la envoltura y devuelve el `data`.
 *
 * @throws {ErrorDeApi} si el cuerpo es la envoltura de error, con su `message`.
 * @throws {ErrorDeProtocolo} si el cuerpo no trae envoltura o su `status` no
 * coincide con el HTTP.
 */
export function desenvolver<T>(respuesta: RespuestaEnBruto): T {
  const { http, cuerpo } = respuesta

  if (!esObjeto(cuerpo)) {
    throw new ErrorDeProtocolo(
      http,
      `la respuesta (HTTP ${http}) no trae la envoltura de la API: se esperaba un objeto JSON {status, message, data}`,
    )
  }

  if (typeof cuerpo.status !== 'number') {
    throw new ErrorDeProtocolo(
      http,
      `la envoltura de la respuesta (HTTP ${http}) no trae un \`status\` numérico`,
    )
  }

  if (cuerpo.status !== http) {
    const mensaje = mensajeDelCuerpo(cuerpo)
    throw new ErrorDeProtocolo(
      cuerpo.status,
      `el \`status\` del cuerpo (${cuerpo.status}) no coincide con el HTTP (${http})` +
        (mensaje === null ? '' : `: ${mensaje}`),
    )
  }

  if (http >= 400) {
    throw new ErrorDeApi(http, mensajeDelCuerpo(cuerpo) ?? `la API contestó ${http}`)
  }

  return cuerpo.data as T
}
