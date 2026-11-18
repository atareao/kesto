import { useState } from 'react'
import type { ReactNode } from 'react'
import { App as AntApp, Button, Layout as LayoutDeAntd, Menu, Typography, theme } from 'antd'
import {
  CameraOutlined,
  DashboardOutlined,
  HistoryOutlined,
  LogoutOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  RollbackOutlined,
  ScheduleOutlined,
  SettingOutlined,
} from '@ant-design/icons'
import { Link, Outlet, useLocation, useNavigate } from 'react-router-dom'
import { ConmutadorDeTema } from '../ConmutadorDeTema/ConmutadorDeTema'
import { useTema } from '../../services/useTema'
import { mensajeDeError } from '../../services/errores'
import { useCerrarSesion, useSesion } from '../../services/session'
import iconoClaro from '../../assets/kesto-icon.png'
import iconoOscuro from '../../assets/kesto-icon-dark.png'

/**
 * El armazón del panel: menú lateral con las pantallas del §6, pie de la barra
 * lateral con la identidad de la sesión, el conmutador de tema, el botón de
 * cierre de sesión y el de plegado, y el `main` donde se pinta la ruta activa.
 *
 * El chasis solo existe cuando hay panel: la barra lateral —y con ella el enlace
 * de salto— se monta en modo `disabled` o en `oidc` con sesión. Sin sesión, o
 * mientras el modo/sesión se resuelven, no se monta: el visitante ve la portada
 * pública sin barra lateral y sin parpadeo.
 *
 * La identidad de la sesión (`sub` y `email`) y el cierre de sesión solo tienen
 * sentido con OIDC: en `disabled` no hay capa de sesión y `/api/v1/auth/*`
 * contesta `404`, así que ni se pinta la identidad ni se ofrece salir.
 *
 * Accesibilidad (§3.5): la navegación es un `nav` con nombre accesible, el
 * contenido vive en un `main` enfocable (`tabIndex={-1}`) al que apunta el
 * enlace de salto, y el botón de plegado anuncia su estado con `aria-expanded`.
 * El foco visible lo pone `estilos.css` con `:focus-visible`.
 *
 * Tema (§ identidad visual y tema): el lateral y su pie toman sus colores del
 * tema activo (`theme.useToken()`), sin ningún color fijado a mano, y el logo
 * del lateral cambia de variante según el modo. La marca (logo + nombre) vive
 * sobre el menú; con la barra plegada solo queda el logo, porque el nombre se
 * deja de renderizar, no se esconde por CSS.
 */

interface EntradaDelMenu {
  ruta: string
  etiqueta: string
  icono: ReactNode
}

const ENTRADAS: readonly EntradaDelMenu[] = [
  { ruta: '/', etiqueta: 'Dashboard', icono: <DashboardOutlined /> },
  { ruta: '/tareas', etiqueta: 'Tareas', icono: <ScheduleOutlined /> },
  { ruta: '/historico', etiqueta: 'Histórico', icono: <HistoryOutlined /> },
  { ruta: '/snapshots', etiqueta: 'Snapshots', icono: <CameraOutlined /> },
  { ruta: '/restauraciones', etiqueta: 'Restauraciones', icono: <RollbackOutlined /> },
  { ruta: '/ajustes', etiqueta: 'Ajustes', icono: <SettingOutlined /> },
]

/** ¿La ruta actual corresponde a esta entrada? `/` solo casa exacta. */
function esLaActiva(rutaActual: string, ruta: string): boolean {
  if (ruta === '/') return rutaActual === '/'
  return rutaActual === ruta || rutaActual.startsWith(`${ruta}/`)
}

