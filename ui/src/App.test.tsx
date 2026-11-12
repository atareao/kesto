import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, useLocation } from 'react-router-dom'
import App from './App'

/**
 * Pruebas de las rutas del panel (§3.1/§3.2): cada entrada del menú abre su
 * pantalla, la ruta activa se marca y una ruta desconocida conserva el menú.
 *
 * El `Ubicacion` es una sonda: pinta el `pathname` actual para poder afirmar que
 * la navegación cambió de ruta sin recargar el documento (es un `MemoryRouter`,
 * así que no hay recarga posible: si la ruta cambia, fue el router).
 */
function Ubicacion() {
  const { pathname } = useLocation()
  return <span data-testid="ubicacion">{pathname}</span>
}

function montar(entrada = '/') {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <MemoryRouter initialEntries={[entrada]}>
        <Ubicacion />
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

const ENTRADAS: ReadonlyArray<{ etiqueta: string; ruta: string }> = [
  { etiqueta: 'Dashboard', ruta: '/' },
  { etiqueta: 'Tareas', ruta: '/tareas' },
  { etiqueta: 'Histórico', ruta: '/historico' },
  { etiqueta: 'Snapshots', ruta: '/snapshots' },
  { etiqueta: 'Ajustes', ruta: '/ajustes' },
]

describe('rutas del panel', () => {
  it('muestra el nombre del panel en el layout', () => {
    montar()
    expect(screen.getByText('Kesto')).toBeInTheDocument()
  })

  it.each(ENTRADAS)(
    'la entrada «$etiqueta» abre su pantalla y cambia la ruta',
    async ({ etiqueta, ruta }) => {
      const usuario = userEvent.setup()
      montar('/')

      const nav = screen.getByRole('navigation', { name: 'Principal' })
      await usuario.click(within(nav).getByRole('link', { name: etiqueta }))

      expect(screen.getByTestId('ubicacion')).toHaveTextContent(ruta)
      expect(screen.getByRole('heading', { level: 1, name: etiqueta })).toBeInTheDocument()
    },
  )

  it('marca como activa solo la entrada de la ruta actual', () => {
    montar('/historico')

    const nav = screen.getByRole('navigation', { name: 'Principal' })
    expect(within(nav).getByRole('link', { name: 'Histórico' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    expect(within(nav).getByRole('link', { name: 'Ajustes' })).not.toHaveAttribute('aria-current')
  })

  it('una ruta desconocida muestra «no encontrada» dentro del menú', () => {
    montar('/no-existe')

    expect(screen.getByRole('heading', { level: 1, name: /no encontrada/i })).toBeInTheDocument()

    const nav = screen.getByRole('navigation', { name: 'Principal' })
    expect(nav).toBeInTheDocument()
    // Las cinco pantallas más la de sesión siguen alcanzables.
    expect(within(nav).getAllByRole('link')).toHaveLength(6)
  })
})
