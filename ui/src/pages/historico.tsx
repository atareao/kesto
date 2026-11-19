import { useState } from 'react'
import { Badge, Button, Empty, Select, Space, Table, Typography } from 'antd'
import type { TableProps } from 'antd'
import { Cargando } from '../components/Cargando/Cargando'
import { ConsolaDeEjecucion } from '../components/ConsolaDeEjecucion/ConsolaDeEjecucion'
import { EstadoDeError } from '../components/EstadoDeError/EstadoDeError'
import type { CambioDePagina } from '../components/Paginador/Paginador'
import {
  Paginador,
  TAMANO_DE_PAGINA_POR_DEFECTO,
  TEXTO_DE_PAGINA_VACIA,
} from '../components/Paginador/Paginador'
import { useEjecuciones } from '../hooks/useEjecuciones'
import { useTareas } from '../hooks/useTareas'
import type { Estado, Execution } from '../services/api/types'
import { colorDeEstado, etiquetaDeEstado } from '../services/estados'
import { mensajeDeError } from '../services/errores'
import { formatearBytes, formatearDuracion, formatearFecha, formatearUuid } from '../services/format'

const ESTADOS: readonly Estado[] = ['pending', 'running', 'success', 'failed']

/**
 * Histórico (§6): ejecuciones paginadas, con filtros por tarea y estado, y la
 * consola en vivo de la ejecución elegida.
 *
 * La paginación es **del servidor**: el paginador de Antd se alimenta del `total`
 * que publica la API y cada cambio de página o de tamaño se traduce en un
 * `limit`/`offset` con el que se pide la página de verdad. No se recorta en
 * cliente: el número de páginas sale del `total` del backend, no del tamaño de
 * la lista recibida. Al cambiar un filtro se vuelve a la primera página, porque
 * un `offset` heredado sobre un resultado acotado daría una página vacía.
 */
export default function Historico() {
  const [jobId, setJobId] = useState<string | undefined>(undefined)
  const [estado, setEstado] = useState<Estado | undefined>(undefined)
  const [pagina, setPagina] = useState(1)
  const [tamanoDePagina, setTamanoDePagina] = useState(TAMANO_DE_PAGINA_POR_DEFECTO)
  const [seleccionadaId, setSeleccionadaId] = useState<string | null>(null)

  const tareas = useTareas()
  const ejecuciones = useEjecuciones({
    job_id: jobId,
    status: estado,
    limit: tamanoDePagina,
    offset: (pagina - 1) * tamanoDePagina,
  })

  const datos = ejecuciones.data?.executions ?? []
  const total = ejecuciones.data?.total ?? 0

  // Solo se guarda el id: la fila se **deriva** de los datos vivos para que un
  // refresco traiga el `status`/`error_message` actuales. Congelar la fila al
  // pulsar «Ver consola» dejaba una ejecución `running` que luego falla sonando
  // «todavía no hay eventos» para siempre.
  const seleccionada = datos.find((fila) => fila.id === seleccionadaId) ?? null

  // La opción se lee por el nombre de la tarea y, sin él, por su identificador:
  // el nombre del target es justo lo que hacía indistinguibles dos tareas sobre
  // el mismo origen. El `value` sigue siendo el `job_id` que filtra la API.
  const opcionesDeTarea = (tareas.data ?? []).map((tarea) => ({
    value: tarea.id,
    label: tarea.name ?? formatearUuid(tarea.id),
  }))

  const columnas: TableProps<Execution>['columns'] = [
    {
      title: 'Tarea',
      key: 'tarea',
      // Sin nombre, cae al identificador de la tarea: el del target no dice de
      // cuál de las tareas sobre ese target es la ejecución.
      render: (_valor, ejecucion) =>
        ejecucion.job_name ?? formatearUuid(ejecucion.job_id),
    },
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
        <Button size="small" onClick={() => setSeleccionadaId(ejecucion.id)}>
          Ver consola
        </Button>
      ),
    },
  ]

  const cambiarTarea = (valor: string | undefined) => {
    setJobId(valor)
    setPagina(1)
  }

  const cambiarEstado = (valor: Estado | undefined) => {
    setEstado(valor)
    setPagina(1)
  }

  /**
   * El paginador ya decidió a qué página ir —y, al cambiar el tamaño, vuelve a
   * la primera—: aquí solo se guardan las dos cifras y el `offset` se deriva.
   */
  const cambiarPagina = (cambio: CambioDePagina) => {
    setPagina(cambio.pagina)
    setTamanoDePagina(cambio.tamanoDePagina)
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
      ) : (
        <>
          {datos.length === 0 ? (
            <Empty
              description={
                pagina > 1 ? TEXTO_DE_PAGINA_VACIA : 'No hay ejecuciones para estos filtros'
              }
            />
          ) : (
            <Table<Execution>
              aria-label="Ejecuciones"
              rowKey="id"
              columns={columnas}
              dataSource={datos}
              pagination={false}
              size="small"
            />
          )}

          {/* El paginador va **fuera** del vacío: si la página encoge y esta
              queda sin filas, sigue a la vista y el operador puede volver a la
              primera. Se pinta cuando la API declara algo que paginar **o**
              cuando el operador está fuera de la primera página y la purga dejó
              el conjunto a cero: sin él no tendría control para retroceder. */}
          {total > 0 || pagina > 1 ? (
            <Paginador
              total={total}
              pagina={pagina}
              tamanoDePagina={tamanoDePagina}
              onCambiar={cambiarPagina}
            />
          ) : null}
        </>
      )}

      <Typography.Title level={2} style={{ marginTop: 24 }}>
        Consola de la ejecución
      </Typography.Title>
      <ConsolaDeEjecucion
        executionId={seleccionadaId}
        status={seleccionada?.status ?? null}
        errorMessage={seleccionada?.error_message ?? null}
      />
    </section>
  )
}
