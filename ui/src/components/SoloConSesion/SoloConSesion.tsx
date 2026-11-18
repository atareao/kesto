import { Navigate, Outlet } from 'react-router-dom'
import { Cargando } from '../Cargando/Cargando'
import { useSesion } from '../../services/session'

/**
 * La guarda de la capa de sesión (§3.4): envuelve las rutas que exigen sesión.
 *
 * - **`disabled`** (desarrollo): no hay capa de sesión; entra directo y **no** se
 *   llama a ninguna ruta `/auth/*`.
 * - **`oidc` con sesión:** pinta la ruta pedida.
 * - **`oidc` sin sesión (o `/health` sin contestar):** desvía a `/`, que es la
 *   portada pública donde se ofrece entrar. Un `401` de cualquier petición
 *   descarta la caché de la sesión (`clienteDeConsultas`), su relectura vuelve a
 *   dar `401` y esta guarda devuelve al visitante a la portada.
 *
 * Mientras `/health` y la sesión están en el aire se muestra la carga: enseñar el
 * panel «por si acaso» sería enseñarlo sin sesión.
 */
export function SoloConSesion() {
  const { modo, sesion, cargando } = useSesion()

  if (cargando) {
    return <Cargando etiqueta="Comprobando la sesión…" />
  }

  if (modo === 'disabled') {
    return <Outlet />
  }

  if (modo === 'oidc' && sesion !== null) {
    return <Outlet />
  }

  return <Navigate to="/" replace />
}
