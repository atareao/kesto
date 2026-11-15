import { render, screen, waitFor, within } from '@testing-library/react'
import type { UserEvent } from '@testing-library/user-event'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { ProveedorDeTema } from '../../services/tema'
import { Layout } from './Layout'

/**
 * Pruebas del layout: accesibilidad (§3.5), menú plegable en ventana estrecha
 * (§3.3), marca del panel y controles del chasis en el pie de la barra lateral
 * (§ identidad visual y tema). El layout se monta dentro de un router porque usa
 * `Link`, `Outlet` y `useLocation`, y dentro del proveedor de tema porque el pie,
 * el logo y el conmutador dependen del modo activo.
 */
function montarLayout() {
  return render(
    <ProveedorDeTema>
      <MemoryRouter initialEntries={['/']}>
        <Routes>
          <Route element={<Layout />}>
            <Route index element={<p>contenido de la pantalla</p>} />
          </Route>
        </Routes>
      </MemoryRouter>
    </ProveedorDeTema>,
  )
}

const PANTALLAS = ['Dashboard', 'Tareas', 'Histórico', 'Snapshots', 'Ajustes'] as const

/**
 * El ancho de ventana es estado global del entorno: varias pruebas lo fijan para
 * simular una ventana estrecha y el `Sider` decide con él si arranca plegado. Se
 * guarda el valor previo —el que traiga jsdom, sin dar por hecho su defecto— y se
 * restaura al terminar cada prueba para no contaminar las siguientes.
 */
let anchoDeVentanaPrevio: number

beforeEach(() => {
  anchoDeVentanaPrevio = window.innerWidth
})

afterEach(() => {
  window.innerWidth = anchoDeVentanaPrevio
})

/**
 * El pie de la barra lateral: el contenedor `.pie-del-panel` que agrupa los
 * mandos del chasis (conmutador de tema y control de plegado) y que va a
 * continuación del menú, dentro de la `Sider`. Se obtiene con aserción para que,
 * mientras el pie no exista, el test lo diga con un fallo explícito en vez de
 * reventar con un `TypeError` al usar un `null`.
 */
function pieDeLaBarraLateral(): HTMLElement {
  const pie = document.querySelector<HTMLElement>('.pie-del-panel')
  expect(pie, 'no se encontró el pie de la barra lateral (.pie-del-panel)').not.toBeNull()
  if (pie === null) throw new Error('no se encontró el pie de la barra lateral (.pie-del-panel)')
  return pie
}

/**
 * Recorre con Tab hasta poner el foco en `destino` y devuelve si lo alcanzó.
 * Comprueba que un control es alcanzable con el teclado sin depender del orden
 * exacto de los elementos tabulables del armazón.
 *
 * La cota no es un número de pasos a ojo: el barrido termina cuando el foco
 * vuelve a un elemento ya visitado —o se queda sin foco—, que es la señal de que
 * se recorrió el ciclo completo de tabulación del documento. Así el límite real
 * es el propio orden de tabulación (una decena de elementos aquí), y no un 40
 * arbitrario que ni garantiza alcanzar el destino ni evita dar pasos de más.
 */
async function alcanzarConTab(usuario: UserEvent, destino: HTMLElement): Promise<boolean> {
  const yaVisitados = new Set<Element>()

  for (;;) {
    await usuario.tab()

    const enfocado = document.activeElement
    if (enfocado === destino) return true
    // Ciclo cerrado: el foco repite posición (normalmente `<body>`, al salir del
    // último tabulable) y el destino no estaba en el recorrido.
    if (enfocado === null || document.body === enfocado || yaVisitados.has(enfocado)) {
      return false
    }
    yaVisitados.add(enfocado)
  }
}

describe('accesibilidad del layout', () => {
  it('tiene navegación con nombre accesible y un main', () => {
    montarLayout()

    expect(screen.getByRole('navigation', { name: 'Principal' })).toBeInTheDocument()

    const principal = screen.getByRole('main')
    expect(principal).toHaveAttribute('id', 'contenido')
    // Enfocable por programa: el enlace de salto puede llevar el foco aquí.
    expect(principal).toHaveAttribute('tabindex', '-1')
  })

  it('ofrece un enlace de salto al contenido', () => {
    montarLayout()

    const salto = screen.getByRole('link', { name: 'Saltar al contenido' })
    expect(salto).toHaveAttribute('href', '#contenido')
  })

  it('el botón de plegado anuncia su estado con aria-expanded', () => {
    montarLayout()

    const boton = screen.getByRole('button', { name: 'Plegar el menú' })
    expect(boton).toHaveAttribute('aria-expanded', 'true')
  })
})

