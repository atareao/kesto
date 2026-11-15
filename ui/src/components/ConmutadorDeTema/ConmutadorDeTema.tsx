import { Switch } from 'antd'
import { useTema } from '../../services/useTema'

export interface PropsDelConmutadorDeTema {
  /** Clase opcional para situarlo en la cabecera. */
  className?: string
}

/**
 * El control de tema de la cabecera (§ identidad visual y tema).
 *
 * Convención única: **el nombre accesible dice siempre el tema hacia el que
 * cambia**, no el estado actual ni el color. Una convención mixta —«estado» en
 * el texto visible y «destino» en la etiqueta— confunde, así que el control no
 * lleva texto dentro: el estado lo expone `aria-checked` (es un `button` con
 * `role="switch"`) y el destino lo dice la etiqueta, que se recalcula al
 * alternar. Se activa solo con el teclado y la elección la persiste el proveedor.
 */
export function ConmutadorDeTema({ className }: PropsDelConmutadorDeTema) {
  const { modo, alternar } = useTema()
  const destino = modo === 'oscuro' ? 'claro' : 'oscuro'

  return (
    <Switch
      className={className}
      checked={modo === 'oscuro'}
      onChange={alternar}
      aria-label={`Cambiar a tema ${destino}`}
    />
  )
}
