/**
 * El cliente de consultas del panel, con la política de sesión cableada.
 *
 * La capa de sesión del §3.4 no puede depender de que cada pantalla recuerde
 * tratar un `401`: un `401` de **cualquier** petición significa «la sesión ya no
 * vale». Aquí se reacciona una sola vez y en un solo sitio: se descarta la caché
 * de la sesión, su relectura da `401` y la guarda (`SoloConSesion`) lleva al
 * usuario a la pantalla de sesión.
 *
 * El `401` de la **propia** sesión se deja pasar sin invalidarla otra vez: eso
 * sería releerla en bucle, y la spec lo prohíbe explícitamente.
 */

import { MutationCache, QueryCache, QueryClient } from '@tanstack/react-query'
import { esErrorDeSesion } from './api/envelope'
import { CLAVE_DE_SESION } from './session'

/** ¿La clave es la de la sesión? `queryKey` se compara por su primer elemento. */
function esLaClaveDeSesion(clave: readonly unknown[] | undefined): boolean {
  return clave !== undefined && clave[0] === CLAVE_DE_SESION[0]
}

/** Un `401` descarta la caché de la sesión; el `401` de la sesión ya es la señal. */
function anteUnError(
  error: unknown,
  clave: readonly unknown[] | undefined,
  cliente: QueryClient,
): void {
  if (!esErrorDeSesion(error)) return
  if (esLaClaveDeSesion(clave)) return
  void cliente.invalidateQueries({ queryKey: CLAVE_DE_SESION })
}

/** Crea el cliente con la reacción global al `401`. */
export function crearClienteDeConsultas(): QueryClient {
  // `cliente` se referencia dentro de los manejadores, que solo corren cuando ya
  // está asignado: no hay uso antes de tiempo.
  const cliente: QueryClient = new QueryClient({
    queryCache: new QueryCache({
      onError: (error, query) => {
        anteUnError(error, query.queryKey, cliente)
      },
    }),
    mutationCache: new MutationCache({
      onError: (error, _variables, _contexto, mutation) => {
        anteUnError(error, mutation.options.mutationKey, cliente)
      },
    }),
  })

  return cliente
}
