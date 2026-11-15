import { useEffect, useState } from 'react'
import { Alert, Button, Empty, Popconfirm, Select, Space, Table, Typography } from 'antd'
import type { TableProps } from 'antd'
import { Cargando } from '../components/Cargando/Cargando'
import { EstadoDeError } from '../components/EstadoDeError/EstadoDeError'
import { useRepositorios } from '../hooks/useRepositorios'
import { useRestaurarSnapshot, useSnapshots } from '../hooks/useSnapshots'
import type { EventoDeRestauracion, Snapshot } from '../services/api/types'
import { mensajeDeError } from '../services/errores'
import { formatearBytes, formatearFecha } from '../services/format'
import { useRestauracion } from '../services/useEventStream'
import type { EstadoDeLaConexion } from '../services/useEventStream'

/** El aviso de la pantalla: el desenlace de una restauración, o el fallo al lanzarla. */
interface Aviso {
  tipo: 'success' | 'error'
  texto: string
}

/** Una restauración en vuelo: lo que se sabe de ella hasta que llegue su desenlace. */
interface EnCurso {
  /** La instantánea que la lanzó: es la fila que muestra el botón en vuelo. */
  snapshotId: string
  /** La ejecución de restauración; `null` hasta que el `POST` contesta. */
  executionId: string | null
  /**
   * El target sobre el que se restaura. Sale del listado —que ya lo publica por
   * fila— y lo confirma la respuesta del `POST` cuando llega, porque es la
   * resolución del vínculo la que manda.
   */
  target: string
  /**
   * El plazo venció sin desenlace: el panel ya no puede afirmar que la
   * restauración siga **en curso** porque no lo sabe.
   */
  sinConfirmacion: boolean
}

/** Las fases con las que el protocolo cierra una restauración. */
const FASES_TERMINALES: readonly string[] = ['terminada', 'fallida']

/**
 * Plazo de confirmación de una restauración lanzada: si el panel no recibe su
 * desenlace dentro de este plazo, deja de afirmar que sigue **en curso** y pasa
 * a decir que **no ha podido confirmarlo**.
 *
 * No es un plazo de fallo y no acota la duración de la operación: el evento
 * `restore_progress` publica **dos** avisos —arranque y cierre— y ninguno lleva
 * avance (los contadores van a `0` a propósito), así que el panel no puede
 * distinguir una restauración lenta de una cuyo desenlace se perdió. Y se pierde
 * de verdad: el stream no guarda replay, el evento incompleto se descarta y uno
 * que no llegue no se reemite, que es lo que dejaba «en curso» para siempre.
 *
 * De ahí que el número sea holgado —**30 minutos**— y no el `ESPERA_MAXIMA_MS`
 * del stream (30 s): ese plazo mide una reconexión, y aplicado aquí vencería en
 * mitad de una restauración sana y diría justo lo contrario de lo que pasa.
 * Treinta minutos quedan por encima de lo que la feature declara que puede tardar
 * una restauración («puede tardar minutos») y del plazo de inactividad del motor
 * (`INACTIVIDAD_MAXIMA`, 5 min), que es el otro reloj de pared de este proyecto.
 * Lo que compra el plazo no es saber qué pasó, sino no mentir: pasado el plazo el
 * aviso es una advertencia que el operador puede cerrar, y el desenlace, si llega
 * tarde, se anuncia igual.
 */
export const PLAZO_DE_CONFIRMACION_MS = 1_800_000

/** El protocolo cierra la restauración: no habrá más avisos de esa fase. */
function esFaseTerminal(phase: string): boolean {
  return FASES_TERMINALES.includes(phase)
}

/**
 * Cómo se lee la fase del protocolo. Sin evento todavía la operación está **en
 * curso**: el `POST` ya se aceptó, pero su primer aviso va de camino.
 *
 * Una fase que el panel no conozca —el contrato puede crecer sin que este panel
 * se entere— se normaliza con un texto propio: el literal crudo del backend no
 * es un aviso, es un dato de cable, y pintarlo dejaría al operador leyendo una
 * palabra suelta que no explica nada.
 */
