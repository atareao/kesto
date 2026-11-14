import { useState } from 'react'
import { BrowserRouter } from 'react-router-dom'
import { QueryClientProvider } from '@tanstack/react-query'
import { ConfigProvider } from 'antd'
import App from '../../App'
import { crearClienteDeConsultas } from '../../services/clienteDeConsultas'
import { ProveedorDeEventos } from '../../services/events'
import { temaDeAntd, useTema } from '../../services/useTema'

/**
 * La raíz de providers del panel. Vive aquí —y no en `main.tsx`— porque monta
 * componentes React: `main.tsx` queda solo para el arranque (buscar el `#root` y
 * renderizar), que es lo que exige `react-refresh/only-export-components`.
 *
 * El `ConfigProvider` de Antd vive **dentro** de `ProveedorDeTema` porque
 * necesita `useTema()` para leer el modo activo y reconstruir el `ThemeConfig`
 * cuando cambia. El cliente de consultas se crea una sola vez por montaje.
 */
export function PanelConTema() {
  const { modo } = useTema()
  const [cliente] = useState(crearClienteDeConsultas)

  return (
    <ConfigProvider theme={temaDeAntd(modo)}>
      <QueryClientProvider client={cliente}>
        <ProveedorDeEventos>
          <BrowserRouter>
            <App />
          </BrowserRouter>
        </ProveedorDeEventos>
      </QueryClientProvider>
    </ConfigProvider>
  )
}
