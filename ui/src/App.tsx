import { Route, Routes } from 'react-router-dom'
import { Layout } from './components/Layout/Layout'
import Dashboard from './pages/dashboard'
import Tareas from './pages/tareas'
import Historico from './pages/historico'
import Snapshots from './pages/snapshots'
import Ajustes from './pages/ajustes'
import Sesion from './pages/sesion'
import NoEncontrada from './pages/noEncontrada'

/**
 * Las rutas del panel (§6). Todas cuelgan del layout, así que el menú lateral
 * sigue visible también en la pantalla de «no encontrada». El `BrowserRouter`
 * vive en `main.tsx`; aquí solo se declaran las rutas.
 */
export default function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<Dashboard />} />
        <Route path="tareas" element={<Tareas />} />
        <Route path="historico" element={<Historico />} />
        <Route path="snapshots" element={<Snapshots />} />
        <Route path="ajustes" element={<Ajustes />} />
        <Route path="sesion" element={<Sesion />} />
        <Route path="*" element={<NoEncontrada />} />
      </Route>
    </Routes>
  )
}
