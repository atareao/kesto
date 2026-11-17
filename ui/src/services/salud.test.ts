import type { SaludDeRepositorio } from './api/types'
import { colorDeSalud, etiquetaDeSalud } from './salud'

/** Los seis estados del contrato, con el texto que la interfaz debe enseñar. */
const ETIQUETAS_ESPERADAS: Array<[SaludDeRepositorio, string]> = [
  ['ok', 'Correcto'],
  ['sin_repositorio', 'Sin repositorio'],
  ['passphrase', 'Passphrase incorrecta'],
  ['inaccesible', 'Inaccesible'],
  ['sin_permiso', 'Sin permiso de lectura'],
  ['desconocido', 'Desconocido'],
]

describe('etiquetaDeSalud', () => {
  it.each(ETIQUETAS_ESPERADAS)('nombra %s como «%s»', (salud, esperada) => {
    expect(etiquetaDeSalud(salud)).toBe(esperada)
  })

  it('un estado fuera del contrato tiene texto igualmente, en vez de quedarse mudo', () => {
    // El union es un tipo, no un dato: el JSON de un backend más nuevo puede
    // traer un estado que este panel no conozca. Se pinta «Desconocido».
    const fueraDelContrato = 'explotado' as SaludDeRepositorio

    expect(etiquetaDeSalud(fueraDelContrato)).toBe('Desconocido')
  })
})

describe('colorDeSalud', () => {
  it('el estado correcto va en verde y el desconocido en el gris neutro', () => {
    expect(colorDeSalud('ok')).toBe('success')
    expect(colorDeSalud('desconocido')).toBe('default')
  })

  it('ningún estado del contrato cae en verde salvo el correcto', () => {
    for (const [salud] of ETIQUETAS_ESPERADAS) {
      if (salud !== 'ok') expect(colorDeSalud(salud)).not.toBe('success')
    }
  })

  it('un estado fuera del contrato se pinta neutro, nunca como sano', () => {
    const fueraDelContrato = 'explotado' as SaludDeRepositorio

    expect(colorDeSalud(fueraDelContrato)).toBe('default')
    expect(colorDeSalud(fueraDelContrato)).not.toBe('success')
  })
})
