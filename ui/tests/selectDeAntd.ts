/**
 * Utilidades para manejar los `Select` de Antd desde las pruebas de pantalla.
 *
 * Antd pinta las opciones en un portal colgado de `document.body`, fuera del
 * contenedor que limpia Testing Library, así que un caso que acabe con el
 * desplegable abierto deja restos que confunden a los siguientes. Además las
 * opciones **no** exponen `role="option"` de forma fiable (Antd solo lo pone en
 * un listado oculto y recortado), por lo que se localizan dentro del
 * desplegable abierto.
 */

import { waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

type Usuario = ReturnType<typeof userEvent.setup>

/** El desplegable abierto más reciente, o `null` si no hay ninguno a la vista. */
export function desplegableAbierto(): HTMLElement | null {
  const abiertos = document.querySelectorAll<HTMLElement>(
    '.ant-select-dropdown:not(.ant-select-dropdown-hidden)',
  )
  return abiertos[abiertos.length - 1] ?? null
}

/**
 * Abre el `Select` de la etiqueta dada y devuelve su desplegable ya pintado.
 *
 * `ambito` acota el combobox, útil cuando la pantalla tiene más de un `Select`.
 */
export async function abrirSelect(
  usuario: Usuario,
  etiqueta: string,
  ambito: HTMLElement = document.body,
): Promise<HTMLElement> {
  await usuario.click(within(ambito).getByRole('combobox', { name: etiqueta }))
  return await waitFor(() => {
    const desplegable = desplegableAbierto()
    if (desplegable === null) throw new Error(`el desplegable de «${etiqueta}» no se abrió`)
    return desplegable
  })
}

/**
 * Abre el `Select` de la etiqueta dada y elige la opción por su etiqueta.
 *
 * La opción se busca por su `title` **dentro del desplegable abierto**: el mismo
 * `title` aparece también en el ítem ya seleccionado, de ahí que haya que acotar
 * el ámbito o la consulta encuentra varios elementos.
 */
export async function elegirEnSelect(
  usuario: Usuario,
  etiqueta: string,
  opcion: string,
  ambito: HTMLElement = document.body,
): Promise<void> {
  const desplegable = await abrirSelect(usuario, etiqueta, ambito)
  await usuario.click(within(desplegable).getByTitle(opcion))
}

/** Descarta los desplegables colgados en `document.body` por casos anteriores. */
export function limpiarDesplegables(): void {
  document.querySelectorAll('.ant-select-dropdown').forEach((desplegable) => {
    desplegable.remove()
  })
}

/** Las etiquetas de las opciones del desplegable, en orden. */
export function opcionesDe(desplegable: HTMLElement): Array<string | null> {
  return Array.from(desplegable.querySelectorAll('.ant-select-item-option-content')).map(
    (opcion) => opcion.textContent,
  )
}
