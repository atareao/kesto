import { render, screen, waitFor, within } from '@testing-library/react'
import type { UserEvent } from '@testing-library/user-event'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { App as AntApp } from 'antd'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { instalarFetchFalso } from '../../../tests/fetchDePrueba'
import { ProveedorDeTema } from '../../services/tema'
import { Layout } from './Layout'

/**
 * Pruebas del layout: accesibilidad (§3.5), menú plegable en ventana estrecha
 * (§3.3), marca del panel, identidad de la sesión y controles del chasis en el
 * pie de la barra lateral (§ identidad visual y tema). El layout se monta dentro
 * de un router porque usa `Link`, `Outlet` y `useLocation`, dentro del proveedor
 * de tema porque el pie, el logo y el conmutador dependen del modo activo, y
 * dentro del cliente de consultas porque la barra lateral solo se monta cuando
 * `useSesion()` dice que hay panel (`disabled` o `oidc` con sesión).
 *
 * El `App` de antd envuelve el árbol porque el cierre de sesión avisa de sus
 * fallos con `App.useApp().message`, que solo existe bajo ese proveedor.
 *
 * `/health`, `/auth/session` y `/auth/logout` se doblan: por defecto el modo es
 * `disabled` —el de desarrollo—, que es el que mantiene la barra visible para
 * las pruebas de plegado, tema y marca.
 */
type ModoDePrueba = 'disabled' | 'oidc'
type EstadoDeSesion = 'con' | 'sin' | 'pendiente'
const SESION = { sub: 'ana', email: 'ana@example.org' } as const

function montarLayout({
  modo = 'disabled',
  sesion = 'sin',
  fallaElCierre = false,
}: {
  modo?: ModoDePrueba
  sesion?: EstadoDeSesion
  fallaElCierre?: boolean
} = {}) {
  const fetch = instalarFetchFalso(({ ruta }) => {
    if (ruta === '/api/v1/health') {
      return { status: 200, data: { version: '0.1.0', auth_mode: modo } }
    }
    if (ruta === '/api/v1/auth/session') {
      if (sesion === 'pendiente') return new Promise<never>(() => {})
      if (sesion === 'con') return { status: 200, data: SESION }
      return { status: 401, message: 'no hay sesión' }
    }
    if (ruta === '/api/v1/auth/logout') {
      if (fallaElCierre) return { status: 500, message: 'no se pudo cerrar la sesión' }
      return { status: 200, data: null }
    }
    throw new Error(`ruta inesperada: ${ruta}`)
  })
  const cliente = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  })
  const resultado = render(
    <ProveedorDeTema>
      <QueryClientProvider client={cliente}>
        <AntApp>
          <MemoryRouter initialEntries={['/']}>
            <Routes>
              <Route element={<Layout />}>
                <Route index element={<p>contenido de la pantalla</p>} />
              </Route>
            </Routes>
          </MemoryRouter>
        </AntApp>
      </QueryClientProvider>
    </ProveedorDeTema>,
  )
  return { ...resultado, fetch }
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
  vi.unstubAllGlobals()
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
  it('tiene navegación con nombre accesible y un main', async () => {
    montarLayout()

    expect(await screen.findByRole('navigation', { name: 'Principal' })).toBeInTheDocument()

    const principal = screen.getByRole('main')
    expect(principal).toHaveAttribute('id', 'contenido')
    // Enfocable por programa: el enlace de salto puede llevar el foco aquí.
    expect(principal).toHaveAttribute('tabindex', '-1')
  })

  it('ofrece un enlace de salto al contenido', async () => {
    montarLayout()

    const salto = await screen.findByRole('link', { name: 'Saltar al contenido' })
    expect(salto).toHaveAttribute('href', '#contenido')
  })

  it('el botón de plegado anuncia su estado con aria-expanded', async () => {
    montarLayout()

    const boton = await screen.findByRole('button', { name: 'Plegar el menú' })
    expect(boton).toHaveAttribute('aria-expanded', 'true')
  })
})

