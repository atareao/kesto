import type { Estado } from './api/types'

/** Estado de una tarea en el panel: el de su última ejecución o «ninguna». */
export type EstadoDeTarea = Estado | 'sin-ejecuciones'

/** Texto que acompaña al `Badge`: el color nunca es el único portador. */
export function etiquetaDeEstado(estado: EstadoDeTarea): string {
  switch (estado) {
    case 'pending':
      return 'Pendiente'
    case 'running':
      return 'En curso'
    case 'success':
      return 'Correcta'
    case 'failed':
      return 'Fallida'
    case 'sin-ejecuciones':
      return 'Sin ejecuciones'
  }
}

/** `status` del `Badge` de Ant Design para cada estado. */
export function colorDeEstado(
  estado: EstadoDeTarea,
): 'default' | 'processing' | 'success' | 'error' | 'warning' {
  switch (estado) {
    case 'pending':
      return 'warning'
    case 'running':
      return 'processing'
    case 'success':
      return 'success'
    case 'failed':
      return 'error'
    case 'sin-ejecuciones':
      return 'default'
  }
}

/** El estado ha terminado: no habrá más eventos para esa ejecución. */
export function estaTerminada(estado: EstadoDeTarea): boolean {
  return estado === 'success' || estado === 'failed'
}
