import { Alert, Button, Flex, Typography } from 'antd'
import { Cargando } from '../components/Cargando/Cargando'
import { useSesion } from '../services/session'
import Dashboard from './dashboard'
import emblema from '../assets/kesto-emblema.png'

/**
 * Portada pública (§6): la primera pantalla que ve quien no tiene sesión.
 *
 * Decide con `useSesion()` —que a su vez lee `/health` y `/auth/session`— si
 * toca enseñar el panel o la portada:
 *
 * - `disabled` o `oidc` con sesión → panel.
 * - `oidc` sin sesión → portada con la marca de Kesto y el botón de entrar, que
 *   navega a `/api/v1/auth/login` (la redirección la hace el servidor).
 * - Mientras `/health` o la sesión no contestan → carga.
 * - Si `/health` falla → error.
 *
 * No hace peticiones propias: solo las de `useSesion()`.
 */
export default function Portada() {
  const { modo, sesion, cargando, error } = useSesion()

  if (cargando) {
    return <Cargando etiqueta="Comprobando el servidor…" />
  }

  if (error !== null) {
    return (
      <Alert
        type="error"
        showIcon
        message="No se pudo contactar con el servidor"
        description="Revisa tu conexión y vuelve a intentarlo."
      />
    )
  }

  if (modo === 'disabled' || (modo === 'oidc' && sesion !== null)) {
    return <Dashboard />
  }

  return (
    <Flex vertical align="center" justify="center" gap={16} style={{ minHeight: '60vh' }}>
      {/* Un único emblema, con fondo transparente: se lee en claro y en oscuro.
          `alt=""` porque el nombre «Kesto» va justo debajo como texto. El par
          `width`/`height` declara la proporción del asset (256×256, cuadrado) para que el
          hueco se reserve antes de cargar: sin él, el logo empujaría el texto. */}
      <img
        data-testid="logo-de-la-portada"
        src={emblema}
        alt=""
        width={256}
        height={256}
        style={{ width: 128, height: 'auto' }}
      />
      <Typography.Title level={1} style={{ margin: 0 }}>
        Kesto
      </Typography.Title>
      <Typography.Paragraph style={{ maxWidth: 480, margin: 0, textAlign: 'center' }}>
        Copias de seguridad cifradas, programadas y verificables. Entra con tu proveedor de
        identidad para ver el estado de tus repositorios y tus últimas copias.
      </Typography.Paragraph>
      <Button type="primary" size="large" href="/api/v1/auth/login">
        Entrar
      </Button>
    </Flex>
  )
}
