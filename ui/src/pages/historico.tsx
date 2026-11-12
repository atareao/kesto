import { useState } from 'react'
import { Badge, Button, Empty, Select, Space, Table, Typography } from 'antd'
import type { TableProps } from 'antd'
import { Cargando } from '../components/Cargando/Cargando'
import { ConsolaDeEjecucion } from '../components/ConsolaDeEjecucion/ConsolaDeEjecucion'
import { EstadoDeError } from '../components/EstadoDeError/EstadoDeError'
import { useEjecuciones } from '../hooks/useEjecuciones'
import { useTareas } from '../hooks/useTareas'
import { useTargets } from '../hooks/useTargets'
import type { Estado, Execution } from '../services/api/types'
import { colorDeEstado, etiquetaDeEstado } from '../services/estados'
import { mensajeDeError } from '../services/errores'
import { formatearBytes, formatearDuracion, formatearFecha, formatearUuid } from '../services/format'

/** Filas por página. El contrato admite de 1 a 200; 20 llena una pantalla. */
export const TAMANO_DE_PAGINA = 20

const ESTADOS: readonly Estado[] = ['pending', 'running', 'success', 'failed']

/**
 * Histórico (§6): ejecuciones paginadas, con filtros por tarea y estado, y la
 * consola en vivo de la ejecución elegida.
 *
 * La paginación es por `limit`/`offset` y no por páginas numeradas: la API no
 * publica un total. Por eso «Siguiente» se apaga cuando la página llega
 * incompleta —la heurística de «no hay más» que el contrato permite— y
 * «Anterior» cuando ya se está en la primera.
 */
export default function Historico() {
  const [jobId, setJobId] = useState<string | undefined>(undefined)
  const [estado, setEstado] = useState<Estado | undefined>(undefined)
  const [offset, setOffset] = useState(0)
  const [seleccionada, setSeleccionada] = useState<string | null>(null)

  const tareas = useTareas()
  const targets = useTargets()
  const ejecuciones = useEjecuciones({
    job_id: jobId,
    status: estado,
    limit: TAMANO_DE_PAGINA,
    offset,
  })

  const datos = ejecuciones.data ?? []
  const haySiguiente = datos.length === TAMANO_DE_PAGINA

  const nombresDeTarget = new Map((targets.data ?? []).map((target) => [target.id, target.name]))
  const opcionesDeTarea = (tareas.data ?? []).map((tarea) => ({
    value: tarea.id,
    label: nombresDeTarget.get(tarea.target_id) ?? formatearUuid(tarea.target_id),
  }))

  const columnas: TableProps<Execution>['columns'] = [
    {
      title: 'Inicio',
      key: 'inicio',
      render: (_valor, ejecucion) => formatearFecha(ejecucion.started_at),
    },
    {
      title: 'Estado',
      key: 'estado',
      render: (_valor, ejecucion) => (
        <Badge
          status={colorDeEstado(ejecucion.status)}
          text={etiquetaDeEstado(ejecucion.status)}
        />
      ),
    },
    {
      title: 'Duración',
      key: 'duracion',
      render: (_valor, ejecucion) =>
        formatearDuracion(ejecucion.started_at, ejecucion.finished_at),
    },
    {
      title: 'Procesado',
      key: 'procesado',
      render: (_valor, ejecucion) => formatearBytes(ejecucion.bytes_processed),
    },
    {
      title: 'Subido',
      key: 'subido',
      render: (_valor, ejecucion) => formatearBytes(ejecucion.bytes_uploaded),
    },
    {
      title: 'Consola',
      key: 'consola',
      render: (_valor, ejecucion) => (
        <Button size="small" onClick={() => setSeleccionada(ejecucion.id)}>
          Ver consola
        </Button>
      ),
    },
  ]

  const cambiarTarea = (valor: string | undefined) => {
    setJobId(valor)
    setOffset(0)
  }

  const cambiarEstado = (valor: Estado | undefined) => {
    setEstado(valor)
    setOffset(0)
  }

  return (
    <section aria-labelledby="titulo-de-la-pantalla">
      <Typography.Title id="titulo-de-la-pantalla" level={1} style={{ marginTop: 0 }}>
        Histórico
      </Typography.Title>

      <Space size="middle" wrap style={{ marginBottom: 16 }}>
        <Select
          allowClear
          aria-label="Tarea"
          placeholder="Todas las tareas"
          style={{ minWidth: 200 }}
          value={jobId}
          onChange={cambiarTarea}
          options={opcionesDeTarea}
        />
        <Select
          allowClear
          aria-label="Estado"
          placeholder="Todos los estados"
          style={{ minWidth: 180 }}
          value={estado}
          onChange={cambiarEstado}
          options={ESTADOS.map((valor) => ({ value: valor, label: etiquetaDeEstado(valor) }))}
        />
      </Space>

      {ejecuciones.error !== null ? (
        <EstadoDeError
          titulo="No se pudieron cargar las ejecuciones"
          mensaje={mensajeDeError(ejecuciones.error)}
          onReintentar={() => void ejecuciones.refetch()}
        />
      ) : ejecuciones.isPending ? (
        <Cargando />
      ) : datos.length === 0 ? (
        <Empty description="No hay ejecuciones para estos filtros" />
      ) : (
        <>
          <Table<Execution>
            aria-label="Ejecuciones"
            rowKey="id"
            columns={columnas}
            dataSource={datos}
            pagination={false}
            size="small"
          />

          <Space style={{ marginTop: 16 }}>
            <Button
              onClick={() => setOffset((previo) => Math.max(0, previo - TAMANO_DE_PAGINA))}
              disabled={offset === 0}
            >
              Anterior
            </Button>
            <Button
              onClick={() => setOffset((previo) => previo + TAMANO_DE_PAGINA)}
              disabled={!haySiguiente}
            >
              Siguiente
            </Button>
            <Typography.Text type="secondary">
              Página {Math.floor(offset / TAMANO_DE_PAGINA) + 1}
            </Typography.Text>
          </Space>
        </>
      )}

      <Typography.Title level={2} style={{ marginTop: 24 }}>
        Consola de la ejecución
      </Typography.Title>
      <ConsolaDeEjecucion executionId={seleccionada} />
    </section>
  )
}
