import { Pagination } from 'antd'
import type { PaginationProps } from 'antd'

/**
 * Filas por página que ofrece el paginador. La primera —`10`— es el **defecto**
 * del panel: el listado se pide al servidor con ese `limit` explícito porque el
 * defecto del backend es otro (`50`) y el panel no debe depender de él.
 */
const OPCIONES_DE_TAMANO_POR_PAGINA: readonly number[] = [10, 20, 50]

/** Tamaño de página con el que arranca cualquier listado paginado. */
export const TAMANO_DE_PAGINA_POR_DEFECTO = 10

/**
 * Vacío de una página que **quedó** vacía —no de un listado sin datos—: se pinta
 * cuando el operador está en una página mayor que la primera y el conjunto ha
 * encogido por debajo de su `offset` (retención o purga). Decir «no hay nada»
 * ahí sería mentir —en la página 1 sí hay—, así que el texto nombra lo que pasó
 * y lleva de vuelta al principio. Vive aquí, con el paginador, para que las tres
 * pantallas digan lo mismo.
 */
export const TEXTO_DE_PAGINA_VACIA = 'Esta página quedó vacía: vuelve a la primera página.'

/**
 * Textos del paginador. Antd los trae en inglés por defecto y el panel está en
 * español, así que se fijan aquí —en un solo sitio— en vez de repetirlos en cada
 * pantalla.
 */
const LOCALE: PaginationProps['locale'] = {
  items_per_page: 'por página',
  jump_to: 'Ir a',
  page: 'Página',
  prev_page: 'Página anterior',
  next_page: 'Página siguiente',
  page_size: 'Filas por página',
}

/** Lo que el operador acaba de elegir en el paginador. */
export interface CambioDePagina {
  /** Página a la que ir, contada desde 1. */
  pagina: number
  /** Filas por página vigentes tras el cambio. */
  tamanoDePagina: number
}

export interface PaginadorProps {
  /** Total de elementos que declara la API; de él salen el número de páginas. */
  total: number
  /** Página actual, contada desde 1. */
  pagina: number
  /** Filas por página vigentes. */
  tamanoDePagina: number
  /** Cambio de página o de tamaño; la pantalla decide qué pedir al servidor. */
  onCambiar: (cambio: CambioDePagina) => void
}

/**
 * El paginador de Ant Design, alimentado por el `total` de la respuesta.
 *
 * Es **del servidor**: el panel nunca recorta una lista ya recibida —eso daría
 * un número de páginas falso y una tabla incompleta con el total correcto—, sino
 * que traduce cada elección a un `limit`/`offset` que pide la página de verdad.
 * El `total` viene del backend y el paginador lo pinta tal cual: no se inventa.
 *
 * Al cambiar el **tamaño** de página se vuelve a la **primera**: un `offset`
 * heredado sobre un tamaño distinto apuntaría a una página que el operador no
 * está viendo. Ese «vuelve a empezar» no lo hace Antd por sí solo —conserva la
 * página mientras siga existiendo—, de ahí que se normalice aquí.
 */
export function Paginador({ total, pagina, tamanoDePagina, onCambiar }: PaginadorProps) {
  /**
   * Filas que se le declaran a Antd para que pueda **representar** el listado.
   *
   * Con `total === 0` —la retención o la purga dejaron el listado vacío mientras
   * el operador estaba en una página alta— `rc-pagination` no admite salto
   * alguno: su guarda `isValid(page)` exige `total > 0`, así que reduce el
   * control a un único item «1» que **no responde al clic** y deja al operador
   * encerrado sin vuelta a la primera página. Declarándole las filas justas para
   * contener la página actual (`pagina * tamanoDePagina`), «1» y el botón de
   * anterior siguen vivos y clicables.
   *
   * La inflación es **solo** para ese caso. Con `total > 0` la misma guarda
   * `isValid` ya deja el item «1» vivo, así que se declara el `total` de la API
   * tal cual: inflarlo con un `Math.max` —como con `total = 15`, `tamano = 10`,
   * `pagina = 3`, que declararía 30— ofrecería páginas que el backend nunca va a
   * devolver.
   *
   * El recuento visible **no** usa esta cifra: sale del `total` real de la API
   * a través de `showTotal`, que es el dato verdadero.
   */
  const totalParaAntd = total === 0 ? pagina * tamanoDePagina : total

  return (
    <Pagination
      current={pagina}
      pageSize={tamanoDePagina}
      total={totalParaAntd}
      showSizeChanger
      showTotal={() => `${total} ${total === 1 ? 'elemento' : 'elementos'}`}
      pageSizeOptions={[...OPCIONES_DE_TAMANO_POR_PAGINA]}
      locale={LOCALE}
      onChange={(nuevaPagina, nuevoTamano) => {
        // Antd llama a `onChange` tanto al cambiar de página como al cambiar el
        // tamaño; no hay un `onShowSizeChange` separado que haga falta. Si el
        // tamaño cambió, el contrato manda volver a la primera página.
        if (nuevoTamano !== tamanoDePagina) {
          onCambiar({ pagina: 1, tamanoDePagina: nuevoTamano })
          return
        }
        onCambiar({ pagina: nuevaPagina, tamanoDePagina: nuevoTamano })
      }}
    />
  )
}
