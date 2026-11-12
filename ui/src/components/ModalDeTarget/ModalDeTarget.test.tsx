import { useState } from 'react'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { instalarFetchFalso } from '../../pruebas/fetchDePrueba'
import { target } from '../../pruebas/fixtures'
import type { Target } from '../../services/api/types'
import { ModalDeTarget } from './ModalDeTarget'

/**
 * Arnés que reproduce el ciclo de la pantalla de Ajustes: abrir edición de una
 * fila, cerrar y abrir la de otra. La instancia del modal no se desmonta entre
 * filas, que es el escenario del hallazgo B1.
 */
function Arnes({ filas }: { filas: readonly Target[] }) {
  const [abierto, setAbierto] = useState(false)
  const [enEdicion, setEnEdicion] = useState<Target | null>(null)

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
      <ModalDeTarget
        abierto={abierto}
        target={enEdicion}
        onCerrar={() => setAbierto(false)}
        onGuardado={() => setAbierto(false)}
      />
    </>
  )
}

function montarArnes(filas: readonly Target[]) {
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

const FILA1 = target({
  id: 't1',
  name: 'db-uno',
  target_type: 'mariadb',
  has_db_password: true,
  container_name: 'c1',
  db_user: 'u1',
  db_name: 'd1',
  host_path: null,
})
const FILA2 = target({
  id: 't2',
  name: 'db-dos',
  target_type: 'mariadb',
  has_db_password: true,
  container_name: 'c2',
  db_user: 'u2',
  db_name: 'd2',
  host_path: null,
})

describe('ModalDeTarget', () => {
  it('al cerrar y editar otra fila, el formulario muestra los valores de la nueva fila y no arrastra el secreto', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(() => {
      throw new Error('no debería llamarse a fetch')
    })

    montarArnes([FILA1, FILA2])

    // Editar la fila 1 y teclear un secreto.
    await usuario.click(screen.getByRole('button', { name: 'Editar t1' }))
    const nombre1 = await screen.findByRole('textbox', { name: 'Nombre' })
    await usuario.clear(nombre1)
    await usuario.type(nombre1, 'editado-uno')
    await usuario.type(screen.getByLabelText(/contraseña de la base de datos/i), 'secreto-de-uno')

    // Cerrar y abrir la edición de la fila 2.
    await usuario.click(screen.getByRole('button', { name: 'Cancelar' }))
    await usuario.click(screen.getByRole('button', { name: 'Editar t2' }))

    expect(await screen.findByRole('textbox', { name: 'Nombre' })).toHaveValue('db-dos')
    // El secreto tecleado para la fila 1 no viaja a la fila 2.
    expect(screen.getByLabelText(/contraseña de la base de datos/i)).toHaveValue('')
  })
})
