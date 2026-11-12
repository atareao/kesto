import { Alert, Button, Card, Col, Row, Skeleton, Statistic } from 'antd'
import { useQueryClient } from '@tanstack/react-query'
import { useEspacioEnS3 } from '../../hooks/useEspacioEnS3'
import { useRepositorios } from '../../hooks/useRepositorios'
import { useTareas } from '../../hooks/useTareas'
import { mensajeDeError } from '../../services/errores'
import { formatearBytes } from '../../services/format'

/**
 * Las tres tarjetas del Dashboard: repositorios, tareas activas y espacio
 * consumido en S3.
 *
 * El espacio es la suma de los `bytes` de las instantáneas de **todos** los
 * repositorios. Si cualquiera de los listados vuelve con `truncated: true`, la
 * cifra es un **mínimo** y el rótulo lo dice; presentarla como total sería
 * mentir con un número.
 */
export function TarjetasDeResumen() {
  const cliente = useQueryClient()
  const repositorios = useRepositorios()
  const tareas = useTareas()
  const espacio = useEspacioEnS3(repositorios.data ?? [])

  const cargando = repositorios.isPending || tareas.isPending || espacio.cargando
  const error = repositorios.error ?? tareas.error ?? espacio.error

  if (cargando) {
    return (
      <Row gutter={[16, 16]}>
        {[0, 1, 2].map((indice) => (
          <Col key={indice} xs={24} sm={8}>
            <Card>
              <Skeleton active paragraph={{ rows: 1 }} title={{ width: '60%' }} />
            </Card>
          </Col>
        ))}
      </Row>
    )
  }

  if (error !== null && error !== undefined) {
    return (
      <Alert
        type="error"
        showIcon
        message="No se pudo cargar el resumen"
        description={mensajeDeError(error)}
        action={
          <Button size="small" onClick={() => void cliente.invalidateQueries()}>
            Reintentar
          </Button>
        }
      />
    )
  }

  const totalRepositorios = repositorios.data?.length ?? 0
  const tareasActivas = (tareas.data ?? []).filter((tarea) => tarea.enabled).length

  return (
    <Row gutter={[16, 16]}>
      <Col xs={24} sm={8}>
        <Card data-testid="tarjeta-repositorios">
          <Statistic title="Repositorios" value={totalRepositorios} />
        </Card>
      </Col>
      <Col xs={24} sm={8}>
        <Card data-testid="tarjeta-tareas">
          <Statistic title="Tareas activas" value={tareasActivas} />
        </Card>
      </Col>
      <Col xs={24} sm={8}>
        <Card data-testid="tarjeta-espacio">
          <Statistic
            title={espacio.truncado ? 'Espacio en S3 (al menos)' : 'Espacio en S3'}
            value={formatearBytes(espacio.bytes)}
          />
        </Card>
      </Col>
    </Row>
  )
}
