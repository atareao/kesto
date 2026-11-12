import { Empty, Progress, Space, Typography } from 'antd'
import { formatearBytes } from '../../services/format'
import { etiquetaDeEstado } from '../../services/estados'
import { useEventStream } from '../../services/useEventStream'

/**
 * El avance de la copia en curso, alimentado por el stream SSE.
 *
 * Se queda con el último evento de una ejecución `pending` o `running`: los
 * eventos de ejecuciones terminadas no son «la copia en curso». Un
 * `percentage` nulo se pinta como barra **indeterminada** —nunca como `0`—,
 * porque el motor lo publica nulo cuando no conoce el total.
 */
export function ProgresoEnVivo() {
  const { ejecuciones } = useEventStream()

  const enCurso =
    [...ejecuciones.values()]
      .filter((evento) => evento.status === 'pending' || evento.status === 'running')
      .at(-1) ?? null

  if (enCurso === null) {
    return <Empty description="No hay ninguna copia en curso" />
  }

  const indeterminado = enCurso.percentage === null

  return (
    <section aria-label="Progreso de la copia en curso">
      <Space direction="vertical" size="small" style={{ width: '100%' }}>
        <Typography.Text strong>
          {etiquetaDeEstado(enCurso.status)} · {enCurso.current_action}
        </Typography.Text>

        <div data-testid="progreso-en-vivo" data-indeterminado={indeterminado ? 'true' : 'false'}>
          {indeterminado ? (
            <>
              <Progress
                percent={100}
                showInfo={false}
                status="active"
                className="progreso-indeterminado"
                aria-label="Progreso de la copia (indeterminado)"
              />
              <Typography.Text type="secondary">Progreso indeterminado</Typography.Text>
            </>
          ) : (
            <Progress
              percent={enCurso.percentage ?? 0}
              status="active"
              aria-label="Progreso de la copia"
            />
          )}
        </div>

        <Typography.Text type="secondary">
          {formatearBytes(enCurso.bytes_uploaded)} subidos de{' '}
          {formatearBytes(enCurso.bytes_processed)} procesados
        </Typography.Text>
      </Space>
    </section>
  )
}
