import type { FiltrosDeEjecuciones } from '../api/ejecuciones'

/**
 * Claves de caché de React Query. Viven en un solo sitio para que invalidar sea
 * una operación deliberada y no una cadena escrita a mano en cada mutación.
 *
 * Las colecciones empiezan por una raíz (`['tareas']`) y la invalidación usa esa
 * raíz: así caen también las variantes con filtros.
 */
export const claves = {
  repositorios: ['repositorios'] as const,
  targets: ['targets'] as const,
  tareas: ['tareas'] as const,
  ejecuciones: (filtros: FiltrosDeEjecuciones = {}) => ['ejecuciones', filtros] as const,
  snapshots: (repositoryId: string) => ['snapshots', repositoryId] as const,
}
