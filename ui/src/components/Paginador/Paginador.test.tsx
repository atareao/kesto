import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { limpiarDesplegables } from '../../../tests/selectDeAntd'
import type { CambioDePagina } from './Paginador'
import { Paginador, TAMANO_DE_PAGINA_POR_DEFECTO } from './Paginador'

/** Un espía de `onCambiar` que guarda el último cambio pedido. */
function espia() {
  const cambios: CambioDePagina[] = []
  return { cambios, onCambiar: (cambio: CambioDePagina) => cambios.push(cambio) }
}

beforeEach(() => {
  limpiarDesplegables()
})

describe('Paginador', () => {
  it('arranca con 10 filas por defecto y pinta el total que declara la API', () => {
    expect(TAMANO_DE_PAGINA_POR_DEFECTO).toBe(10)

    const { onCambiar } = espia()
    render(<Paginador total={120} pagina={1} tamanoDePagina={10} onCambiar={onCambiar} />)

    // El total sale de la respuesta, no se inventa.
    expect(screen.getByText('120 elementos')).toBeInTheDocument()
    // 120 filas a 10 por página son 12 páginas: la última ofrece ir a ella.
    expect(screen.getByTitle('12')).toBeInTheDocument()
    expect(screen.queryByTitle('13')).not.toBeInTheDocument()
  })

  it('elegir una página pide esa página al servidor', async () => {
    const usuario = userEvent.setup()
    const { cambios, onCambiar } = espia()
    render(<Paginador total={120} pagina={1} tamanoDePagina={10} onCambiar={onCambiar} />)

    await usuario.click(screen.getByTitle('3'))

    expect(cambios).toEqual([{ pagina: 3, tamanoDePagina: 10 }])
  })

  it('con el listado vacío en una página alta deja volver a la primera y sigue leyendo 0 elementos', async () => {
    const usuario = userEvent.setup()
    const { cambios, onCambiar } = espia()
    // La retención o la purga vaciaron el listado con el operador en la página 2.
    render(
      <Paginador
        total={0}
        pagina={2}
        tamanoDePagina={TAMANO_DE_PAGINA_POR_DEFECTO}
        onCambiar={onCambiar}
      />,
    )

    // El recuento visible sale del `total` real de la API, no del que se le
    // declara a Antd para que la página actual siga siendo representable.
    expect(screen.getByText('0 elementos')).toBeInTheDocument()

    // `rc-pagination` con `total === 0` desactivaba el item «1» y encerraba al
    // operador en una página que ya no existe.
    await usuario.click(screen.getByTitle('1'))

    expect(cambios).toEqual([{ pagina: 1, tamanoDePagina: TAMANO_DE_PAGINA_POR_DEFECTO }])
  })

  it('con el total real por encima de la página no ofrece páginas fantasma', async () => {
    const usuario = userEvent.setup()
    const { cambios, onCambiar } = espia()
    // 15 filas a 10 por página son 2 páginas, y el operador está en la 3: la
    // retención o la purga encogieron el listado por debajo de su `offset`.
    // Inflar lo que se le declara a Antd (`Math.max(15, 3 * 10)` daría 30)
    // inventaría una tercera página que el backend nunca va a devolver.
    render(<Paginador total={15} pagina={3} tamanoDePagina={10} onCambiar={onCambiar} />)

    expect(screen.queryByTitle('3')).not.toBeInTheDocument()
    // El recuento visible sigue siendo el real.
    expect(screen.getByText('15 elementos')).toBeInTheDocument()
    // Las dos páginas que sí existen están vivas. `rc-pagination` recorta la
    // `current` fuera de rango a la última página real, así que la 2 se pinta
    // como activa —y no responde al clic, ya está ahí—: la vuelta se hace por
    // la 1, que sí viaja.
    expect(screen.getByTitle('1')).toBeInTheDocument()
    expect(screen.getByTitle('2')).toBeInTheDocument()

    await usuario.click(screen.getByTitle('1'))

    expect(cambios).toEqual([{ pagina: 1, tamanoDePagina: 10 }])
  })

  it('cambiar el tamaño de página vuelve a la primera', async () => {
    const usuario = userEvent.setup()
    const { cambios, onCambiar } = espia()
    render(<Paginador total={120} pagina={3} tamanoDePagina={10} onCambiar={onCambiar} />)

    await usuario.click(screen.getByRole('combobox', { name: 'Filas por página' }))
    await usuario.click(await screen.findByTitle('50 por página'))

    // No conserva la página 3: con otro tamaño apuntaría a filas distintas.
    expect(cambios).toEqual([{ pagina: 1, tamanoDePagina: 50 }])
  })
})
