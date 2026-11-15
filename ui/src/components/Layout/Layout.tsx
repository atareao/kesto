import { useState } from 'react'
import type { ReactNode } from 'react'
import { Button, Layout as LayoutDeAntd, Menu, Typography, theme } from 'antd'
import {
  CameraOutlined,
  DashboardOutlined,
  HistoryOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  ScheduleOutlined,
  SettingOutlined,
  UserOutlined,
} from '@ant-design/icons'
import { Link, Outlet, useLocation } from 'react-router-dom'
import { ConmutadorDeTema } from '../ConmutadorDeTema/ConmutadorDeTema'
import { useTema } from '../../services/useTema'
import iconoClaro from '../../assets/kesto-icon.png'
import iconoOscuro from '../../assets/kesto-icon-dark.png'

/**
 * El armazón del panel: menú lateral con las pantallas del §6, cabecera con el
 * conmutador de tema y el botón de plegado, y el `main` donde se pinta la ruta
 * activa.
 *
 * Accesibilidad (§3.5): la navegación es un `nav` con nombre accesible, el
 * contenido vive en un `main` enfocable (`tabIndex={-1}`) al que apunta el
 * enlace de salto, y el botón de plegado anuncia su estado con `aria-expanded`.
 * El foco visible lo pone `estilos.css` con `:focus-visible`.
 *
 * Tema (§ identidad visual y tema): la cabecera y el lateral toman sus colores
 * del tema activo (`theme.useToken()`), sin ningún color fijado a mano, y el
 * logo del lateral cambia de variante según el modo.
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
  { ruta: '/ajustes', etiqueta: 'Ajustes', icono: <SettingOutlined /> },
  { ruta: '/sesion', etiqueta: 'Sesión', icono: <UserOutlined /> },
]

/** ¿La ruta actual corresponde a esta entrada? `/` solo casa exacta. */
function esLaActiva(rutaActual: string, ruta: string): boolean {
  if (ruta === '/') return rutaActual === '/'
  return rutaActual === ruta || rutaActual.startsWith(`${ruta}/`)
}

export function Layout() {
  const { pathname } = useLocation()
  const [plegado, setPlegado] = useState(false)
  const { token } = theme.useToken()
  const { modo } = useTema()
  const esOscuro = modo === 'oscuro'

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
          <img className="logo-del-panel" data-testid="logo-del-panel" src={logoDelPanel} alt="" />
          <Typography.Text strong style={{ color: token.colorText }}>
            Kesto
          </Typography.Text>
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
      </LayoutDeAntd.Sider>

      <LayoutDeAntd>
        <LayoutDeAntd.Header
          className="cabecera-del-panel"
          style={{
            background: token.colorBgContainer,
            borderBottom: `1px solid ${token.colorSplit}`,
          }}
        >
          <Button
            type="text"
            aria-label={plegado ? 'Desplegar el menú' : 'Plegar el menú'}
            aria-expanded={!plegado}
            aria-controls="menu-principal"
            onClick={() => setPlegado((previo) => !previo)}
            icon={plegado ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
          />
          <ConmutadorDeTema className="conmutador-de-tema" />
        </LayoutDeAntd.Header>
        {/* `Layout.Content` ya se pinta como `<main>`: el id y el foco van en él. */}
        <LayoutDeAntd.Content id="contenido" tabIndex={-1} className="contenido-del-panel">
          <Outlet />
        </LayoutDeAntd.Content>
      </LayoutDeAntd>
    </LayoutDeAntd>
  )
}

