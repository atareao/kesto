import { Spin, Typography } from 'antd'

export interface PropsDeCargando {
  /** Texto que acompaña al indicador. */
  etiqueta?: string
}

/**
 * Estado de carga de una pantalla o sección. Un `Spin` visible y un texto: el
 * giro por sí solo no anuncia nada a quien usa un lector de pantalla.
 */
export function Cargando({ etiqueta = 'Cargando…' }: PropsDeCargando) {
  return (
    <div
      data-testid="cargando"
      style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '24px 0' }}
    >
      <Spin />
      <Typography.Text type="secondary">{etiqueta}</Typography.Text>
    </div>
  )
}
