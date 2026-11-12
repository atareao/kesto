import { Alert, Badge, Empty, Table, Typography } from 'antd'
import type { TableProps } from 'antd'
import { useEjecuciones } from '../../hooks/useEjecuciones'
import { useTareas } from '../../hooks/useTareas'
import { useTargets } from '../../hooks/useTargets'
import type { EventoDeEjecucion, Execution, Job } from '../../services/api/types'
import { colorDeEstado, etiquetaDeEstado } from '../../services/estados'
import type { EstadoDeTarea } from '../../services/estados'
import { formatearFecha, formatearUuid } from '../../services/format'
import { mensajeDeError } from '../../services/errores'
import { useEventStream } from '../../services/useEventStream'

/** Una fila de la tabla del Dashboard. */
interface FilaDeTarea {
  key: string
  tarea: Job
  nombre: string
  estado: EstadoDeTarea
  porcentaje: number | null
  ultimaEjecucion: Execution | null
}

/** La ejecución más reciente de cada tarea, comparando por `started_at`. */
function ultimasPorTarea(ejecuciones: readonly Execution[]): Map<string, Execution> {
  const ultimas = new Map<string, Execution>()

  for (const ejecucion of ejecuciones) {
    const previa = ultimas.get(ejecucion.job_id)
    if (previa === undefined || Date.parse(ejecucion.started_at) >= Date.parse(previa.started_at)) {
      ultimas.set(ejecucion.job_id, ejecucion)
    }
  }

  return ultimas
}

/** El último evento SSE de cada tarea; manda sobre el histórico. */
function eventosPorTarea(eventos: ReadonlyMap<string, EventoDeEjecucion>): Map<string, EventoDeEjecucion> {
  const porTarea = new Map<string, EventoDeEjecucion>()
  for (const evento of eventos.values()) porTarea.set(evento.job_id, evento)
  return porTarea
}

/**
 * Tabla de tareas activas con su estado más reciente.
 *
 * El estado sale de la última ejecución; un evento SSE de esa tarea lo pisa
 * mientras la copia está viva. Una tarea que nunca se ha ejecutado no vale
 * «correcta»: se pinta «sin ejecuciones». El `Badge` siempre lleva texto: el
 * color no es el único portador del estado.
 */
export function TablaDeTareasEnCurso() {
  const tareas = useTareas()
  const targets = useTargets()
  const ejecuciones = useEjecuciones({ limit: 200 })
  const { ejecuciones: eventos } = useEventStream()

  const ultimas = ultimasPorTarea(ejecuciones.data ?? [])
  const porTarea = eventosPorTarea(eventos)
  const nombresDeTarget = new Map((targets.data ?? []).map((target) => [target.id, target.name]))

  const filas: FilaDeTarea[] = (tareas.data ?? [])
    .filter((tarea) => tarea.enabled)
    .map((tarea) => {
      const evento = porTarea.get(tarea.id)
      const ultima = ultimas.get(tarea.id) ?? null
      const estado: EstadoDeTarea = evento?.status ?? ultima?.status ?? 'sin-ejecuciones'

      return {
        key: tarea.id,
        tarea,
        nombre: nombresDeTarget.get(tarea.target_id) ?? formatearUuid(tarea.target_id),
        estado,
        porcentaje: evento?.percentage ?? null,
        ultimaEjecucion: ultima,
      }
    })

  const columnas: TableProps<FilaDeTarea>['columns'] = [
    { title: 'Tarea', dataIndex: 'nombre', key: 'nombre' },
    {
      title: 'Estado',
      key: 'estado',
      render: (_valor, fila) => (
        <Badge status={colorDeEstado(fila.estado)} text={etiquetaDeEstado(fila.estado)} />
      ),
    },
    {
      title: 'Última ejecución',
      key: 'ultima',
      render: (_valor, fila) => formatearFecha(fila.ultimaEjecucion?.started_at),
    },
    {
      title: 'Progreso',
      key: 'progreso',
      render: (_valor, fila) =>
        fila.porcentaje === null ? (
          <Typography.Text type="secondary">—</Typography.Text>
        ) : (
          <Typography.Text>{`${fila.porcentaje}%`}</Typography.Text>
        ),
    },
  ]

  if (tareas.error) {
    return (
      <Alert
        type="error"
        showIcon
        message="No se pudieron cargar las tareas"
        description={mensajeDeError(tareas.error)}
      />
    )
  }

  return (
    <Table<FilaDeTarea>
      aria-label="Tareas activas"
      rowKey="key"
      columns={columnas}
      dataSource={filas}
      loading={tareas.isPending || targets.isPending || ejecuciones.isPending}
      pagination={false}
      size="small"
      locale={{ emptyText: <Empty description="No hay tareas activas" /> }}
    />
  )
}
