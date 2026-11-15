import { Flex, Typography } from 'antd'
import { ProgresoEnVivo } from '../components/ProgresoEnVivo/ProgresoEnVivo'
import { TablaDeTareasEnCurso } from '../components/TablaDeTareasEnCurso/TablaDeTareasEnCurso'
import { TarjetasDeResumen } from '../components/TarjetasDeResumen/TarjetasDeResumen'

/**
 * Dashboard (§6): el resumen del sistema y el avance de la copia en curso.
 *
 * Las tarjetas y la tabla se alimentan de la API; el `Progress` se alimenta del
 * stream SSE, que es la única fuente que sabe de una copia que está pasando
 * ahora mismo.
 *
 * El espaciado entre bloques sale de un `Flex` vertical en vez de márgenes a
 * mano en cada título: así la separación es la misma en todo el panel y no hay
 * `marginTop` que descuadre un tema respecto al otro.
 */
export default function Dashboard() {
  return (
    <section aria-labelledby="titulo-de-la-pantalla">
      <Flex vertical gap={24}>
        <Typography.Title id="titulo-de-la-pantalla" level={1} style={{ margin: 0 }}>
          Dashboard
        </Typography.Title>

        <TarjetasDeResumen />

        <Flex vertical gap={12}>
          <Typography.Title level={2} style={{ margin: 0 }}>
            Copia en curso
          </Typography.Title>
          <ProgresoEnVivo />
        </Flex>

        <Flex vertical gap={12}>
          <Typography.Title level={2} style={{ margin: 0 }}>
            Tareas activas
          </Typography.Title>
          <TablaDeTareasEnCurso />
        </Flex>
      </Flex>
    </section>
  )
}
