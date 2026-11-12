import { ErrorDeApi, ErrorDeProtocolo } from './api/envelope'
import { ErrorDeRed } from './api/client'

/**
 * Convierte cualquier error en un texto que se pueda enseñar. Nunca devuelve
 * JSON crudo ni un `[object Object]`: si no se reconoce, se dice que no se pudo
 * completar la operación.
 */
export function mensajeDeError(error: unknown): string {
  if (error instanceof ErrorDeRed) return 'No se pudo contactar con el servidor.'
  if (error instanceof ErrorDeApi) return error.message
  if (error instanceof ErrorDeProtocolo) {
    return 'La respuesta del servidor no tiene el formato esperado.'
  }
  if (error instanceof Error && error.message !== '') return error.message
  return 'No se pudo completar la operación.'
}
