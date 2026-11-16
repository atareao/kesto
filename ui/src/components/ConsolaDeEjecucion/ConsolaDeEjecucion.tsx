import { Alert, Empty, Space, Typography, theme } from 'antd'
import type { Estado, EventoDeEjecucion } from '../../services/api/types'
import { etiquetaDeEstado } from '../../services/estados'
import { formatearBytes } from '../../services/format'
import { useLineasDeEjecucion } from '../../services/useEventStream'

export interface PropsDeConsolaDeEjecucion {
  /** Ejecución cuya consola se sigue; `null` mientras no se elige ninguna. */
  executionId: string | null
  /**
   * `status` de la fila elegida, tal cual viaja en el Histórico. Es lo que
   * distingue una ejecución viva —que aún emitirá eventos— de una terminada,
   * que ya no los emitirá y por eso merece un resumen. `null` mientras no hay
   * selección.
   */
  status: Estado | null
  /**
   * `error_message` de la fila elegida. Con `status = failed` y una ejecución
   * terminada, es el resumen de la consola. `null` cuando no lo hay.
   */
  errorMessage: string | null
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
 * Resumen de una ejecución terminada que no dejó líneas en el historial en vivo.
 *
 * Solo cuenta lo que la fila ya sabe: nada de inventar eventos. Si falló y trae
 * `error_message`, ese es el resumen; si no, se dice claro que no hay eventos.
 */
function ResumenDeEjecucion({
  status,
  errorMessage,
}: {
  status: Estado | null
  errorMessage: string | null
}) {
  if (status === 'failed' && errorMessage !== null && errorMessage !== '') {
    return <Alert type="error" showIcon message="La ejecución falló" description={errorMessage} />
  }

  return (
    <Typography.Text type="secondary">
      La ejecución ha terminado y no hay eventos que mostrar.
    </Typography.Text>
  )
}

/**
 * Consola de una ejecución, alimentada por el stream SSE.
 *
 * Solo pinta los eventos cuyo `execution_id` es el elegido: los de otra copia no
 * aparecen, aunque compartan conexión. El histórico lo lleva el proveedor con un
 * tope de líneas, así que aquí no hay que recortar nada más. Si el stream se
 * cae, se avisa: una consola congelada sin explicación parece una copia parada.
 *
 * Cuando la ejecución elegida **ya terminó** y no hay líneas, la consola no se
 * queda en blanco: pinta un resumen a partir de la fila —su `error_message` si
 * falló, o un texto llano si no hay nada que contar—. El resumen no inventa
 * líneas: nunca pinta eventos que el panel no recibió.
 */
export function ConsolaDeEjecucion({
  executionId,
  status,
  errorMessage,
}: PropsDeConsolaDeEjecucion) {
  const { token } = theme.useToken()
  const { estado, lineas } = useLineasDeEjecucion(executionId ?? '')

  if (executionId === null) {
    return <Empty description="Selecciona una ejecución para ver su consola" />
  }

  // `success` y `failed` son terminales: no volverán a emitir. `pending` y
  // `running` siguen vivas, así que su consola solo espera eventos.
  const terminada = status === 'success' || status === 'failed'

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
        terminada ? (
          <ResumenDeEjecucion status={status} errorMessage={errorMessage} />
        ) : (
          <Typography.Text type="secondary">
            Todavía no hay eventos para esta ejecución.
          </Typography.Text>
        )
      ) : (
        <div
          data-testid="consola"
          role="log"
          aria-label="Consola de la ejecución"
          style={{
            maxHeight: 320,
            overflowY: 'auto',
            // La consola mantiene un fondo «de terminal» en ambos temas, pero
            // derivado del tema activo: `colorBgSpotlight` (superficie oscura en
            // claro, gris oscuro en oscuro) con su texto sólido. Fijar un gris a
            // mano era justo lo que la spec prohíbe.
            background: token.colorBgSpotlight,
            color: token.colorTextLightSolid,
            fontFamily: 'monospace',
            fontSize: 12,
            lineHeight: 1.6,
            padding: 12,
            borderRadius: token.borderRadiusLG,
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