function textoDeFase(phase: string | null): string {
  switch (phase) {
    case 'terminada':
      return 'terminada'
    case 'fallida':
      return 'fallida'
    case null:
    case 'restaurando':
      return 'en curso'
    default:
      return 'estado desconocido'
  }
}

/** Cómo se nombra una instantánea en los avisos: su etiqueta o su identificador. */
function etiquetaDeSnapshot(snapshot: Snapshot): string {
  return snapshot.label ?? snapshot.id
}

/** El nombre del target de la fila; su identificador si la API no publicó nombre. */
function nombreDelTarget(snapshot: Snapshot): string {
  return snapshot.target_name ?? snapshot.target_id ?? 'sin nombre'
}

/**
 * Por qué esta fila no ofrece restaurar, o `null` si sí la ofrece.
 *
 * Los dos casos que la acción no debe lanzar: sin `target_type` la instantánea
 * no casa con ninguna ejecución, así que no hay destino sobre el que restaurar;
 * y con un tipo que no sea `directory` la operación no está soportada en esta
 * versión. La limitación se declara **en la fila** y no después del error:
 * ofrecer el botón para que la API conteste `409` o `422` llevaría al operador a
 * confirmar algo que no va a ocurrir.
 */
function limitacionDeRestauracion(snapshot: Snapshot): string | null {
  if (snapshot.target_type === null) {
    return 'No se pudo resolver el target de origen de esta instantánea: no hay destino sobre el que restaurar.'
  }
  if (snapshot.target_type !== 'directory') {
    return `En esta versión solo se restauran targets de directorio: «${nombreDelTarget(snapshot)}» es de tipo ${snapshot.target_type}.`
  }
  return null
}

/**
 * Lo que se restaura y dónde, para la confirmación: nombre y tipo del target.
 *
 * El tipo se pinta tal cual y sin valor por defecto: a esta confirmación solo se
 * llega cuando `limitacionDeRestauracion` no encontró pega, y eso significa que
 * el tipo se conoce y es `directory`.
 */
function descripcionDeRestauracion(snapshot: Snapshot): string {
  return `Se restaurará «${etiquetaDeSnapshot(snapshot)}» sobre el target «${nombreDelTarget(snapshot)}» (${snapshot.target_type}).`
}

/**
 * El texto con el que se anuncia una restauración cerrada.
 *
 * El `motivo` es el mensaje con el que la API explica el desenlace: cuando
 * llega, manda sobre el texto por defecto, porque el panel no reescribe lo que
 * dice la API. Los contadores del evento no entran aquí: van a `0` por diseño
 * —la biblioteca de rustic no ofrece avance incremental—, así que lo que se
 * anuncia es la **fase**, no una cifra.
 */
function resultadoDe(evento: EventoDeRestauracion, target: string): Aviso {
  if (evento.phase === 'fallida') {
    return {
      tipo: 'error',
      texto: evento.motivo ?? `La restauración sobre «${target}» ha fallado.`,
    }
  }
  return {
    tipo: 'success',
    texto: evento.motivo ?? `Restauración terminada sobre «${target}».`,
  }
}

/**
 * Lo que hay que advertir cuando el stream no está vivo. Un aviso de progreso
 * congelado y sin explicación parece una restauración parada.
 */
function avisoDeConexion(estado: EstadoDeLaConexion | null): string | null {
  if (estado === 'reconectando') {
    return 'Se ha perdido la conexión con el servidor: el aviso de fase puede llegar con retraso.'
  }
  if (estado === 'detenido') {
    return 'El aviso en vivo está detenido: el panel no está recibiendo el progreso de esta restauración.'
  }
  return null
}

/**
 * El titular del aviso de progreso: la fase en curso, o el estado explícito de
 * «sin confirmación» cuando el plazo se agotó.
 */
