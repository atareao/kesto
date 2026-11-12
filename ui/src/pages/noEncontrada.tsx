import { EnConstruccion } from '../components/EnConstruccion/EnConstruccion'

/** Ruta desconocida: se pinta dentro del layout, sin perder el menú. */
export default function NoEncontrada() {
  return (
    <EnConstruccion
      titulo="No encontrada"
      descripcion="Esta ruta no existe en el panel. Usa el menú lateral para volver a una pantalla."
    />
  )
}
