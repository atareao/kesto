import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { PanelConTema } from './components/PanelConTema/PanelConTema'
import { ProveedorDeTema } from './services/tema'
import './estilos.css'

/**
 * Arranque del panel: busca el `#root`, envuelve el árbol en `ProveedorDeTema`
 * (el modo activo, oscuro por defecto) y monta la raíz de providers. Sin
 * componentes definidos aquí: el rule de Fast Refresh pide que un módulo sin
 * exports de componentes no declare componentes.
 */

const rootElement = document.getElementById('root')
if (!rootElement) {
  throw new Error('No se encontró el elemento #root en el documento')
}

createRoot(rootElement).render(
  <StrictMode>
    <ProveedorDeTema>
      <PanelConTema />
    </ProveedorDeTema>
  </StrictMode>,
)
