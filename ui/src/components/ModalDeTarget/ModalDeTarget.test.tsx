import { useState } from 'react'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  abrirSelect,
  elegirEnSelect,
  limpiarDesplegables,
  opcionesDe,
} from '../../pruebas/selectDeAntd'
import { instalarFetchFalso } from '../../pruebas/fetchDePrueba'
import { target } from '../../pruebas/fixtures'
import type { Target } from '../../services/api/types'
import { ModalDeTarget } from './ModalDeTarget'

/**
 * Arnés que reproduce el ciclo de la pantalla de Ajustes: abrir edición de una
 * fila, cerrar y abrir la de otra. La instancia del modal no se desmonta entre
 * filas, que es el escenario del hallazgo B1. El botón de alta es el de la
 * pantalla, que abre el mismo modal con `target === null`.
 */
function Arnes({ filas }: { filas: readonly Target[] }) {
  const [abierto, setAbierto] = useState(false)
  const [enEdicion, setEnEdicion] = useState<Target | null>(null)

  return (
    <>
      <button
        type="button"
        onClick={() => {
          setEnEdicion(null)
          setAbierto(true)
        }}
      >
        Nuevo target
      </button>
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
  // Los desplegables de Antd viven en un portal de `document.body`, fuera del
  // contenedor que Testing Library limpia entre casos: los restos de un caso que
  // acabase con uno abierto aparecerían en el siguiente.
  limpiarDesplegables()
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

  it('el selector de tipo ofrece los cuatro tipos, sqlite incluido', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(() => {
      throw new Error('no debería llamarse a fetch')
    })

    montarArnes([])

    await usuario.click(screen.getByRole('button', { name: 'Nuevo target' }))
    const dialogo = await screen.findByRole('dialog')
    const desplegable = await abrirSelect(usuario, 'Tipo', dialogo)

    expect(opcionesDe(desplegable)).toEqual([
      'Directorio',
      'MariaDB',
      'PostgreSQL',
      'SQLite',
    ])
  })

  it('con sqlite se ven solo contenedor y db_path, y se ocultan host y credenciales', async () => {
    const usuario = userEvent.setup()
    instalarFetchFalso(() => {
      throw new Error('no debería llamarse a fetch')
    })

    montarArnes([])

    await usuario.click(screen.getByRole('button', { name: 'Nuevo target' }))
    const dialogo = await screen.findByRole('dialog')
    await elegirEnSelect(usuario, 'Tipo', 'SQLite', dialogo)

    expect(within(dialogo).getByLabelText('Contenedor')).toBeInTheDocument()
    expect(within(dialogo).getByLabelText(/ruta del \.db/i)).toBeInTheDocument()

    expect(within(dialogo).queryByLabelText('Ruta del host')).not.toBeInTheDocument()
    expect(within(dialogo).queryByLabelText('Usuario de la base de datos')).not.toBeInTheDocument()
    expect(
      within(dialogo).queryByLabelText('Contraseña de la base de datos'),
    ).not.toBeInTheDocument()
    expect(within(dialogo).queryByLabelText('Base de datos')).not.toBeInTheDocument()
  })

  it('el alta de un sqlite manda target_type, contenedor y db_path', async () => {
    const usuario = userEvent.setup()
    const creado = target({
      id: 't9',
      name: 'kesto-db',
      target_type: 'sqlite',
      container_name: 'kesto',
      host_path: null,
      db_path: '/var/lib/kesto/kesto.db',
    })
    const fetch = instalarFetchFalso(({ ruta, method }) => {
      if (ruta === '/api/v1/targets' && method === 'POST') return { status: 201, data: creado }
      throw new Error(`ruta inesperada: ${method} ${ruta}`)
    })

    montarArnes([])

    await usuario.click(screen.getByRole('button', { name: 'Nuevo target' }))
    const dialogo = await screen.findByRole('dialog')
    await usuario.type(within(dialogo).getByLabelText('Nombre'), 'kesto-db')
    await elegirEnSelect(usuario, 'Tipo', 'SQLite', dialogo)
    await usuario.type(within(dialogo).getByLabelText('Contenedor'), 'kesto')
    await usuario.type(within(dialogo).getByLabelText(/ruta del \.db/i), '/var/lib/kesto/kesto.db')
    await usuario.click(within(dialogo).getByRole('button', { name: 'Guardar' }))

    await waitFor(() => expect(fetch.a('/api/v1/targets')).toHaveLength(1))

    const alta = fetch.a('/api/v1/targets')[0]
    expect(alta?.method).toBe('POST')
    expect(alta?.cuerpo).toMatchObject({
      name: 'kesto-db',
      target_type: 'sqlite',
      container_name: 'kesto',
      db_path: '/var/lib/kesto/kesto.db',
    })
    // Un sqlite no lleva ni ruta de host ni credenciales de base de datos.
    expect(alta?.cuerpo).not.toHaveProperty('host_path')
    expect(alta?.cuerpo).not.toHaveProperty('db_user')
    expect(alta?.cuerpo).not.toHaveProperty('db_password')
    expect(alta?.cuerpo).not.toHaveProperty('db_name')
  })
})
