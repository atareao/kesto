import { render, screen } from '@testing-library/react'
import { MarcadorSinNombre } from './MarcadorSinNombre'

describe('MarcadorSinNombre', () => {
  it('pinta un marcador explícito y atenuado, nunca un hueco en blanco', () => {
    render(<MarcadorSinNombre />)

    const marcador = screen.getByText('sin nombre')
    expect(marcador).toBeInTheDocument()
    expect(marcador).toHaveClass('ant-typography-secondary')
  })
})