describe('los mandos del chasis viven en el pie de la barra lateral', () => {
  it('sitúa el control de plegado dentro de la barra lateral, a continuación del menú', () => {
    montarLayout()

    const menu = screen.getByRole('navigation', { name: 'Principal' })
    const pie = pieDeLaBarraLateral()

    // Mismo contenedor que el menú: el pie está dentro de la barra lateral.
    expect(pie.parentElement).toBe(menu.parentElement)
    // Y a continuación del menú en el documento.
    expect(menu.compareDocumentPosition(pie) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0)

    // El control de plegado vive en el pie y conserva nombre accesible y estado.
    const boton = within(pie).getByRole('button', { name: 'Plegar el menú' })
    expect(boton).toHaveAttribute('aria-expanded', 'true')
    expect(boton).toHaveAttribute('aria-controls', 'menu-principal')

    // Ya no queda una cabecera del panel con los controles.
    expect(boton.closest('.ant-layout-header')).toBeNull()
    expect(document.querySelector('.cabecera-del-panel')).toBeNull()
  })

  it('deja el control de plegado alcanzable y activable con el teclado', async () => {
    const usuario = userEvent.setup()
    montarLayout()

    const boton = within(pieDeLaBarraLateral()).getByRole('button', { name: 'Plegar el menú' })

    expect(await alcanzarConTab(usuario, boton)).toBe(true)

    // Con el foco en el control, Intro lo pliega: cambian el nombre y el estado.
    await usuario.keyboard('{Enter}')
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Desplegar el menú' })).toHaveAttribute(
        'aria-expanded',
        'false',
      ),
    )
  })
})

describe('tema del layout', () => {
  afterEach(() => {
    window.localStorage.clear()
    delete document.documentElement.dataset.tema
  })

  it('arranca en oscuro y muestra el conmutador en el pie de la barra lateral', () => {
    montarLayout()

    expect(document.documentElement.dataset.tema).toBe('oscuro')

    const conmutador = within(pieDeLaBarraLateral()).getByRole('switch', {
      name: 'Cambiar a tema claro',
    })
    expect(conmutador).toBeInTheDocument()

    // Ni el conmutador ni el plegado viven ya en una cabecera.
    expect(conmutador.closest('.ant-layout-header')).toBeNull()
    expect(document.querySelector('.cabecera-del-panel')).toBeNull()
  })

  it('deja el conmutador de tema alcanzable con el teclado desde el pie', async () => {
    const usuario = userEvent.setup()
    montarLayout()

    const conmutador = within(pieDeLaBarraLateral()).getByRole('switch', {
      name: 'Cambiar a tema claro',
    })

    expect(await alcanzarConTab(usuario, conmutador)).toBe(true)
  })

  it('muestra el logo de Kesto en el menú lateral', () => {
    montarLayout()

    expect(screen.getByTestId('logo-del-panel')).toBeInTheDocument()
  })
})

describe('menú plegable en ventana estrecha', () => {
  it('arranca plegado y se despliega sin dejar entradas inalcanzables', async () => {
    const usuario = userEvent.setup()
    // Ventana estrecha: por debajo del punto de ruptura `lg` (992 px).
    window.innerWidth = 640
    montarLayout()

    // El `breakpoint` del `Sider` lo deja plegado al montar.
    const desplegar = await screen.findByRole('button', { name: 'Desplegar el menú' })
    expect(desplegar).toHaveAttribute('aria-expanded', 'false')

    // Con el menú plegado, las cinco pantallas siguen en el DOM y son enlazables.
    const nav = screen.getByRole('navigation', { name: 'Principal' })
    for (const pantalla of PANTALLAS) {
      expect(within(nav).getByRole('link', { name: pantalla })).toBeInTheDocument()
    }

    // Se despliega y se vuelve a plegar.
    await usuario.click(desplegar)
    expect(screen.getByRole('button', { name: 'Plegar el menú' })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
    await usuario.click(screen.getByRole('button', { name: 'Plegar el menú' }))
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Desplegar el menú' })).toHaveAttribute(
        'aria-expanded',
        'false',
      ),
    )
  })
})

describe('marca del panel', () => {
  it('muestra el nombre Kesto junto al logo con la barra lateral desplegada', () => {
    montarLayout()

    const logo = screen.getByTestId('logo-del-panel')
    expect(logo).toBeVisible()

    // El nombre acompaña al logo dentro del bloque de marca.
    const bloqueDeMarca = logo.parentElement as HTMLElement
    expect(within(bloqueDeMarca).getByText('Kesto')).toBeInTheDocument()
  })

  it('no renderiza el nombre Kesto con la barra lateral plegada y mantiene el logo', async () => {
    // Ventana estrecha: el `breakpoint` del `Sider` pliega la barra al montar.
    window.innerWidth = 640
    montarLayout()

    // Espera al plegado de verdad: el nombre accesible del control cambia.
    const desplegar = await screen.findByRole('button', { name: 'Desplegar el menú' })
    expect(desplegar).toHaveAttribute('aria-expanded', 'false')

    // El nombre no se renderiza: no basta con esconderlo por CSS.
    expect(screen.queryByText('Kesto')).toBeNull()
    // El logo sigue visible.
    expect(screen.getByTestId('logo-del-panel')).toBeInTheDocument()
  })
})
