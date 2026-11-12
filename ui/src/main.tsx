import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import { QueryClientProvider } from '@tanstack/react-query'
import App from './App'
import { crearClienteDeConsultas } from './services/clienteDeConsultas'
import { ProveedorDeEventos } from './services/events'
import './estilos.css'

const queryClient = crearClienteDeConsultas()

const rootElement = document.getElementById('root')
if (!rootElement) {
  throw new Error('No se encontró el elemento #root en el documento')
}

createRoot(rootElement).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <ProveedorDeEventos>
        <BrowserRouter>
          <App />
        </BrowserRouter>
      </ProveedorDeEventos>
    </QueryClientProvider>
  </StrictMode>,
)
