import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  actualizarRepositorio,
  borrarRepositorio,
  crearRepositorio,
  listarRepositorios,
} from '../api/repositorios'
import type { AltaRepository } from '../services/api/types'
import { claves } from './claves'

/** Lista de repositorios. La clave es la raíz: una mutación invalida la lista entera. */
export function useRepositorios() {
  return useQuery({
    queryKey: claves.repositorios,
    queryFn: ({ signal }) => listarRepositorios(signal),
  })
}

/** Alta de un repositorio. */
export function useCrearRepositorio() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: (cuerpo: AltaRepository) => crearRepositorio(cuerpo),
    onSuccess: () => cliente.invalidateQueries({ queryKey: claves.repositorios }),
  })
}

/** Edición de un repositorio. Un secreto vacío se omite y conserva el guardado. */
export function useActualizarRepositorio() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: ({ id, cuerpo }: { id: string; cuerpo: AltaRepository }) =>
      actualizarRepositorio(id, cuerpo),
    onSuccess: () => cliente.invalidateQueries({ queryKey: claves.repositorios }),
  })
}

/**
 * Borrado de un repositorio. Sus snapshots dejan de ser consultables y las
 * tareas que lo usaban caen en cascada: se invalidan ambas listas.
 */
export function useBorrarRepositorio() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => borrarRepositorio(id),
    onSuccess: () => {
      void cliente.invalidateQueries({ queryKey: claves.repositorios })
      void cliente.invalidateQueries({ queryKey: claves.tareas })
    },
  })
}
