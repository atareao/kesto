import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { Layout } from './Layout'

/**
 * Pruebas del layout: accesibilidad (§3.5) y menú plegable en ventana estrecha
 * (§3.3). El layout se monta dentro de un router porque usa `Link`, `Outlet` y
 * `useLocation`.
 */
function montarLayout() {
  return render(
    <MemoryRouter initialEntries={['/']}>
      <Routes>
        <Route element={<Layout />}>
          <Route index element={<p>contenido de la pantalla</p>} />
        </Route>
      </Routes>
    </MemoryRouter>,
  )
}

const PANTALLAS = ['Dashboard', 'Tareas', 'Histórico', 'Snapshots', 'Ajustes'] as const

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

describe('menú plegable en ventana estrecha', () => {
  afterEach(() => {
    // Devuelve el ancho por defecto de jsdom para no contaminar otras pruebas.
    window.innerWidth = 1024
  })

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
