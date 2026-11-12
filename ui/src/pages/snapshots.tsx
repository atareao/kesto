import { useState } from 'react'
import { Alert, Empty, Select, Space, Table, Typography } from 'antd'
import type { TableProps } from 'antd'
import { Cargando } from '../components/Cargando/Cargando'
import { EstadoDeError } from '../components/EstadoDeError/EstadoDeError'
import { useRepositorios } from '../hooks/useRepositorios'
import { useSnapshots } from '../hooks/useSnapshots'
import type { Snapshot } from '../services/api/types'
import { mensajeDeError } from '../services/errores'
import { formatearBytes, formatearFecha } from '../services/format'

/**
 * Snapshots (§6): las instantáneas de un repositorio.
 *
 * El identificador se pinta **completo**: es el hexadecimal con el que rustic
 * nombra la instantánea y acortarlo lo haría inservible para buscarla.
 *
 * No hay botón de purga a propósito: el contrato no publica un endpoint de
 * borrado de instantáneas, y un botón que no puede hacer nada es peor que
 * ninguno.
 */
export default function Snapshots() {
  const repositorios = useRepositorios()
  const [seleccionado, setSeleccionado] = useState<string | null>(null)

  // Sin elección explícita, se usa el primero: una pantalla que obliga a elegir
  // cuando solo hay un repositorio es un paso de más.
  const repositoryId = seleccionado ?? repositorios.data?.[0]?.id ?? null
  const snapshots = useSnapshots(repositoryId)

  const columnas: TableProps<Snapshot>['columns'] = [
    {
      title: 'Identificador',
      key: 'id',
      render: (_valor, snapshot) => (
        <Typography.Text code copyable>
          {snapshot.id}
        </Typography.Text>
      ),
    },
    {
      title: 'Fecha',
      key: 'fecha',
      render: (_valor, snapshot) => formatearFecha(snapshot.time),
    },
    {
      title: 'Etiqueta',
      key: 'etiqueta',
      render: (_valor, snapshot) =>
        snapshot.label === null ? (
          <Typography.Text type="secondary">sin etiqueta</Typography.Text>
        ) : (
          snapshot.label
        ),
    },
    {
      title: 'Ficheros',
      key: 'ficheros',
      render: (_valor, snapshot) => snapshot.files.toLocaleString('es-ES'),
    },
    {
      title: 'Bytes',
      key: 'bytes',
      render: (_valor, snapshot) => formatearBytes(snapshot.bytes),
    },
  ]

  return (
    <section aria-labelledby="titulo-de-la-pantalla">
      <Typography.Title id="titulo-de-la-pantalla" level={1} style={{ marginTop: 0 }}>
        Snapshots
      </Typography.Title>

      {repositorios.error !== null ? (
        <EstadoDeError
          titulo="No se pudieron cargar los repositorios"
          mensaje={mensajeDeError(repositorios.error)}
          onReintentar={() => void repositorios.refetch()}
        />
      ) : repositorios.isPending ? (
        <Cargando />
      ) : (repositorios.data ?? []).length === 0 ? (
        <Empty description="No hay repositorios configurados" />
      ) : (
        <>
          <Space style={{ marginBottom: 16 }}>
            <Select
              aria-label="Repositorio"
              style={{ minWidth: 220 }}
              value={repositoryId ?? undefined}
              onChange={(valor: string) => setSeleccionado(valor)}
              options={(repositorios.data ?? []).map((repositorio) => ({
                value: repositorio.id,
                label: repositorio.name,
              }))}
            />
          </Space>

          {snapshots.error !== null ? (
            <EstadoDeError
              titulo="No se pudieron cargar las instantáneas"
              mensaje={mensajeDeError(snapshots.error)}
              onReintentar={() => void snapshots.refetch()}
            />
          ) : snapshots.isPending ? (
            <Cargando />
          ) : (
            <>
              {snapshots.data?.truncated === true && (
                <Alert
                  type="warning"
                  showIcon
                  message="Puede haber más instantáneas de las que se muestran."
                  style={{ marginBottom: 16 }}
                />
              )}

              {(snapshots.data?.snapshots ?? []).length === 0 ? (
                <Empty description="No hay instantáneas en este repositorio" />
              ) : (
                <Table<Snapshot>
                  aria-label="Snapshots del repositorio"
                  rowKey="id"
                  columns={columnas}
                  dataSource={snapshots.data?.snapshots ?? []}
                  pagination={false}
                  size="small"
                />
              )}
            </>
          )}
        </>
      )}
    </section>
  )
}
