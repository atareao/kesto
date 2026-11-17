import type { SaludDeRepositorio } from './api/types'

/**
 * Vocabulario de la salud de un repositorio en la interfaz.
 *
 * Los dos `switch` son **exhaustivos en compilación**: si el contrato gana un
 * estado, la llamada a `noPrevisto` deja de compilar y hay que decidir su texto
 * y su color en vez de pintar un hueco. Y también tienen caída en **tiempo de
 * ejecución**, porque el union es un tipo y el JSON es un dato: un backend más
 * nuevo —o una respuesta manipulada— puede mandar un estado que este panel no
 * conozca, y entonces el distintivo no puede quedarse mudo. Ninguna de las dos
 * funciones inventa estados: lo que llega dentro del contrato se nombra tal cual,
 * y lo que se sale de él se pinta como «Desconocido».
 */

/** Estado del `Badge` de Ant Design para una salud del repositorio. */
export type ColorDeSalud = 'success' | 'warning' | 'error' | 'default'

/** Cómo se nombra un estado: su texto y el color que lo refuerza. */
interface NombreDeSalud {
  etiqueta: string
  color: ColorDeSalud
}

/** Lo que se enseña cuando el estado no está en el contrato: neutro, no sano. */
const NO_PREVISTO: NombreDeSalud = { etiqueta: 'Desconocido', color: 'default' }

/**
 * Red de seguridad de los dos `switch` de abajo.
 *
 * Recibe `never` a propósito: la llamada solo compila mientras el union esté
 * cubierto por completo, así que un estado nuevo del contrato obliga a darle
 * texto y color. El parámetro no se puede mirar —por tipos, aquí ya no hay
 * valor—: el `void` lo deja explícito, y la respuesta es siempre la misma, el
 * estado neutro.
 */
function noPrevisto(salud: never): NombreDeSalud {
  void salud
  return NO_PREVISTO
}

/** Texto del estado. Acompaña **siempre** al color: nunca es el único portador. */
export function etiquetaDeSalud(salud: SaludDeRepositorio): string {
  switch (salud) {
    case 'ok':
      return 'Correcto'
    case 'sin_repositorio':
      return 'Sin repositorio'
    case 'passphrase':
      return 'Passphrase incorrecta'
    case 'inaccesible':
      return 'Inaccesible'
    case 'sin_permiso':
      return 'Sin permiso de lectura'
    case 'desconocido':
      return NO_PREVISTO.etiqueta
  }

  return noPrevisto(salud).etiqueta
}

/**
 * `status` del `Badge` de Ant Design para cada estado.
 *
 * «Desconocido» va en `default` —el gris neutro— y no en `success`: un
 * repositorio sin comprobar no puede leerse como sano ni de reojo.
 */
export function colorDeSalud(salud: SaludDeRepositorio): ColorDeSalud {
  switch (salud) {
    case 'ok':
      return 'success'
    case 'sin_repositorio':
      return 'warning'
    case 'passphrase':
      return 'error'
    case 'inaccesible':
      return 'error'
    case 'sin_permiso':
      return 'error'
    case 'desconocido':
      return NO_PREVISTO.color
  }

  return noPrevisto(salud).color
}
