import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router-dom'
import App from './App'

// Humo del andamiaje: el panel se monta dentro de los mismos providers que en
// producción (TanStack Query + router) y muestra su nombre.
function renderApp() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('App', () => {
  it('muestra el nombre del panel', () => {
    renderApp()
    expect(screen.getByText('Kesto')).toBeInTheDocument()
  })
})