function mensajeDeProgreso(enCurso: EnCurso, phase: string | null): string {
  if (enCurso.sinConfirmacion) {
    return `Sin confirmación del servidor para «${enCurso.target}»`
  }
  return `Restaurando «${enCurso.target}» — ${textoDeFase(phase)}`
}

/**
 * La explicación del aviso de progreso. Con la confirmación perdida dice lo
 * único que el panel sabe de verdad: que no recibió el desenlace y que la
 * operación puede seguir en curso; cerrarlo es la salida, porque mantener el
 * aviso no va a traer ninguna certeza.
 */
function descripcionDeProgreso(
  enCurso: EnCurso,
  estado: EstadoDeLaConexion | null,
): string | undefined {
  if (enCurso.sinConfirmacion) {
    return `El panel no ha recibido el desenlace de esta restauración en ${PLAZO_DE_CONFIRMACION_MS / 60_000} minutos: puede seguir en curso, pero ya no hay forma de confirmarlo. Ciérralo cuando quieras soltarla.`
  }
  return avisoDeConexion(estado) ?? undefined
}

/**
 * Por qué esta fila no puede lanzar una restauración **ahora mismo**, aunque la
 * instantánea sí sea restaurable: el panel sigue una sola a la vez.
 */
function esperaDeRestauracion(enCurso: EnCurso): string {
  return `Hay una restauración en curso sobre «${enCurso.target}»: podrás lanzar otra cuando cierre.`
}

/**
 * Snapshots (§6): las instantáneas de un repositorio.
 *
 * El identificador se pinta **completo**: es el hexadecimal con el que rustic
 * nombra la instantánea y acortarlo lo haría inservible para buscarla.
 *
 * No hay botón de purga a propósito: el contrato no publica un endpoint de
 * borrado de instantáneas, y un botón que no puede hacer nada es peor que
 * ninguno.
 *
 * Restaurar sí está, con dos reglas que vienen de la spec: la confirmación
 * **nombra el target** sobre el que se va a restaurar —lo publica el listado en
 * cada fila—, y la acción se **deshabilita explicándose** cuando el target no se
 * conoce o no es de directorio. El resultado se comunica al **cerrar la fase**
 * por SSE, no al aceptar el `POST`: si se anunciara antes, el operador vería a
 * la vez «restaurado» y «restaurando».
 *
 * Y se restaura **de una en una**: mientras hay una en vuelo, las demás filas
 * deshabilitan su acción y explican por qué, con el mismo recurso que las filas
 * no restaurables. Es una elección entre las dos salidas posibles, y se toma la
 * estrecha a propósito. El proveedor del stream sí guarda un mapa por
 * `execution_id` —de modo que el desenlace de una segunda restauración llegaría
 * y estaría ahí—, pero el panel seguiría sin poder anunciarlo: llevarlo exigiría
 * un aviso y un plazo por restauración, entradas sin `execution_id` con una clave
 * inventada mientras el `POST` no contesta y un aviso de desenlace por cada una.
 * A cambio de esa máquina, aquí hay **un** canal de desenlace y la garantía de
 * que ninguno se pierde: la segunda no puede pisar a la primera porque no se
 * lanza. El precio es que el panel es más estrecho que la API —el motor solo
 * rechaza la segunda sobre el **mismo** destino (`409`), así que dos destinos
 * distintos sí cabrían— y se paga a gusto: `restore_progress` no publica avance
 * (los contadores van a `0`), de modo que dos restauraciones a la vez no darían
 * nada mejor que dos «en curso» y un desenlace que sí puede perderse.
 *
 * La operación se suelta por dos caminos y solo dos: llega la fase terminal, o
 * vence el `PLAZO_DE_CONFIRMACION_MS` y el operador cierra el aviso de «sin
 * confirmación». Sin el segundo, un desenlace perdido —el stream no guarda
 * replay— dejaría la pantalla bloqueada para siempre.
 */
