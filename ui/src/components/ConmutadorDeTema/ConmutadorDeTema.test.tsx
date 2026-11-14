import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ProveedorDeTema } from '../../services/tema'
import { CLAVE_DE_TEMA } from '../../services/useTema'
import { ConmutadorDeTema } from './ConmutadorDeTema'

/**
 * El control de tema (§ identidad visual y tema): debe tener nombre accesible
 * —el tema hacia el que cambia, no su color—, ser alcanzable y activable solo
 * con teclado, y persistir la elección.
 */
function montar() {
  return render(
    <ProveedorDeTema>
      <ConmutadorDeTema />
    </ProveedorDeTema>,
  )
}

beforeEach(() => {
  window.localStorage.clear()
  delete document.documentElement.dataset.tema
})

afterEach(() => {
  window.localStorage.clear()
  delete document.documentElement.dataset.tema
})

describe('ConmutadorDeTema', () => {
  it('anuncia el tema destino como nombre accesible', () => {
    montar()

    // En tema oscuro (defecto), el control lleva al claro.
    expect(screen.getByRole('switch', { name: 'Cambiar a tema claro' })).toBeInTheDocument()
  })

  it('alterna el tema al activarse con el teclado y persiste la elección', async () => {
    const usuario = userEvent.setup()
    montar()

    const conmutador = screen.getByRole('switch', { name: 'Cambiar a tema claro' })
    conmutador.focus()
    expect(conmutador).toHaveFocus()

    await usuario.keyboard(' ')

    // Ahora el control ofrece volver al oscuro y la elección quedó guardada.
    expect(screen.getByRole('switch', { name: 'Cambiar a tema oscuro' })).toBeInTheDocument()
    expect(window.localStorage.getItem(CLAVE_DE_TEMA)).toBe('claro')
    expect(document.documentElement.dataset.tema).toBe('claro')
  })
})
