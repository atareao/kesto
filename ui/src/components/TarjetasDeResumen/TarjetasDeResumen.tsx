import { Alert, Button, Card, Col, Flex, Row, Skeleton, Statistic } from 'antd'
import { useQueryClient } from '@tanstack/react-query'
import { useEspacioEnS3 } from '../../hooks/useEspacioEnS3'
import { useRepositorios } from '../../hooks/useRepositorios'
import { useTareas } from '../../hooks/useTareas'
import { mensajeDeError } from '../../services/errores'
import { formatearBytes } from '../../services/format'

/**
 * Nombra los repositorios que no respondieron, entre comillas y separados por
 * comas: es lo que el aviso tiene que decir para señalar el fallo sin sustituir
 * el resumen entero.
 */
function nombrarRepositorios(nombres: readonly string[]): string {
  return nombres.map((nombre) => `«${nombre}»`).join(', ')
}

/**
 * Las tres tarjetas del Dashboard: repositorios, tareas activas y espacio
 * consumido en S3.
 *
 * El espacio es la suma de los `bytes` de las instantáneas de **todos** los
 * repositorios. La cifra se presenta como **mínimo** —«al menos»— cuando algún
 * listado vuelve recortado (`truncated`) **o** cuando algún repositorio no se
 * pudo leer: en los dos casos la suma es parcial y darla como total sería
 * mentir con un número.
 *
 * El resumen **no es todo-o-nada**: el fallo de un repositorio no borra la
 * información de los demás. Las cifras que no dependen del S3 se siguen
 * mostrando y el fallo se anuncia en un aviso que **nombra** el repositorio
 * caído y ofrece reintentar, en vez de cambiar las tres tarjetas por un único
 * error global.
 */
export function TarjetasDeResumen() {
  const cliente = useQueryClient()
  const repositorios = useRepositorios()
  const tareas = useTareas()
  const espacio = useEspacioEnS3(repositorios.data ?? [])

  // Solo la carga de las cifras que **no** dependen del S3 bloquea las tarjetas.
  // El espacio se pinta aparte: mientras se resuelve, su tarjeta enseña carga,
  // pero las otras dos ya tienen su cifra.
  if (repositorios.isPending || tareas.isPending) {
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

  // El fallo de las cifras que **no** dependen del S3 sí deja el resumen sin
  // nada que enseñar. El del espacio, en cambio, es **acotado** y se resuelve
  // con el aviso de abajo, no colapsando las tres tarjetas.
  const errorBase = repositorios.error ?? tareas.error
  if (errorBase !== null && errorBase !== undefined) {
    return (
      <Alert
        type="error"
        showIcon
        message="No se pudo cargar el resumen"
        description={mensajeDeError(errorBase)}
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
  const caidos = espacio.repositoriosCaidos

  return (
    <Flex vertical gap={16}>
      {caidos.length > 0 && (
        <Alert
          data-testid="aviso-de-repositorios-caidos"
          type="warning"
          showIcon
          message={`No se pudo leer el espacio de ${nombrarRepositorios(caidos)}.`}
          description="Las cifras que dependen del S3 son un mínimo; el resto del resumen no se ve afectado."
          action={
            <Button size="small" onClick={espacio.reintentar}>
              Reintentar
            </Button>
          }
        />
      )}

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
          <Card data-testid="tarjeta-espacio" aria-busy={espacio.cargando ? true : undefined}>
            {espacio.cargando ? (
              // El estado de carga tiene que ser anunciable, no un esqueleto
              // mudo: `role="status"` con un texto `sr-only` es el mismo patrón
              // que usa el resto del panel (p. ej. la pantalla de sesión).
              <div role="status" aria-busy="true">
                <span className="sr-only">Cargando el espacio consumido en S3…</span>
                <Skeleton active paragraph={{ rows: 1 }} title={{ width: '60%' }} />
              </div>
            ) : (
              <Statistic
                title={espacio.esMinimo ? 'Espacio en S3 (al menos)' : 'Espacio en S3'}
                value={formatearBytes(espacio.bytes)}
              />
            )}
          </Card>
        </Col>
      </Row>
    </Flex>
  )
}
