import { EnConstruccion } from '../components/EnConstruccion/EnConstruccion'

/** Pantalla de Ajustes (Fase 4): salud del despliegue y CRUD de fuentes y destinos. */
export default function Ajustes() {
  return (
    <EnConstruccion
      titulo="Ajustes"
      descripcion="Aquí se mostrará la versión y el modo de autenticación, y se gestionarán los targets y los repositorios."
    />
  )
}
