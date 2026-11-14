import { render, screen } from '@testing-library/react'
import { ProveedorDeTema } from '../../services/tema'
import { CLAVE_DE_TEMA } from '../../services/useTema'
import { PanelConTema } from './PanelConTema'

/**
 * Cableado del `ConfigProvider` (§ identidad visual y tema).
 *
 * `App` se sustituye por una sonda que lee un token del `ConfigProvider` que
 * monta `PanelConTema`: así se comprueba que el panel aplica el `ThemeConfig`
 * del modo activo, sin arrastrar rutas, red ni stream. Una mutación de
 * `PanelConTema` que fijase `temaDeAntd('claro')` haría fallar la primera prueba.
 */
vi.mock('../../App', async () => {
  const React = await import('react')
  const { theme } = await import('antd')

  return {
    default: function SondaDeTokens() {
      const { token } = theme.useToken()
      return React.createElement('span', { 'data-testid': 'fondo-del-layout' }, token.colorBgLayout)
    },
  }
})

beforeEach(() => {
  window.localStorage.clear()
  delete document.documentElement.dataset.tema
})

afterEach(() => {
  window.localStorage.clear()
  delete document.documentElement.dataset.tema
})

describe('PanelConTema', () => {
  it('aplica por defecto los tokens del tema oscuro', () => {
    render(
      <ProveedorDeTema>
        <PanelConTema />
      </ProveedorDeTema>,
    )

    expect(screen.getByTestId('fondo-del-layout')).toHaveTextContent('#0D1B2A')
  })

  it('aplica los tokens del tema claro cuando hay una elección guardada', () => {
    window.localStorage.setItem(CLAVE_DE_TEMA, 'claro')

    render(
      <ProveedorDeTema>
        <PanelConTema />
      </ProveedorDeTema>,
    )

    expect(screen.getByTestId('fondo-del-layout')).toHaveTextContent('#F8FAFC')
  })
})