export function Layout() {
  const { pathname } = useLocation()
  const navegar = useNavigate()
  const [plegado, setPlegado] = useState(false)
  const { token } = theme.useToken()
  const { modo } = useTema()
  const { message: mensajes } = AntApp.useApp()
  const { modo: modoDeAutenticacion, sesion, cargando } = useSesion()
  const cierre = useCerrarSesion()
  const esOscuro = modo === 'oscuro'

  // Hay sesión OIDC: de ahí salen la identidad y el cierre. En `disabled` no hay
  // capa de sesión y `/api/v1/auth/*` contesta `404`, así que no se ofrece nada.
  const conSesion = modoDeAutenticacion === 'oidc' && sesion !== null

  // Hay panel —y por tanto chasis— en `disabled` o en `oidc` con sesión. Mientras
  // el modo o la sesión no contestan tampoco se monta: enseñar la barra «por si
  // acaso» sería enseñar el panel a quien todavía no ha entrado.
  const hayPanel = !cargando && (modoDeAutenticacion === 'disabled' || conSesion)

  const activa = ENTRADAS.find((entrada) => esLaActiva(pathname, entrada.ruta))

  const entradas = ENTRADAS.map((entrada) => ({
    key: entrada.ruta,
    icon: entrada.icono,
    label: (
      <Link
        to={entrada.ruta}
        aria-current={esLaActiva(pathname, entrada.ruta) ? 'page' : undefined}
      >
        {entrada.etiqueta}
      </Link>
    ),
  }))

  // El logo va sobre la superficie del lateral; cada variante se lee sobre su fondo.
  const logoDelPanel = esOscuro ? iconoOscuro : iconoClaro
  const colorDeSuperficie = token.colorBgContainer
  const temaDelMenu = esOscuro ? 'dark' : 'light'

  return (
    <LayoutDeAntd style={{ minHeight: '100vh' }}>
      {hayPanel ? (
        <>
          <a className="enlace-de-salto" href="#contenido">
            Saltar al contenido
          </a>
          <LayoutDeAntd.Sider
            collapsible
            collapsed={plegado}
            trigger={null}
            breakpoint="lg"
            onBreakpoint={setPlegado}
            theme={temaDelMenu}
            width={220}
            style={{ background: colorDeSuperficie }}
          >
            <div className="marca-del-panel">
              <img
                className="logo-del-panel"
                data-testid="logo-del-panel"
                src={logoDelPanel}
                alt=""
              />
              {!plegado ? (
                <Typography.Text strong style={{ color: token.colorText }}>
                  Kesto
                </Typography.Text>
              ) : null}
            </div>
            <nav id="menu-principal" aria-label="Principal">
              <Menu
                mode="inline"
                theme={temaDelMenu}
                style={{ background: 'transparent' }}
                selectedKeys={activa === undefined ? [] : [activa.ruta]}
                items={entradas}
              />
            </nav>
            {/* Los mandos del chasis cierran la barra lateral, pegados al fondo. */}
            <div className="pie-del-panel">
              {/* Identidad de la sesión: el correo a la vista y el `sub` en el
                  título del bloque. Solo con la barra desplegada: con la `Sider`
                  a 80 px el pie va en columna y el correo se desbordaría. */}
              {conSesion && !plegado ? (
                <div
                  className="identidad-de-la-sesion"
                  data-testid="identidad-de-la-sesion"
                  title={sesion.sub}
                >
                  {sesion.email}
                </div>
              ) : null}
              <ConmutadorDeTema />
              {conSesion ? (
                <Button
                  type="text"
                  aria-label="Cerrar sesión"
                  loading={cierre.isPending}
                  onClick={() =>
                    cierre.mutate(undefined, {
                      onSuccess: () => navegar('/'),
                      onError: (error) => mensajes.error(mensajeDeError(error)),
                    })
                  }
                  icon={<LogoutOutlined />}
                />
              ) : null}
              <Button
                type="text"
                aria-label={plegado ? 'Desplegar el menú' : 'Plegar el menú'}
                aria-expanded={!plegado}
                aria-controls="menu-principal"
                onClick={() => setPlegado((previo) => !previo)}
                icon={plegado ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
              />
            </div>
          </LayoutDeAntd.Sider>
        </>
      ) : null}

      <LayoutDeAntd>
        {/* `Layout.Content` ya se pinta como `<main>`: el id y el foco van en él. */}
        <LayoutDeAntd.Content id="contenido" tabIndex={-1} className="contenido-del-panel">
          <Outlet />
        </LayoutDeAntd.Content>
      </LayoutDeAntd>
    </LayoutDeAntd>
  )
}

