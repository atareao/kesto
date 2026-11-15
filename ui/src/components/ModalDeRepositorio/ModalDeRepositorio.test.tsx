import { useState } from 'react'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../../tests/fetchDePrueba'
import { repositorio } from '../../../tests/fixtures'
import type { Repository } from '../../services/api/types'
import { ModalDeRepositorio } from './ModalDeRepositorio'

/**
 * Arnés que reproduce el ciclo de la pantalla de Ajustes: abrir edición de una
 * fila, cerrar y abrir la de otra. La instancia del modal no se desmonta entre
 * filas, que es el escenario del hallazgo B1.
 */
function Arnes({ filas }: { filas: readonly Repository[] }) {
  const [abierto, setAbierto] = useState(false)
  const [enEdicion, setEnEdicion] = useState<Repository | null>(null)

  return (
    <>
      {filas.map((fila) => (
        <button
          key={fila.id}
          type="button"
          onClick={() => {
            setEnEdicion(fila)
            setAbierto(true)
          }}
        >
          {`Editar ${fila.id}`}
        </button>
      ))}
      <ModalDeRepositorio
        abierto={abierto}
        repositorio={enEdicion}
        onCerrar={() => setAbierto(false)}
        onGuardado={() => setAbierto(false)}
      />
    </>
  )
}

function montarArnes(filas: readonly Repository[]) {
  const cliente = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  })
  return render(
    <QueryClientProvider client={cliente}>
      <Arnes filas={filas} />
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

const FILA1 = repositorio({
  id: 'r1',
  name: 's3-uno',
  endpoint: 'https://uno.example.com',
  has_secret_access_key: true,
  has_passphrase: true,
})
const FILA2 = repositorio({
  id: 'r2',
  name: 's3-dos',
  endpoint: 'https://dos.example.com',
  has_secret_access_key: true,
  has_passphrase: true,
})

describe('ModalDeRepositorio', () => {
  it('al cerrar y editar otra fila, el formulario muestra los valores de la nueva fila y no arrastra los secretos', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(() => {
      throw new Error('no debería llamarse a fetch')
    })

    montarArnes([FILA1, FILA2])

    // Editar la fila 1 y teclear los secretos.
    await usuario.click(screen.getByRole('button', { name: 'Editar r1' }))
    const nombre1 = await screen.findByRole('textbox', { name: 'Nombre' })
    await usuario.clear(nombre1)
    await usuario.type(nombre1, 'editado-uno')
    await usuario.type(screen.getByLabelText(/clave secreta/i), 'secreto-de-uno')
    await usuario.type(screen.getByLabelText(/passphrase/i), 'frase-de-uno')

    // Cerrar y abrir la edición de la fila 2.
    await usuario.click(screen.getByRole('button', { name: 'Cancelar' }))
    await usuario.click(screen.getByRole('button', { name: 'Editar r2' }))

    expect(await screen.findByRole('textbox', { name: 'Nombre' })).toHaveValue('s3-dos')
    expect(screen.getByRole('textbox', { name: 'Endpoint' })).toHaveValue('https://dos.example.com')
    // Los secretos tecleados para la fila 1 no viajan a la fila 2.
    expect(screen.getByLabelText(/clave secreta/i)).toHaveValue('')
    expect(screen.getByLabelText(/passphrase/i)).toHaveValue('')
  })
})
