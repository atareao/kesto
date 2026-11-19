import { useQueries, useQueryClient } from '@tanstack/react-query'
import { listarSnapshots } from '../api/snapshots'
import type { Repository } from '../services/api/types'
import { claves } from './claves'
import { REINTENTOS_DE_S3 } from './useSnapshots'

/**
 * Lo que se puede afirmar del espacio ocupado en S3.
 *
 * El resultado se modela **por repositorio** —no como un único error—: un
 * repositorio que no responde no borra lo que sí se sabe de los demás. Quien
 * consuma esto puede seguir pintando las cifras que **no** dependen del S3 y
 * limitarse a **nombrar** el fallo, en vez de colapsar el resumen entero.
 */
export interface EspacioEnS3 {
  /** Suma de los `bytes` de las instantáneas que **sí** se pudieron leer. */
  bytes: number
  /**
   * ¿La cifra es un **mínimo**? Es `true` cuando algún repositorio tiene más
   * instantáneas que las que se contaron —`truncated`, o `snapshots.length <
   * total`— **o** cuando algún repositorio no se pudo leer: en los dos casos la
   * suma es parcial y presentarla como total sería mentir. Es la **única**
   * expresión de «mínimo»: el consumidor no vuelve a combinar condiciones.
   */
  esMinimo: boolean
  /**
   * Nombres de los repositorios que no respondieron, en el orden del listado de
   * entrada. Vacío si todos respondieron. Es lo que permite **nombrar** el fallo
   * sin sustituir el resumen por un error global.
   */
  repositoriosCaidos: string[]
  /** ¿Queda todavía algún listado en vuelo? */
  cargando: boolean
  /**
   * Vuelve a pedir el espacio de **todos** los repositorios. Es la acción del
   * aviso de fallo: reintentar lo decide el operador, no un bucle del panel.
   */
  reintentar: () => void
}

/**
 * Suma el espacio de las instantáneas de **todos** los repositorios.
 *
 * Se pide una consulta por repositorio con `useQueries` —no un bucle de hooks—
 * y cada una se cachea con su propia clave, así que volver al Dashboard no
 * repite las peticiones que ya están frescas. Cada consulta acota su reintento
 * (`REINTENTOS_DE_S3`): un repositorio mudo tiene que acabar en error, no en un
 * «cargando» sin fin.
 *
 * Ojo con de dónde viene ese desenlace: el cliente HTTP **no** tiene deadline
 * propio (`services/api/client.ts` no impone un timeout y el `AbortSignal` que
 * recibe no lo trae), así que `retry: false` solo acota los **reintentos**, no
 * la **espera**. Que una petición que nunca resuelve deje de ser «cargando»
 * depende del **backend**: Kesto corta el listado a los 10 s (`TOPE_DEL_DESTINO`
 * del motor de rustic), y ese corte es lo que hace rechazar el `fetch` y llegar
 * aquí el error. Si algún día el backend dejara de acotar el destino, esta vista
 * se quedaría en `cargando` para siempre: la garantía no vive en este hook.
 */
export function useEspacioEnS3(repositorios: readonly Repository[]): EspacioEnS3 {
  const cliente = useQueryClient()
  const consultas = useQueries({
    queries: repositorios.map((repositorio) => ({
      // La clave registra el `limit` que pide el `queryFn`, no un objeto vacío:
      // si no, la caché diría que la misma entrada es un listado cualquiera y
      // dos `limit` distintos compartirían clave.
      queryKey: claves.snapshots(repositorio.id, { limit: 200 }),
      // Se pide el tope del contrato (200) de una vez: este resumen suma los
      // `bytes` de **todas** las instantáneas que puede leer, no una página.
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        listarSnapshots(repositorio.id, { limit: 200 }, signal),
      retry: REINTENTOS_DE_S3,
    })),
  })

  let bytes = 0
  let parcial = false
  let cargando = false
  const repositoriosCaidos: string[] = []

  consultas.forEach((consulta, indice) => {
    if (consulta.isPending) cargando = true

    if (consulta.error !== null && consulta.error !== undefined) {
      // El nombre sale del repositorio que pidió la consulta: `useQueries`
      // conserva el orden del listado, así que el índice es el suyo.
      const repositorio = repositorios[indice]
      if (repositorio !== undefined) repositoriosCaidos.push(repositorio.name)
    }

    if (consulta.data === undefined) return

    // Si la página recibida no cubre todo el repositorio, la suma es parcial:
    // hay más instantáneas de las que se han podido leer. No se piden más —eso
    // sería paginar— ni se pide un segundo listado: se declara el mínimo.
    //
    // `truncated` es la señal **explícita** del backend y se mira aparte: la
    // comparación por sí sola presupone que la guarda del listado sigue por
    // encima del `limit: 200` de aquí. Con una página completa y
    // `truncated: true` —`snapshots.length === total`— la comparación no ve
    // nada, y la bandera significa exactamente eso: «la suma es parcial, la
    // cifra es un mínimo».
    parcial =
      parcial ||
      consulta.data.truncated ||
      consulta.data.snapshots.length < consulta.data.total
    for (const instantanea of consulta.data.snapshots) bytes += instantanea.bytes
  })

  return {
    bytes,
    // Una sola expresión de «mínimo»: el listado no cubre el total **o** un
    // repositorio ilegible. En los dos casos la suma es parcial, nunca el total.
    esMinimo: parcial || repositoriosCaidos.length > 0,
    repositoriosCaidos,
    cargando,
    reintentar: () => {
      for (const repositorio of repositorios) {
        // Prefijo de array, no la clave completa: así alcanza cualquier variante
        // de filtros de ese repositorio —incluida la del `limit: 200` de aquí—.
        void cliente.invalidateQueries({ queryKey: ['snapshots', repositorio.id] })
      }
    },
  }
}
