import { Empty, Table } from 'antd'
import type { TableProps } from 'antd'
import { Cargando } from '../Cargando/Cargando'
import { EstadoDeError } from '../EstadoDeError/EstadoDeError'
import { IndicadorDeSalud } from '../IndicadorDeSalud/IndicadorDeSalud'
import { useRepositorios } from '../../hooks/useRepositorios'
import type { Repository } from '../../services/api/types'
import { mensajeDeError } from '../../services/errores'

/**
 * Las columnas no dependen de nada del render —no cierran sobre manejadores—,
 * así que se declaran una vez, fuera del componente.
 */
const COLUMNAS: TableProps<Repository>['columns'] = [
  { title: 'Repositorio', dataIndex: 'name', key: 'nombre' },
  {
    title: 'Salud',
    key: 'salud',
    render: (_valor, repositorio) => (
      <IndicadorDeSalud
        salud={repositorio.health}
        detalle={repositorio.health_detail}
        comprobadoEn={repositorio.health_checked_at}
      />
    ),
  },
]

/**
 * El listado de repositorios del Dashboard con su salud **persistida**.
 *
 * Lee el recurso (`GET /api/v1/repositories`) y nada más: **no** dispara
 * ninguna comprobación al cargar. Comprobar cuesta hasta el tope del destino por
 * repositorio y lo pide el operador en Ajustes, no una pantalla que se abre a
 * menudo. Un repositorio sin comprobar llega como «desconocido» y así se pinta
 * —nunca como sano—, y uno caído no borra la fila de los que sí responden: el
 * listado es completo, no todo-o-nada.
 */
export function SaludDeRepositorios() {
  const repositorios = useRepositorios()

  if (repositorios.error !== null) {
    return (
      <EstadoDeError
        titulo="No se pudo cargar la salud de los repositorios"
        mensaje={mensajeDeError(repositorios.error)}
        onReintentar={() => void repositorios.refetch()}
      />
    )
  }

  if (repositorios.isPending) return <Cargando />

  const filas = repositorios.data ?? []
  if (filas.length === 0) return <Empty description="No hay repositorios configurados" />

  return (
    <Table<Repository>
      aria-label="Salud de los repositorios"
      rowKey="id"
      columns={COLUMNAS}
      dataSource={filas}
      pagination={false}
      size="small"
    />
  )
}
