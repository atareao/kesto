import { useState } from 'react'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { GeneradorCron } from './GeneradorCron'

/** Envoltorio controlado, como lo usaría un `Form.Item`. */
function Controlado({ inicial = '' }: { inicial?: string }) {
  const [valor, setValor] = useState(inicial)
  return <GeneradorCron value={valor} onChange={setValor} />
}

describe('GeneradorCron', () => {
  it('el preset de las 03:00 produce una expresión de seis campos', async () => {
    const usuario = userEvent.setup()
    render(<Controlado />)

    await usuario.click(screen.getByRole('button', { name: 'Diaria a las 03:00' }))

    const expresion = screen.getByRole('textbox', { name: 'Expresión cron' })
    expect(expresion).toHaveValue('0 0 3 * * *')
    expect((expresion as HTMLInputElement).value.trim().split(/\s+/)).toHaveLength(6)
  })

  it('hora, minuto y segundo componen la expresión', async () => {
    const usuario = userEvent.setup()
    render(<Controlado />)

    await usuario.type(screen.getByRole('spinbutton', { name: 'Segundo' }), '0')
    await usuario.type(screen.getByRole('spinbutton', { name: 'Minuto' }), '0')
    await usuario.type(screen.getByRole('spinbutton', { name: 'Hora' }), '3')

    expect(screen.getByRole('textbox', { name: 'Expresión cron' })).toHaveValue('0 0 3 * * *')
  })

  it('la expresión se puede editar directamente como texto', async () => {
    const usuario = userEvent.setup()
    render(<Controlado inicial="0 0 3 * * *" />)

    const expresion = screen.getByRole('textbox', { name: 'Expresión cron' })
    await usuario.clear(expresion)
    await usuario.type(expresion, '0 30 2 * * 1')

    expect(expresion).toHaveValue('0 30 2 * * 1')
  })
})
