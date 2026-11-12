import { formatearBytes, formatearFecha, formatearUuid } from './format'

describe('formatearFecha', () => {
  it('pasa un RFC3339 a hora local con la zona visible', () => {
    // Un desplazamiento distinto de cero: si se ignorase y se pintase el texto
    // de entrada, la hora no coincidiría con la local.
    const entrada = '2026-03-15T12:34:56+02:00'
    const fecha = new Date(entrada)

    const salida = formatearFecha(entrada)

    const horaLocal = new Intl.DateTimeFormat('es-ES', {
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    }).format(fecha)
    const zonaLocal = new Intl.DateTimeFormat('es-ES', { timeZoneName: 'short' })
      .formatToParts(fecha)
      .find((parte) => parte.type === 'timeZoneName')?.value

    expect(salida).toContain(horaLocal)
    expect(salida).toMatch(/GMT|UTC|[A-Z]{2,5}/)
    if (zonaLocal !== undefined) expect(salida).toContain(zonaLocal)

    // Si la máquina no estuviera en +02:00 —que es el caso de la máquina de
    // desarrollo, en CET—, la hora pintada **no** puede ser la que se escribió
    // en el origen: eso probaría que se formateó en local y no copiando texto.
    const desplazamientoLocal = -fecha.getTimezoneOffset()
    if (desplazamientoLocal !== 120) expect(salida).not.toContain('12:34')
  })

  it('formatea el mismo instante igual venga en UTC o con desplazamiento', () => {
    expect(formatearFecha('2026-03-15T10:34:56Z')).toBe(formatearFecha('2026-03-15T12:34:56+02:00'))
  })

  it('no inventa una fecha con un valor ilegible o ausente', () => {
    expect(formatearFecha('no-es-una-fecha')).toBe('—')
    expect(formatearFecha(null)).toBe('—')
    expect(formatearFecha(undefined)).toBe('—')
  })
})

describe('formatearUuid', () => {
  it('conserva el UUID con guiones tal cual', () => {
    const uuid = '0199f0aa-1111-7000-8000-0000000000ff'

    expect(formatearUuid(uuid)).toBe(uuid)
  })

  it('repone los guiones de un UUID sin ellos', () => {
    expect(formatearUuid('0199f0aa1111700080000000000000ff')).toBe(
      '0199f0aa-1111-7000-8000-0000000000ff',
    )
  })

  it('deja intacto un identificador que no es un UUID (el snapshot es hex completo)', () => {
    const hex = 'a'.repeat(64)

    expect(formatearUuid(hex)).toBe(hex)
  })
})

describe('formatearBytes', () => {
  it('usa las unidades binarias del SRS con un solo decimal cuando hace falta', () => {
    expect(formatearBytes(0)).toBe('0 B')
    expect(formatearBytes(512)).toBe('512 B')
    expect(formatearBytes(1024)).toBe('1 KiB')
    expect(formatearBytes(1536)).toBe('1.5 KiB')
    expect(formatearBytes(1024 * 1024)).toBe('1 MiB')
    expect(formatearBytes(1024 * 1024 * 1024)).toBe('1 GiB')
  })

  it('no pinta decimales de más ni cae en notación científica', () => {
    expect(formatearBytes(1024 * 1024 * 1024 * 2)).toBe('2 GiB')
    expect(formatearBytes(1024 * 1024 * 1024 * 1024 * 5)).toBe('5 TiB')
    expect(formatearBytes(10 * 1024 ** 5)).toBe('10240 TiB')
  })

  it('devuelve cero con un valor imposible', () => {
    expect(formatearBytes(-1)).toBe('0 B')
    expect(formatearBytes(Number.NaN)).toBe('0 B')
  })
})
