/**
 * Los consumidores del stream SSE: el contexto, el nombre del evento y los
 * hooks. El proveedor que abre la conexión vive en `events.tsx`.
 *
 * Están separados a propósito: `eslint.config.js` activa
 * `react-refresh/only-export-components` —tal como lo dejó la Fase 1— y un
 * fichero que exporta un componente **y** hooks deja de ser refrescable. Aquí no
 * hay ningún componente, así que el aviso no aparece y no hay que tocar la
 * configuración.
 *
 * Contrato del evento (`src/api/events.rs`):
 *
 * - El nombre es literalmente `execution_progress`; renombrarlo en un lado es un
 *   cambio de protocolo, no una decisión de estilo.
 * - El `data` es un JSON con siete campos: `execution_id`, `job_id`, `status`,
 *   `bytes_processed`, `bytes_uploaded`, `percentage` y `current_action`.
 * - `execution_id` y `job_id` vienen con guiones, igual que en `/executions`, y
 *   se comparan tal cual: no se normalizan.
 * - `percentage` es `null` cuando no se conoce el total (barra indeterminada).
 *
 * El mismo stream publica un segundo tipo de evento, `restore_progress`, con
 * **otro** payload (siete campos distintos, en `EventoDeRestauracion`). Se
 * mantiene aparte y no se funde con `execution_progress`: son dos protocolos
 * con dos formas de medir el avance, y mezclarlos obligaría a cada consumidor a
 * distinguir de dónde viene lo que lee.
 */

import { createContext, useContext, useEffect } from 'react'
import type { EventoDeEjecucion, EventoDeRestauracion } from './api/types'

/** Lo que se espera antes del primer reintento; cada uno dobla al anterior. */
export const ESPERA_BASE_MS = 1000

/** Tope de la espera entre reintentos: no crece sin fin. */
export const ESPERA_MAXIMA_MS = 30_000

/** Única ruta del stream de eventos. */
export const RUTA_DE_EVENTOS = '/api/v1/events'

/** Nombre del evento de progreso de una copia. Literal de protocolo. */
export const NOMBRE_DEL_EVENTO = 'execution_progress'

/** Nombre del evento de progreso de una restauración. Literal de protocolo. */
export const NOMBRE_DEL_EVENTO_DE_RESTAURACION = 'restore_progress'

/** Estado de la conexión, para pintar un aviso si el stream no está vivo. */
export type EstadoDeLaConexion = 'conectando' | 'abierto' | 'reconectando' | 'detenido'

/**
 * Líneas que guarda la consola por ejecución. Es un tope, no una promesa: una
 * copia larga emite muchos eventos y la consola no puede crecer sin fin en
 * memoria. Se conservan **las últimas**.
 */
export const LIMITE_DE_LINEAS = 200

/**
 * Restauraciones que se recuerdan en el mapa. Igual que con la consola, es un
 * tope y no una promesa: el stream no guarda replay y una sesión larga puede
 * ver muchas restauraciones.
 *
 * El recorte no es el mismo que el de `historial` y no es un descuido: aquí
 * **reinserta** cada evento y expulsa la entrada que lleva más tiempo sin
 * moverse —LRU por recencia—, mientras que `historial` conserva **las últimas
 * N líneas** de cada ejecución y no reordena nada. Da igual cuál de las dos se
 * lea como «las últimas»: cada restauración publica su empezó y su terminó, así
 * que el aviso terminal reinserta su entrada y la mantiene viva hasta que
 * lleguen `LIMITE_DE_RESTAURACIONES` restauraciones más nuevas. Lo que importa
 * aquí es que la entrada del desenlace no se vaya por antigüedad antes de que la
 * pantalla lo correlacione.
 */
export const LIMITE_DE_RESTAURACIONES = 50

/** Lo que ve un consumidor: la conexión y el último progreso de cada ejecución. */
export interface ContextoDeEventos {
  estado: EstadoDeLaConexion
  /** Último evento conocido de cada ejecución, indexado por `execution_id`. */
  ejecuciones: ReadonlyMap<string, EventoDeEjecucion>
  /**
   * Eventos recibidos de cada ejecución, indexado por `execution_id`, con un
   * máximo de `LIMITE_DE_LINEAS` y en orden de llegada. Alimenta la consola.
   */
  historial: ReadonlyMap<string, readonly EventoDeEjecucion[]>
  /**
   * Último evento `restore_progress` conocido de cada restauración, indexado
   * por `execution_id`. Es un mapa aparte del de `ejecuciones`: el payload de
   * una restauración es otro, y un consumidor que espera un `EventoDeEjecucion`
   * no debe encontrarse uno de restauración.
   */
  restauraciones: ReadonlyMap<string, EventoDeRestauracion>
}

