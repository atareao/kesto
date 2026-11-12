import { Alert, Button, Descriptions, Result, Spin, Typography } from 'antd'
import { useNavigate } from 'react-router-dom'
import { useCerrarSesion, useSesion } from '../services/session'

/**
 * Pantalla de sesión (§3.4). El desenlace lo decide el backend:
 *
 * - **`disabled`** (desarrollo): no hay capa de sesión; se avisa y no se llama a
 *   ninguna ruta `/auth/*`, que en ese modo contesta `404`.
 * - **`oidc` con sesión:** se muestran `sub` y `email`. Nunca un token: el DTO
 *   `Sesion` no lo trae y aquí no se pinta nada más.
 * - **`oidc` sin sesión:** un botón que **navega** a `/api/v1/auth/login` (lo
 *   hace el servidor con un `302`; no es una petición `fetch`).
 * - **Salir:** `POST /auth/logout`, se vacía la caché y se vuelve aquí.
 */
export default function Sesion() {
  const { modo, sesion, cargando, sinSesion, error, refrescar } = useSesion()
  const cierre = useCerrarSesion()
  const navegar = useNavigate()

  if (cargando) {
    return (
      <div role="status" className="centrado">
        <Spin size="large" />
        <p className="sr-only">Comprobando la sesión…</p>
      </div>
    )
  }

  if (error !== null) {
    return (
      <Alert
        type="error"
        showIcon
        message="No se pudo comprobar la sesión"
        description={error.message}
        action={<Button onClick={refrescar}>Reintentar</Button>}
      />
    )
  }

  if (modo === 'disabled') {
    return (
      <Result
        status="info"
        title="Modo de desarrollo: sin autenticación"
        subTitle="El backend arrancó con auth_mode: disabled. No hay proveedor de identidad y el panel no pide sesión."
        extra={
          <Button type="primary" onClick={() => navegar('/')}>
            Ir al panel
          </Button>
        }
      />
    )
  }

  // Tras un `401` de la relectura, TanStack Query conserva el `data` anterior:
  // `sinSesion` manda sobre un `sesion` obsoleto.
  if (modo === 'oidc' && sesion !== null && !sinSesion) {
    return (
      <section aria-labelledby="titulo-de-la-pantalla">
        <Typography.Title id="titulo-de-la-pantalla" level={1} style={{ marginTop: 0 }}>
          Sesión
        </Typography.Title>
        <Descriptions
          column={1}
          bordered
          items={[
            { key: 'sub', label: 'Identificador (sub)', children: sesion.sub },
            { key: 'email', label: 'Correo', children: sesion.email },
          ]}
        />
        <Button
          danger
          style={{ marginTop: 16 }}
          loading={cierre.isPending}
          onClick={() =>
            cierre.mutate(undefined, {
              onSuccess: () => navegar('/sesion'),
            })
          }
        >
          Cerrar sesión
        </Button>
      </section>
    )
  }

  // Modo `oidc` sin sesión: el botón es un enlace real, no un `fetch`.
  return (
    <Result
      status="warning"
      title="No hay sesión"
      subTitle="El panel requiere identificarse con el proveedor de identidad."
      extra={
        <Button type="primary" href="/api/v1/auth/login">
          Entrar
        </Button>
      }
    />
  )
}