describe('los mandos del chasis viven en el pie de la barra lateral', () => {
  it('sitúa el control de plegado dentro de la barra lateral, a continuación del menú', async () => {
    montarLayout()

    const menu = await screen.findByRole('navigation', { name: 'Principal' })
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

    await screen.findByRole('navigation', { name: 'Principal' })
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

  it('arranca en oscuro y muestra el conmutador en el pie de la barra lateral', async () => {
    montarLayout()

    expect(document.documentElement.dataset.tema).toBe('oscuro')

    const conmutador = await screen.findByRole('switch', {
      name: 'Cambiar a tema claro',
    })
    expect(conmutador).toBeInTheDocument()
    expect(within(pieDeLaBarraLateral()).getByRole('switch')).toBe(conmutador)

    // Ni el conmutador ni el plegado viven ya en una cabecera.
    expect(conmutador.closest('.ant-layout-header')).toBeNull()
    expect(document.querySelector('.cabecera-del-panel')).toBeNull()
  })

  it('deja el conmutador de tema alcanzable con el teclado desde el pie', async () => {
    const usuario = userEvent.setup()
    montarLayout()

    const conmutador = await screen.findByRole('switch', {
      name: 'Cambiar a tema claro',
    })

    expect(await alcanzarConTab(usuario, conmutador)).toBe(true)
  })

  it('muestra el logo de Kesto en el menú lateral', async () => {
    montarLayout()

    expect(await screen.findByTestId('logo-del-panel')).toBeInTheDocument()
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
  it('muestra el nombre Kesto junto al logo con la barra lateral desplegada', async () => {
    montarLayout()

    const logo = await screen.findByTestId('logo-del-panel')
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

/**
 * La barra lateral solo es chasis del panel: existe en `disabled` o en `oidc`
 * con sesión, y desaparece sin sesión o mientras el modo/sesión se resuelven.
 */
describe('la barra lateral solo se monta cuando hay panel', () => {
  it('en modo oidc sin sesión no hay menú lateral ni pie de la barra', async () => {
    const { fetch } = montarLayout({ modo: 'oidc', sesion: 'sin' })

    // Se espera a que la sesión se resuelva (401) antes de afirmar la ausencia.
    await waitFor(() => expect(fetch.a('/api/v1/auth/session')).toHaveLength(1))

    expect(screen.queryByRole('navigation', { name: 'Principal' })).toBeNull()
    expect(document.querySelector('.pie-del-panel')).toBeNull()
  })

  it('mientras el modo o la sesión se resuelven tampoco hay menú lateral', () => {
    montarLayout({ modo: 'oidc', sesion: 'pendiente' })

    expect(screen.queryByRole('navigation', { name: 'Principal' })).toBeNull()
    expect(document.querySelector('.pie-del-panel')).toBeNull()
  })

  it('en modo disabled sí hay menú lateral y pie', async () => {
    montarLayout({ modo: 'disabled' })

    expect(await screen.findByRole('navigation', { name: 'Principal' })).toBeInTheDocument()
    expect(document.querySelector('.pie-del-panel')).not.toBeNull()
  })

  it('en modo oidc con sesión sí hay menú lateral y pie', async () => {
    montarLayout({ modo: 'oidc', sesion: 'con' })

    expect(await screen.findByRole('navigation', { name: 'Principal' })).toBeInTheDocument()
    expect(document.querySelector('.pie-del-panel')).not.toBeNull()
  })
})

describe('identidad de la sesión en la barra lateral', () => {
  it('con sesión oidc muestra el email y deja el sub en el título', async () => {
    montarLayout({ modo: 'oidc', sesion: 'con' })

    await screen.findByRole('navigation', { name: 'Principal' })
    const identidad = within(pieDeLaBarraLateral()).getByTestId('identidad-de-la-sesion')

    expect(within(identidad).getByText(SESION.email)).toBeInTheDocument()
    expect(identidad).toHaveAttribute('title', SESION.sub)
  })

  it('en modo disabled no hay bloque de identidad', async () => {
    montarLayout({ modo: 'disabled' })

    await screen.findByRole('navigation', { name: 'Principal' })
    expect(screen.queryByTestId('identidad-de-la-sesion')).toBeNull()
  })

  it('con la barra desplegada se ve la identidad y desaparece al plegarla', async () => {
    // Ventana ancha: el `Sider` arranca desplegado y el correo cabe en el pie.
    const usuario = userEvent.setup()
    montarLayout({ modo: 'oidc', sesion: 'con' })

    await screen.findByRole('navigation', { name: 'Principal' })
    expect(within(pieDeLaBarraLateral()).getByTestId('identidad-de-la-sesion')).toBeInTheDocument()

    // Con 80 px de `Sider` un email largo se desborda: al plegar, el bloque va fuera.
    await usuario.click(screen.getByRole('button', { name: 'Plegar el menú' }))

    await waitFor(() =>
      expect(screen.queryByTestId('identidad-de-la-sesion')).toBeNull(),
    )
    // El cierre sigue disponible aunque la identidad no se pinte.
    expect(within(pieDeLaBarraLateral()).getByRole('button', { name: 'Cerrar sesión' })).toBeInTheDocument()
  })
})

describe('cierre de sesión en el pie de la barra lateral', () => {
  it('en modo disabled el pie no ofrece cerrar sesión (no hay /auth/*)', async () => {
    montarLayout({ modo: 'disabled' })

    await screen.findByRole('navigation', { name: 'Principal' })
    expect(within(pieDeLaBarraLateral()).queryByRole('button', { name: 'Cerrar sesión' })).toBeNull()
  })

  it('con sesión oidc ofrece «Cerrar sesión» en el pie y llama a POST /api/v1/auth/logout', async () => {
    const usuario = userEvent.setup()
    const { fetch } = montarLayout({ modo: 'oidc', sesion: 'con' })

    await screen.findByRole('navigation', { name: 'Principal' })

    const boton = within(pieDeLaBarraLateral()).getByRole('button', { name: 'Cerrar sesión' })

    await usuario.click(boton)

    await waitFor(() => expect(fetch.a('/api/v1/auth/logout')).toHaveLength(1))
    expect(fetch.a('/api/v1/auth/logout')[0]?.method).toBe('POST')
  })

  it('si el cierre falla muestra el mensaje de la envoltura', async () => {
    const usuario = userEvent.setup()
    montarLayout({ modo: 'oidc', sesion: 'con', fallaElCierre: true })

    await screen.findByRole('navigation', { name: 'Principal' })
    await usuario.click(within(pieDeLaBarraLateral()).getByRole('button', { name: 'Cerrar sesión' }))

    expect(await screen.findByText('no se pudo cerrar la sesión')).toBeInTheDocument()
  })
})