/** El contexto y las dos operaciones con las que el proveedor cuenta consumidores. */
export interface ValorDelStream extends ContextoDeEventos {
  suscribir: () => void
  desuscribir: () => void
}

export const ContextoDelStream = createContext<ValorDelStream | null>(null)

/** El contexto y la suscripción del consumidor que llama. */
function useContextoDelStream(): ValorDelStream {
  const contexto = useContext(ContextoDelStream)
  if (contexto === null) {
    throw new Error('useEventStream necesita un <ProveedorDeEventos> por encima en el árbol')
  }

  const { suscribir, desuscribir } = contexto
  useEffect(() => {
    suscribir()
    return desuscribir
  }, [suscribir, desuscribir])

  return contexto
}

/**
 * El stream: estado de la conexión y el último progreso conocido de cada
 * ejecución, indexado por `execution_id`.
 */
export function useEventStream(): ContextoDeEventos {
  const { estado, ejecuciones, historial, restauraciones } = useContextoDelStream()
  return { estado, ejecuciones, historial, restauraciones }
}

/**
 * Las líneas de la consola de **una** ejecución, en orden de llegada y con el
 * tope de `LIMITE_DE_LINEAS`. Los eventos de otras ejecuciones no aparecen.
 */
export function useLineasDeEjecucion(executionId: string): {
  lineas: readonly EventoDeEjecucion[]
  estado: EstadoDeLaConexion
} {
  const { estado, historial } = useContextoDelStream()
  return { estado, lineas: historial.get(executionId) ?? [] }
}

/**
 * El progreso de **una** ejecución. Los eventos de otras ejecuciones no tocan lo
 * que devuelve este hook: se leen del mapa por su identificador.
 */
export function useEjecucion(executionId: string): {
  evento: EventoDeEjecucion | null
  estado: EstadoDeLaConexion
} {
  const { estado, ejecuciones } = useContextoDelStream()
  return { estado, evento: ejecuciones.get(executionId) ?? null }
}

/**
 * El progreso de **una** restauración. A diferencia de `useEjecucion`, tolera
 * que no haya proveedor por encima: una pantalla puede montarse aislada —las
 * pruebas de la pantalla lo hacen— y no por eso debe romperse. Sin proveedor
 * devuelve `null` y no se suscribe.
 *
 * Con proveedor se suscribe **mientras el hook vive**, aunque todavía no haya
 * `executionId`: el identificador de la ejecución lo reparte el `POST` y el
 * primer `restaurando` se publica nada más lanzar el trabajo, así que si el
 * consumidor esperase a la respuesta para suscribirse, ese primer aviso se
 * perdería —el stream no guarda replay—. Suscribirse una vez y no soltarlo
 * evita además el ciclo cerrar/abrir por operación: el `EventSource` se abre
 * con el primer consumidor y se cierra con el último.
 *
 * El evento se localiza por `executionId`: dentro de la misma pantalla es
 * inequívoco, porque cada restauración reparte uno nuevo. El mapa del proveedor
 * retiene los eventos aunque todavía no haya `executionId` con el que buscarlos,
 * de modo que el desenlace no se pierde si llega antes que el `POST`.
 */
export function useRestauracion(executionId: string | null): {
  evento: EventoDeRestauracion | null
  estado: EstadoDeLaConexion | null
} {
  const contexto = useContext(ContextoDelStream)
  const suscribir = contexto?.suscribir
  const desuscribir = contexto?.desuscribir

  useEffect(() => {
    if (suscribir === undefined || desuscribir === undefined) return
    suscribir()
    return desuscribir
  }, [suscribir, desuscribir])

  if (contexto === null) return { evento: null, estado: null }

  const evento = executionId === null ? null : contexto.restauraciones.get(executionId) ?? null
  return { evento, estado: contexto.estado }
}
