import { useState } from 'react'
import { Badge, Button, Empty, Select, Space, Table, Typography } from 'antd'
import type { TableProps } from 'antd'
import { Cargando } from '../components/Cargando/Cargando'
import { EstadoDeError } from '../components/EstadoDeError/EstadoDeError'
import { useRestauraciones } from '../hooks/useRestauraciones'
import { useTargets } from '../hooks/useTargets'
import type { Estado, RestauracionRegistrada } from '../services/api/types'
import { colorDeEstado, etiquetaDeEstado } from '../services/estados'
import { mensajeDeError } from '../services/errores'
import { formatearFecha, formatearUuid, SIN_VALOR } from '../services/format'

/** Filas por página. El contrato admite de 1 a 200; 20 llena una pantalla. */
export const TAMANO_DE_PAGINA = 20

const ESTADOS: readonly Estado[] = ['pending', 'running', 'success', 'failed']

/**
 * El nombre del target de la fila. Sin nombre cae al identificador del target y,
 * si el target se borró, a un guion: el histórico **no** inventa un nombre, pero
 * tampoco deja la celda en blanco.
 */
function nombreDelTarget(fila: RestauracionRegistrada): string {
  if (fila.target_name !== null) return fila.target_name
  if (fila.target_id !== null) return formatearUuid(fila.target_id)
  return SIN_VALOR
}

/**
 * Restauraciones (§6): el histórico de las ejecuciones de restauración.
 *
 * El estado que se pinta es el del **servidor** —la fila persistida—, no un
 * «lanzada» optimista: una restauración puede fallar y el panel tiene que poder
 * decir por qué. Las filas `pending` y `running` cambian solas, así que el
 * listado se refresca sin recargar la página (`useRestauraciones` programa el
 * refresco) y, además, se puede forzar con «Actualizar».
 *
 * La paginación es por `limit`/`offset`. A diferencia del histórico de
 * ejecuciones, aquí la respuesta **sí** trae el total, así que «Siguiente» se
 * apaga con el total y no con la heurística de «página incompleta».
 *
 * El filtro por target se elige en un selector cuyas opciones vienen del catálogo
 * (`GET /api/v1/targets`) y viaja a la API como `target_id`. El elegido vive en
 * estado **propio** y no se deriva del catálogo: si el target se borra en otra
 * pestaña, la pantalla conserva el filtro y pinta su vacío en vez de reconciliar
 * opciones y romperse.
 */
export default function Restauraciones() {
  const [estado, setEstado] = useState<Estado | undefined>(undefined)
  const [targetId, setTargetId] = useState<string | undefined>(undefined)
  const [offset, setOffset] = useState(0)

  const targets = useTargets()

  const restauraciones = useRestauraciones({
    status: estado,
    target_id: targetId,
    limit: TAMANO_DE_PAGINA,
    offset,
  })

  const datos = restauraciones.data?.restorations ?? []
  const total = restauraciones.data?.total ?? 0
  const haySiguiente = offset + datos.length < total

  const columnas: TableProps<RestauracionRegistrada>['columns'] = [
    {
      title: 'Target',
      key: 'target',
      render: (_valor, fila) => nombreDelTarget(fila),
    },
    {
      title: 'Tipo',
      key: 'tipo',
      render: (_valor, fila) =>
        fila.target_type === null ? (
          <Typography.Text type="secondary">{SIN_VALOR}</Typography.Text>
        ) : (
          fila.target_type
        ),
    },
    {
      title: 'Estado',
      key: 'estado',
      render: (_valor, fila) => (
        <Badge status={colorDeEstado(fila.status)} text={etiquetaDeEstado(fila.status)} />
      ),
    },
    {
      title: 'Instantánea',
      key: 'snapshot',
      // El identificador se pinta **completo**: es el hexadecimal con el que
      // rustic nombra la instantánea y acortarlo lo haría inservible.
      render: (_valor, fila) => <Typography.Text code>{fila.snapshot_id}</Typography.Text>,
    },
    {
      title: 'Inicio',
      key: 'inicio',
      render: (_valor, fila) => formatearFecha(fila.started_at),
    },
    {
      title: 'Fin',
      key: 'fin',
      render: (_valor, fila) => formatearFecha(fila.finished_at),
    },
    {
      title: 'Motivo',
      key: 'motivo',
      // Una restauración `failed` explica por qué con el `error_message` de la
      // API. En el resto de estados no hay motivo que enseñar.
      render: (_valor, fila) =>
        fila.error_message === null ? (
          <Typography.Text type="secondary">{SIN_VALOR}</Typography.Text>
        ) : (
          <Typography.Text type="danger">{fila.error_message}</Typography.Text>
        ),
    },
  ]

  const cambiarEstado = (valor: Estado | undefined) => {
    setEstado(valor)
    setOffset(0)
  }

  const cambiarTarget = (valor: string | undefined) => {
    setTargetId(valor)
    setOffset(0)
  }

  return (
    <section aria-labelledby="titulo-de-la-pantalla">
      <Typography.Title id="titulo-de-la-pantalla" level={1} style={{ marginTop: 0 }}>
        Restauraciones
      </Typography.Title>

      <Space size="middle" wrap style={{ marginBottom: 16 }}>
        <Select
          allowClear
          aria-label="Target"
          placeholder="Todos los targets"
          style={{ minWidth: 200 }}
          value={targetId}
          onChange={cambiarTarget}
          options={(targets.data ?? []).map((target) => ({
            value: target.id,
            label: target.name,
          }))}
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
        <Button onClick={() => void restauraciones.refetch()} loading={restauraciones.isRefetching}>
          Actualizar
        </Button>
      </Space>

      {restauraciones.error !== null ? (
        <EstadoDeError
          titulo="No se pudieron cargar las restauraciones"
          mensaje={mensajeDeError(restauraciones.error)}
          onReintentar={() => void restauraciones.refetch()}
        />
      ) : restauraciones.isPending ? (
        <Cargando />
      ) : datos.length === 0 ? (
        <Empty description="No hay restauraciones para estos filtros" />
      ) : (
        <>
          <Table<RestauracionRegistrada>
            aria-label="Restauraciones"
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
              Página {Math.floor(offset / TAMANO_DE_PAGINA) + 1} de {Math.max(1, Math.ceil(total / TAMANO_DE_PAGINA))}
            </Typography.Text>
          </Space>
        </>
      )}
    </section>
  )
}
