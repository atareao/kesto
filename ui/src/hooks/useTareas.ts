import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { actualizarTarea, borrarTarea, crearTarea, dispararTarea, listarTareas } from '../api/tareas'
import { ErrorDeApi } from '../services/api/envelope'
import type { AltaJob } from '../services/api/types'
import { claves } from './claves'

/** Lista de tareas programadas. */
export function useTareas() {
  return useQuery({
    queryKey: claves.tareas,
    queryFn: ({ signal }) => listarTareas(signal),
  })
}

/** Alta de una tarea. Al terminar, la lista se vuelve a pedir. */
export function useCrearTarea() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: (cuerpo: AltaJob) => crearTarea(cuerpo),
    onSuccess: () => cliente.invalidateQueries({ queryKey: claves.tareas }),
  })
}

/** Edición de una tarea. */
export function useActualizarTarea() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: ({ id, cuerpo }: { id: string; cuerpo: AltaJob }) => actualizarTarea(id, cuerpo),
    onSuccess: () => cliente.invalidateQueries({ queryKey: claves.tareas }),
  })
}

/** Borrado de una tarea. También caen sus ejecuciones: ya no cuelgan de nada. */
export function useBorrarTarea() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => borrarTarea(id),
    onSuccess: () => {
      void cliente.invalidateQueries({ queryKey: claves.tareas })
      void cliente.invalidateQueries({ queryKey: ['ejecuciones'] })
    },
  })
}

/**
 * Disparo manual. Un `202` deja una ejecución `pending`, así que se refrescan
 * las ejecuciones. Un `404` significa que la tarea ya no existe: se refresca la
 * lista para que desaparezca. El error se propaga para que la pantalla lo
 * explique (un `409` no es un fallo de red, es una ejecución en curso).
 */
export function useDispararTarea() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => dispararTarea(id),
    onSuccess: () => {
      void cliente.invalidateQueries({ queryKey: ['ejecuciones'] })
    },
    onError: (error) => {
      if (error instanceof ErrorDeApi && error.status === 404) {
        void cliente.invalidateQueries({ queryKey: claves.tareas })
      }
    },
  })
}
