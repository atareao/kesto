import { Badge, Typography } from 'antd'
import type { SaludDeRepositorio } from '../../services/api/types'
import { formatearFecha } from '../../services/format'
import { colorDeSalud, etiquetaDeSalud } from '../../services/salud'

/** El estilo de las dos líneas secundarias (motivo y fecha), hoisteado. */
const ESTILO_SECUNDARIO = { display: 'block', fontSize: 12 } as const

export interface PropsDeIndicadorDeSalud {
  /** El estado tal cual lo sirve la API; el panel no lo reinterpreta. */
  salud: SaludDeRepositorio
  /** Motivo acotado y saneado de la última comprobación; `null` si no hay. */
  detalle: string | null
  /** RFC3339 de la última comprobación; `null` si nunca se comprobó. */
  comprobadoEn: string | null
}

/**
 * El estado de salud de un repositorio, con su **texto**, su motivo y la fecha
 * de la última comprobación.
 *
 * El color del `Badge` es un refuerzo, no el mensaje: el estado se nombra
 * siempre con la etiqueta (`services/salud.ts`), que es lo único que un lector
 * de pantalla —o quien no distinga los colores— puede leer. La fecha usa el
 * `—` del panel cuando no hubo comprobación: nunca una fecha inventada.
 */
export function IndicadorDeSalud({ salud, detalle, comprobadoEn }: PropsDeIndicadorDeSalud) {
  return (
    <div>
      <Badge status={colorDeSalud(salud)} text={etiquetaDeSalud(salud)} />
      {detalle !== null && detalle !== '' && (
        <Typography.Text type="secondary" style={ESTILO_SECUNDARIO}>
          {detalle}
        </Typography.Text>
      )}
      <Typography.Text type="secondary" style={ESTILO_SECUNDARIO}>
        Última comprobación: {formatearFecha(comprobadoEn)}
      </Typography.Text>
    </div>
  )
}