export default function Snapshots() {
  const repositorios = useRepositorios()
  const [seleccionado, setSeleccionado] = useState<string | null>(null)
  const [aviso, setAviso] = useState<Aviso | null>(null)
  const [enCurso, setEnCurso] = useState<EnCurso | null>(null)
  /** El aviso de progreso se puede cerrar: una operación larga no debe tapar la tabla. */
  const [progresoOculto, setProgresoOculto] = useState(false)
  const restaurar = useRestaurarSnapshot()

  // Sin elección explícita, se usa el primero: una pantalla que obliga a elegir
  // cuando solo hay un repositorio es un paso de más.
  const repositoryId = seleccionado ?? repositorios.data?.[0]?.id ?? null
  const snapshots = useSnapshots(repositoryId)

  // El avance llega por SSE (`restore_progress`), no por una recarga del
  // listado. El hook se suscribe desde el montaje y no lo suelta entre
  // operaciones: el stream no guarda replay, así que la conexión tiene que
  // estar viva **antes** del `POST` o el primer aviso se perdería.
  const { evento, estado } = useRestauracion(enCurso?.executionId ?? null)
  const faseTerminal = evento !== null && esFaseTerminal(evento.phase)

  // El desenlace se anuncia al cerrar la fase, y la transición se resuelve en el
  // propio render en vez de en un efecto: el evento y el aviso son lo mismo
  // visto dos veces, así que no hay estado que sincronizar. La condición se
  // apaga a sí misma —en cuanto `enCurso` vuelve a `null` deja de cumplirse—,
  // de modo que el ajuste ocurre una única vez por desenlace. La fase terminal
  // apaga el aviso de progreso en el mismo render, así que los dos mensajes
  // nunca conviven.
  if (enCurso !== null && faseTerminal) {
    setAviso(resultadoDe(evento, enCurso.target))
    setEnCurso(null)
  }

  // El plazo cubre los dos desenlaces que pueden quedarse sin llegar: la fase
  // terminal que se pierde —el stream no guarda replay— y el `POST` que nunca
  // contesta, con el desenlace ya retenido en el mapa del proveedor pero sin
  // `executionId` con el que correlacionarlo. Empieza al lanzar y se reinicia
  // solo si el `POST` contesta, que es el único cambio de estado de la
  // operación: la fase que llega no vive aquí, vive en el stream.
  useEffect(() => {
    if (enCurso === null || enCurso.sinConfirmacion) return

    const plazo = setTimeout(() => {
      setEnCurso((previo) =>
        previo === null || previo.sinConfirmacion ? previo : { ...previo, sinConfirmacion: true },
      )
      // Aunque el operador lo hubiera cerrado, el aviso vuelve: «sin
      // confirmación» es un cambio de estado que no debe pasar desapercibido, y
      // hasta que se cierre la operación sigue bloqueando las demás filas.
      setProgresoOculto(false)
    }, PLAZO_DE_CONFIRMACION_MS)

    return () => clearTimeout(plazo)
  }, [enCurso])

  const alRestaurar = async (snapshot: Snapshot) => {
    // La guarda no es adorno: el botón deshabilitado es lo que se ve, pero una
    // confirmación abierta de antes del bloqueo llegaría hasta aquí.
    if (repositoryId === null || enCurso !== null) return

    setAviso(null)
    setProgresoOculto(false)
    setEnCurso({
      snapshotId: snapshot.id,
      executionId: null,
      target: nombreDelTarget(snapshot),
      sinConfirmacion: false,
    })

    try {
      const resultado = await restaurar.mutateAsync({
        repositoryId,
        snapshotId: snapshot.id,
      })
      // El `POST` solo acepta el trabajo: aquí no se anuncia nada. Su respuesta
      // sirve para saber en qué ejecución corre la restauración y con qué
      // nombre la API resolvió el target. Si el desenlace ya llegó por SSE, el
      // aviso se cerró y esto no lo resucita; y si el plazo ya venció, la
      // operación no vuelve a «en curso» por contestar tarde.
      setEnCurso((previo) =>
        previo === null
          ? null
          : { ...previo, executionId: resultado.execution_id, target: resultado.target_name },
      )
    } catch (error) {
      setEnCurso(null)
      setAviso({ tipo: 'error', texto: mensajeDeError(error) })
    }
  }

  /**
   * Cerrar el aviso de progreso. Mientras la restauración sigue **en curso**
   * —o sin confirmar, pero con la conexión viva— cerrar solo lo oculta: el
   * desenlace llegará igual y se anunciará. Con el plazo vencido, cerrar suelta
   * además la operación: el panel no puede decir nada más de ella y mantener las
   * filas bloqueadas esperando un desenlace que quizá no llegue no ayuda a nadie.
   */
  const cerrarProgreso = () => {
    if (enCurso?.sinConfirmacion === true) {
      setEnCurso(null)
      return
    }
    setProgresoOculto(true)
  }

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
      title: 'Tarea',
      key: 'tarea',
      // El nombre de la tarea que hizo la copia y, sin él, el identificador que
      // ya publica `label` (que es el de la tarea). La columna «Etiqueta» no se
      // toca: sigue siendo la etiqueta del snapshot.
      render: (_valor, snapshot) => snapshot.job_name ?? snapshot.label ?? '',
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
    {
      title: 'Acciones',
      key: 'acciones',
      render: (_valor, snapshot) => {
        const limitacion = limitacionDeRestauracion(snapshot)
        const nombre = `Restaurar ${etiquetaDeSnapshot(snapshot)}`

        // Sin destino conocido o con un target que esta versión no restaura, la
        // acción no se ofrece: se explica la limitación en la propia fila.
        if (limitacion !== null) {
          return (
            <Space direction="vertical" size={2}>
              <Button size="small" disabled aria-label={nombre}>
                Restaurar
              </Button>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                {limitacion}
              </Typography.Text>
            </Space>
          )
        }

        // Una restauración cada vez: la fila que está en vuelo se queda con el
        // botón cargando y las demás explican por qué no pueden lanzar otra.
        // Así el desenlace que llega por el stream solo puede ser de **esta**
        // operación: no hay un segundo aviso que pueda pisarlo.
        if (enCurso !== null) {
          return (
            <Space direction="vertical" size={2}>
              <Button
                size="small"
                disabled
                aria-label={nombre}
                loading={restaurar.isPending && enCurso.snapshotId === snapshot.id}
              >
                Restaurar
              </Button>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                {esperaDeRestauracion(enCurso)}
              </Typography.Text>
            </Space>
          )
        }

        return (
          <Popconfirm
            title="¿Restaurar la instantánea?"
            description={descripcionDeRestauracion(snapshot)}
            okText="Restaurar"
            cancelText="Cancelar"
            onConfirm={() => void alRestaurar(snapshot)}
          >
            <Button
              size="small"
              aria-label={nombre}
              loading={restaurar.isPending}
              disabled={restaurar.isPending}
            >
              Restaurar
            </Button>
          </Popconfirm>
        )
      },
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

          {aviso !== null && (
            <Alert
              data-testid="resultado-de-la-restauracion"
              type={aviso.tipo}
              showIcon
              closable
              onClose={() => setAviso(null)}
              style={{ marginBottom: 16 }}
              message={aviso.texto}
            />
          )}

          {enCurso !== null && !progresoOculto && !faseTerminal && (
            <Alert
              data-testid="progreso-de-la-restauracion"
              // Con el stream caído el aviso se vuelve advertencia: el operador
              // tiene que saber que lo que lee puede estar congelado. Y con el
              // plazo vencido también, porque ya no afirma que siga en curso.
              type={
                enCurso.sinConfirmacion || avisoDeConexion(estado) !== null ? 'warning' : 'info'
              }
              showIcon
              closable
              onClose={cerrarProgreso}
              style={{ marginBottom: 16 }}
              message={mensajeDeProgreso(enCurso, evento?.phase ?? null)}
              description={descripcionDeProgreso(enCurso, estado)}
            />
          )}

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
