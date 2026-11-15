import { render, renderHook, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { theme } from 'antd'
import { ProveedorDeTema } from './tema'
import {
  CLAVE_DE_TEMA,
  leerTemaGuardado,
  temaDeAntd,
  useTema,
} from './useTema'

/**
 * Pruebas del tema claro/oscuro (§ identidad visual y tema):
 *
 * - El panel arranca **oscuro** salvo que haya una elección guardada.
 * - La elección se persiste en `localStorage` (`kesto-tema`).
 * - El hook `useTema` no puede usarse fuera del proveedor.
 * - Los tokens de Antd salen de la paleta de marca según el modo.
 */

/** Sonda: pinta el modo activo y expone un botón que alterna. */
function Sonda() {
  const { modo, alternar } = useTema()
  return (
    <div>
      <span data-testid="modo">{modo}</span>
      <button type="button" onClick={alternar}>
        alternar
      </button>
    </div>
  )
}

beforeEach(() => {
  window.localStorage.clear()
  delete document.documentElement.dataset.tema
})

afterEach(() => {
  window.localStorage.clear()
  delete document.documentElement.dataset.tema
})

describe('leerTemaGuardado', () => {
  it('sin nada guardado devuelve oscuro', () => {
    expect(leerTemaGuardado()).toBe('oscuro')
  })

  it('respeta una elección de claro guardada', () => {
    window.localStorage.setItem(CLAVE_DE_TEMA, 'claro')
    expect(leerTemaGuardado()).toBe('claro')
  })

  it('ignora un valor no válido y cae a oscuro', () => {
    window.localStorage.setItem(CLAVE_DE_TEMA, 'azul')
    expect(leerTemaGuardado()).toBe('oscuro')
  })
})

describe('ProveedorDeTema', () => {
  it('arranca en oscuro y lo refleja en el documento', () => {
    render(
      <ProveedorDeTema>
        <Sonda />
      </ProveedorDeTema>,
    )

    expect(screen.getByTestId('modo')).toHaveTextContent('oscuro')
    expect(document.documentElement.dataset.tema).toBe('oscuro')
  })

  it('respeta el tema claro guardado al arrancar', () => {
    window.localStorage.setItem(CLAVE_DE_TEMA, 'claro')
    render(
      <ProveedorDeTema>
        <Sonda />
      </ProveedorDeTema>,
    )

    expect(screen.getByTestId('modo')).toHaveTextContent('claro')
    expect(document.documentElement.dataset.tema).toBe('claro')
  })

  it('alternar cambia el modo y lo persiste en localStorage', async () => {
    const usuario = userEvent.setup()
    render(
      <ProveedorDeTema>
        <Sonda />
      </ProveedorDeTema>,
    )

    await usuario.click(screen.getByRole('button', { name: 'alternar' }))

    expect(screen.getByTestId('modo')).toHaveTextContent('claro')
    expect(window.localStorage.getItem(CLAVE_DE_TEMA)).toBe('claro')
    expect(document.documentElement.dataset.tema).toBe('claro')
  })

  it('mantiene el theme-color alineado con el fondo del modo activo', async () => {
    const meta = document.createElement('meta')
    meta.setAttribute('name', 'theme-color')
    document.head.appendChild(meta)

    try {
      const usuario = userEvent.setup()
      render(
        <ProveedorDeTema>
          <Sonda />
        </ProveedorDeTema>,
      )

      // Oscuro por defecto: el fondo de marca oscuro.
      expect(meta.getAttribute('content')).toBe('#0D1B2A')

      await usuario.click(screen.getByRole('button', { name: 'alternar' }))
      expect(meta.getAttribute('content')).toBe('#F8FAFC')
    } finally {
      meta.remove()
    }
  })
})

describe('useTema', () => {
  it('lanza un error si se usa fuera del proveedor', () => {
    const silencio = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    expect(() => renderHook(() => useTema())).toThrow(/proveedor/i)
    silencio.mockRestore()
  })
})

describe('temaDeAntd', () => {
  it('usa el algoritmo oscuro y los fondos de marca en oscuro', () => {
    const config = temaDeAntd('oscuro')

    expect(config.algorithm).toBe(theme.darkAlgorithm)
    expect(config.token?.colorPrimary).toBe('#2194AA')
    expect(config.token?.colorSuccess).toBe('#7EBC50')
    expect(config.token?.colorWarning).toBe('#F08724')
    expect(config.token?.colorInfo).toBe('#33998D')
    expect(config.token?.colorBgLayout).toBe('#0D1B2A')
    expect(config.token?.colorBgContainer).toBe('#123048')
  })

  it('usa el algoritmo por defecto y los fondos claros en claro', () => {
    const config = temaDeAntd('claro')

    expect(config.algorithm).toBe(theme.defaultAlgorithm)
    expect(config.token?.colorPrimary).toBe('#2194AA')
    expect(config.token?.colorBgLayout).toBe('#F8FAFC')
    expect(config.token?.colorBgContainer).toBe('#FFFFFF')
  })
})

/**
 * Contraste (WCAG 2.1) de los pares que la spec exige legibles en **ambos**
 * temas. Se calcula a partir de los tokens, sin navegador. Los pares
 * comprobados:
 *
 * 1. Enlace de marca (`colorLink`) sobre el fondo del layout (`colorBgLayout`).
 * 2. Enlace de marca sobre la superficie de contenido (`colorBgContainer`).
 * 3. Texto de la entrada de menú **seleccionada** sobre su fondo
 *    (`Menu.item*`/`darkItem*Selected*`).
 * 4. Texto del botón sólido sobre el cyan de marca (`Button.primaryColor` sobre
 *    `colorPrimary`).
 */
function luminancia(hex: string): number {
  const canales = [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16) / 255)
  const [r, g, b] = canales.map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4))
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

function razonDeContraste(frente: string, fondo: string): number {
  const [claro, oscuro] = [luminancia(frente), luminancia(fondo)].sort((a, b) => b - a)
  return (claro + 0.05) / (oscuro + 0.05)
}

const AA = 4.5

describe('contraste AA de los tokens por modo', () => {
  it.each(['oscuro', 'claro'] as const)('los pares de marca de «%s» superan 4.5:1', (modo) => {
    const config = temaDeAntd(modo)
    const token = config.token
    const menu = config.components?.Menu

    const fondoLayout = token?.colorBgLayout as string
    const fondoContenido = token?.colorBgContainer as string
    const esOscuro = modo === 'oscuro'

    const pares: ReadonlyArray<[string, string | undefined, string | undefined]> = [
      ['enlace sobre el fondo del layout', token?.colorLink, fondoLayout],
      ['enlace sobre el contenido', token?.colorLink, fondoContenido],
      [
        'entrada de menú seleccionada',
        esOscuro ? menu?.darkItemSelectedColor : menu?.itemSelectedColor,
        esOscuro ? menu?.darkItemSelectedBg : menu?.itemSelectedBg,
      ],
      ['texto del botón sólido', config.components?.Button?.primaryColor, token?.colorPrimary],
    ]

    for (const [nombre, frente, fondo] of pares) {
      expect(frente, `${modo}: falta el color de ${nombre}`).toBeDefined()
      expect(fondo, `${modo}: falta el fondo de ${nombre}`).toBeDefined()
      expect(
        razonDeContraste(frente as string, fondo as string),
        `${modo} — ${nombre} (${frente} sobre ${fondo})`,
      ).toBeGreaterThanOrEqual(AA)
    }
  })
})
