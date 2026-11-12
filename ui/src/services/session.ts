/**
 * Sesión del panel.
 *
 * El modo lo decide el backend: `GET /api/v1/health` publica `auth_mode`.
 *
 * - **`disabled`** (desarrollo, sin proveedor de identidad): **no hay capa de
 *   sesión**. Las rutas `/api/v1/auth/*` **no existen** —contestan `404`, no
 *   `401`—, así que el panel no puede llamarlas. Aquí no se llama a ninguna.
 * - **`oidc`**: `GET /api/v1/auth/session` devuelve `{sub, email}` o `401` si no
 *   hay sesión. Un `401` **no se reintenta**: es una respuesta, no un fallo
 *   pasajero, y reintentarlo sería un bucle de peticiones.
 *
 * El login y el callback son navegaciones (`302`), no peticiones de datos: se
 * enlazan, no se hacen con `fetch`. La salida sí es una petición: `POST`.
 */

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { UseMutationResult, UseQueryResult } from '@tanstack/react-query'
import { peticion } from './api/client'
import { esErrorDeSesion } from './api/envelope'
import type { Health, ModoDeAutenticacion, Sesion } from './api/types'

/** Clave de caché de `/health`. */
export const CLAVE_DE_SALUD = ['salud'] as const

/** Clave de caché de la sesión. */
export const CLAVE_DE_SESION = ['sesion'] as const

/** `/api/v1/health` — versión y modo de autenticación. */
export function obtenerSalud(): Promise<Health> {
  return peticion<Health>('/health')
}

/** `/api/v1/auth/session` — solo tiene sentido en modo `oidc`. */
export function obtenerSesion(): Promise<Sesion> {
  return peticion<Sesion>('/auth/session')
}

/** `/api/v1/auth/logout` — la salida; desemboca en un `401` en la siguiente lectura. */
export function cerrarLaSesion(): Promise<null> {
  return peticion<null>('/auth/logout', { method: 'POST' })
}

export interface EstadoDeSesion {
  /** `null` mientras `/health` no ha contestado. */
  modo: ModoDeAutenticacion | null
  sesion: Sesion | null
  cargando: boolean
  /** Hay proveedor de identidad y no hay sesión: toca enseñar el botón de entrar. */
  sinSesion: boolean
  error: Error | null
  /** Vuelve a preguntar la sesión (por ejemplo, tras entrar en otra pestaña). */
  refrescar: () => void
}

/** `/health` no cambia durante la vida de la pestaña: no hace falta repetirlo. */
export function useSalud(): UseQueryResult<Health, Error> {
  return useQuery({
    queryKey: CLAVE_DE_SALUD,
    queryFn: obtenerSalud,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  })
}

/**
 * El estado de la sesión del panel, listo para pintar. En modo `disabled`
 * devuelve `modo: 'disabled'` y **no** llama a `/auth/*`.
 */
export function useSesion(): EstadoDeSesion {
  const cliente = useQueryClient()
  const salud = useSalud()
  const modo = salud.data?.auth_mode ?? null

  const consultaDeSesion = useQuery({
    queryKey: CLAVE_DE_SESION,
    queryFn: obtenerSesion,
    enabled: modo === 'oidc',
    // Un `401` es una respuesta, no un fallo pasajero: ni se reintenta en la
    // consulta…
    retry: false,
    // …ni al montar otro observador. Sin esto, el desvío a `/sesion` desmonta la
    // guarda y monta la pantalla, y la relectura volvía a pedir la sesión: dos
    // peticiones donde la spec quiere una. Un `401` es definitivo hasta que algo
    // lo invalide (`refrescar`, la salida o el `401` de `clienteDeConsultas`).
    retryOnMount: false,
    // Con sesión válida, tampoco se relee por el mero hecho de que otro
    // componente la observe: los cambios de sesión son por evento, no por montaje.
    refetchOnMount: false,
  })

  // Un 401 no es un error que enseñar: es «no hay sesión».
  const error = salud.error ?? (consultaDeSesion.error ?? null)
  const esSinSesion = esErrorDeSesion(consultaDeSesion.error)

  return {
    modo,
    // Tras un `401` de la relectura, TanStack Query conserva el `data` anterior:
    // `sinSesion` manda y la sesión se publica como ausente, no obsoleta.
    sesion: esSinSesion ? null : (consultaDeSesion.data ?? null),
    cargando: salud.isPending || (modo === 'oidc' && consultaDeSesion.isPending),
    sinSesion: esSinSesion,
    error: esSinSesion ? null : error,
    refrescar: () => {
      void cliente.invalidateQueries({ queryKey: CLAVE_DE_SESION })
    },
  }
}

/** Cierra la sesión y vuelve a leerla, que es lo que deja `sinSesion: true`. */
export function useCerrarSesion(): UseMutationResult<null, Error, void> {
  const cliente = useQueryClient()

  return useMutation<null, Error, void>({
    mutationFn: cerrarLaSesion,
    onSuccess: () => {
      void cliente.invalidateQueries({ queryKey: CLAVE_DE_SESION })
    },
  })
}
