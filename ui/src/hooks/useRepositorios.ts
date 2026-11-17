import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  actualizarRepositorio,
  borrarRepositorio,
  comprobarSaludDeRepositorio,
  comprobarSaludDeTodos,
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

/**
 * Comprobación de salud de **un** repositorio.
 *
 * El desenlace lo persiste el backend antes de responder, así que al terminar se
 * invalida el listado y el indicador enseña lo que quedó anotado en la fila, sin
 * que el panel se invente un estado optimista. El `isPending` es de esta
 * mutación y de nadie más: la comprobación de una fila no bloquea
 * la tabla, y `variables` dice **cuál** se está comprobando.
 *
 * Un fallo no invalida nada: si la petición no llegó, el listado sigue siendo la
 * última verdad conocida y el error se enseña como aviso.
 */
export function useComprobarSaludDeRepositorio() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => comprobarSaludDeRepositorio(id),
    onSuccess: () => cliente.invalidateQueries({ queryKey: claves.repositorios }),
  })
}

/**
 * Comprobación de salud de **todos** los repositorios. Igual que la anterior,
 * invalida el listado al terminar para que cada fila refleje su desenlace.
 */
export function useComprobarSaludDeTodos() {
  const cliente = useQueryClient()
  return useMutation({
    mutationFn: () => comprobarSaludDeTodos(),
    onSuccess: () => cliente.invalidateQueries({ queryKey: claves.repositorios }),
  })
}
