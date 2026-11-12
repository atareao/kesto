import { EnConstruccion } from '../components/EnConstruccion/EnConstruccion'

/** Pantalla de Histórico (Fase 4): ejecuciones paginadas y consola en vivo. */
export default function Historico() {
  return (
    <EnConstruccion
      titulo="Histórico"
      descripcion="Aquí se listarán las ejecuciones con sus filtros, su paginación y la consola de la ejecución en vivo."
    />
  )
}
