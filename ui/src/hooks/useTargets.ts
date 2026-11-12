import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  actualizarTarget,
  borrarTarget,
  crearTarget,
  listarTargets,
} from '../api/targets'
import type { AltaTarget } from '../services/api/types'
import { claves } from './claves'

/** Lista de targets (orígenes de copia). */
export function useTargets() {
  return useQuery({
    queryKey: claves.targets,
    queryFn: ({ signal }) => listarTargets(signal),
  })
}

/** Alta de un target. Al terminar, la lista se vuelve a pedir. */
export function useCrearTarget() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: (cuerpo: AltaTarget) => crearTarget(cuerpo),
    onSuccess: () => cliente.invalidateQueries({ queryKey: claves.targets }),
  })
}

/** Edición de un target. */
export function useActualizarTarget() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: ({ id, cuerpo }: { id: string; cuerpo: AltaTarget }) =>
      actualizarTarget(id, cuerpo),
    onSuccess: () => cliente.invalidateQueries({ queryKey: claves.targets }),
  })
}

/**
 * Borrado de un target. Las tareas que colgaban de él caen en cascada
 * (`ON DELETE CASCADE`), así que también se invalida su lista.
 */
export function useBorrarTarget() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => borrarTarget(id),
    onSuccess: () => {
      void cliente.invalidateQueries({ queryKey: claves.targets })
      void cliente.invalidateQueries({ queryKey: claves.tareas })
    },
  })
}
