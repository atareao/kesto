import { Route, Routes } from 'react-router-dom'
import { Layout } from './components/Layout/Layout'
import { SoloConSesion } from './components/SoloConSesion/SoloConSesion'
import Portada from './pages/portada'
import Tareas from './pages/tareas'
import Historico from './pages/historico'
import Snapshots from './pages/snapshots'
import Restauraciones from './pages/restauraciones'
import Ajustes from './pages/ajustes'
import NoEncontrada from './pages/noEncontrada'

/**
 * Las rutas del panel (§6). Todas cuelgan del layout, así que el menú lateral
 * sigue visible también en la pantalla de «no encontrada». El `BrowserRouter`
 * vive en `PanelConTema.tsx`; aquí solo se declaran las rutas.
 *
 * La portada es la ruta índice y queda **fuera** de `SoloConSesion`: en modo
 * `oidc` sin sesión enseña la portada pública con el botón de entrar, que es el
 * único punto de entrada antes de la autenticación. El resto de pantallas —y la
 * de «no encontrada»— van bajo la guarda: en `oidc` sin sesión desvían a `/`.
 */
export default function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<Portada />} />
        <Route element={<SoloConSesion />}>
          <Route path="tareas" element={<Tareas />} />
          <Route path="historico" element={<Historico />} />
          <Route path="snapshots" element={<Snapshots />} />
          <Route path="restauraciones" element={<Restauraciones />} />
          <Route path="ajustes" element={<Ajustes />} />
          <Route path="*" element={<NoEncontrada />} />
        </Route>
      </Route>
    </Routes>
  )
}
