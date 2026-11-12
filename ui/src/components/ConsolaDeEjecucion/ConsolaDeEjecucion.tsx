import { Alert, Empty, Space, Typography } from 'antd'
import type { EventoDeEjecucion } from '../../services/api/types'
import { etiquetaDeEstado } from '../../services/estados'
import { formatearBytes } from '../../services/format'
import { useLineasDeEjecucion } from '../../services/useEventStream'

export interface PropsDeConsolaDeEjecucion {
  /** Ejecución cuya consola se sigue; `null` mientras no se elige ninguna. */
  executionId: string | null
}

/** Una línea de la consola: estado, acción, porcentaje y bytes de ese evento. */
function Linea({ evento }: { evento: EventoDeEjecucion }) {
  const porcentaje = evento.percentage === null ? '—' : `${evento.percentage}%`

  return (
    <div data-testid="linea-de-consola">
      {`${etiquetaDeEstado(evento.status)} · ${evento.current_action} · ${porcentaje} · `}
      {`${formatearBytes(evento.bytes_uploaded)}/${formatearBytes(evento.bytes_processed)}`}
    </div>
  )
}

/**
 * Consola de una ejecución, alimentada por el stream SSE.
 *
 * Solo pinta los eventos cuyo `execution_id` es el elegido: los de otra copia no
 * aparecen, aunque compartan conexión. El histórico lo lleva el proveedor con un
 * tope de líneas, así que aquí no hay que recortar nada más. Si el stream se
 * cae, se avisa: una consola congelada sin explicación parece una copia parada.
 */
export function ConsolaDeEjecucion({ executionId }: PropsDeConsolaDeEjecucion) {
  const { estado, lineas } = useLineasDeEjecucion(executionId ?? '')

  if (executionId === null) {
    return <Empty description="Selecciona una ejecución para ver su consola" />
  }

  return (
    <Space direction="vertical" size="small" style={{ width: '100%' }}>
      {estado === 'reconectando' && (
        <Alert
          type="warning"
          showIcon
          message="Se ha perdido la conexión con el servidor; reconectando…"
        />
      )}

      {lineas.length === 0 ? (
        <Typography.Text type="secondary">
          Todavía no hay eventos para esta ejecución.
        </Typography.Text>
      ) : (
        <div
          data-testid="consola"
          role="log"
          aria-label="Consola de la ejecución"
          style={{
            maxHeight: 320,
            overflowY: 'auto',
            background: '#141414',
            color: '#e6e6e6',
            fontFamily: 'monospace',
            fontSize: 12,
            lineHeight: 1.6,
            padding: 12,
            borderRadius: 8,
          }}
        >
          {lineas.map((evento, indice) => (
            // La consola es un registro append-only: el índice es estable
            // mientras no se recorte por delante, y el recorte reescribe las
            // claves por las que React reconcilia.
            <Linea key={`${indice}-${evento.current_action}`} evento={evento} />
          ))}
        </div>
      )}
    </Space>
  )
}
