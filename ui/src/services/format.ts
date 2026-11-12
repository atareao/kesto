/**
 * Utilidades de presentación. Ninguna inventa datos: si el valor no sirve, se
 * pinta un guion en vez de una cifra falsa.
 */

/** Lo que se pinta cuando no hay valor. */
export const SIN_VALOR = '—'

// El `Intl` se construye una vez: crear un `DateTimeFormat` por celda de tabla
// es de lo más caro que se puede hacer en un listado largo.
const FORMATO_DE_FECHA = new Intl.DateTimeFormat('es-ES', {
  day: '2-digit',
  month: 'short',
  year: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
  // La zona **visible**: sin ella, una hora en un panel que se consulta desde
  // otra parte del mundo es una hora equivocada sin que se note. (`dateStyle` y
  // `timeStyle` no se pueden combinar con `timeZoneName`: hay que escribir los
  // campos uno a uno.)
  timeZoneName: 'short',
})

/**
 * Pasa un RFC3339 del backend a la hora local del navegador, con la zona
 * visible. Un valor vacío o ilegible se pinta como `—`.
 */
export function formatearFecha(rfc3339: string | null | undefined): string {
  if (rfc3339 === null || rfc3339 === undefined || rfc3339 === '') return SIN_VALOR

  const fecha = new Date(rfc3339)
  if (Number.isNaN(fecha.getTime())) return SIN_VALOR

  return FORMATO_DE_FECHA.format(fecha)
}

const LARGO_DE_UN_UUID = 32
const HEXADECIMAL = /^[0-9a-f]+$/i

/**
 * Devuelve el UUID en su forma canónica, con guiones.
 *
 * El backend los manda ya con guiones (`uuid::fmt::Hyphenated`), así que lo
 * normal es devolverlo tal cual; si algún día llegase sin ellos, se reponen en
 * vez de partirlo por la mitad. Un identificador que no es un UUID —el
 * `snapshot_id` es el hexadecimal completo de rustic— se deja intacto.
 */
export function formatearUuid(id: string): string {
  const sinGuiones = id.replaceAll('-', '')
  if (sinGuiones.length !== LARGO_DE_UN_UUID || !HEXADECIMAL.test(sinGuiones)) return id

  return [
    sinGuiones.slice(0, 8),
    sinGuiones.slice(8, 12),
    sinGuiones.slice(12, 16),
    sinGuiones.slice(16, 20),
    sinGuiones.slice(20),
  ].join('-')
}

/**
 * Duración legible entre dos instantes RFC3339. Sin `fin` la ejecución sigue
 * viva y se dice «en curso»; con valores ilegibles o un fin anterior al inicio,
 * se pinta `—` en vez de una cifra inventada.
 */
export function formatearDuracion(
  inicio: string | null | undefined,
  fin: string | null | undefined,
): string {
  if (fin === null || fin === undefined || fin === '') return 'en curso'
  if (inicio === null || inicio === undefined || inicio === '') return SIN_VALOR

  const desde = Date.parse(inicio)
  const hasta = Date.parse(fin)
  if (Number.isNaN(desde) || Number.isNaN(hasta) || hasta < desde) return SIN_VALOR

  const segundosTotales = Math.round((hasta - desde) / 1000)
  if (segundosTotales < 60) return `${segundosTotales} s`

  const minutosTotales = Math.floor(segundosTotales / 60)
  if (minutosTotales < 60) {
    const segundos = segundosTotales % 60
    return segundos === 0 ? `${minutosTotales} min` : `${minutosTotales} min ${segundos} s`
  }

  const horas = Math.floor(minutosTotales / 60)
  const minutos = minutosTotales % 60
  return minutos === 0 ? `${horas} h` : `${horas} h ${minutos} min`
}

// Binarias, como el `KiB` del sistema de ficheros: el panel habla de lo que
// ocupa una copia, no de lo que dice el fabricante de un disco.
const UNIDADES = ['B', 'KiB', 'MiB', 'GiB', 'TiB'] as const

/** Formatea un tamaño en bytes con unidades binarias. */
export function formatearBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return `0 ${UNIDADES[0]}`

  let valor = bytes
  let unidad = 0
  while (valor >= 1024 && unidad < UNIDADES.length - 1) {
    valor /= 1024
    unidad += 1
  }

  // Sin decimales para los bytes sueltos; con uno —y solo uno— para el resto.
  const cantidad = unidad === 0 ? String(Math.round(valor)) : valor.toFixed(1).replace(/\.0$/, '')
  return `${cantidad} ${UNIDADES[unidad]}`
}
