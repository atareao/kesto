import { render, screen } from '@testing-library/react'
import type { SaludDeRepositorio } from '../../services/api/types'
import { IndicadorDeSalud } from './IndicadorDeSalud'

/** Los seis estados del contrato con el texto que se lee en pantalla. */
const ETIQUETAS_ESPERADAS: Array<[SaludDeRepositorio, string]> = [
  ['ok', 'Correcto'],
  ['sin_repositorio', 'Sin repositorio'],
  ['passphrase', 'Passphrase incorrecta'],
  ['inaccesible', 'Inaccesible'],
  ['sin_permiso', 'Sin permiso de lectura'],
  ['desconocido', 'Desconocido'],
]

/**
 * El distintivo que envuelve el texto: es donde Antd pone el color del punto.
 * Se busca **por el texto visible** —lo que lee el operador— y no por un
 * `data-testid`, que no diría nada de lo que el usuario ve.
 */
function distintivoDe(texto: string): HTMLElement {
  const distintivo = screen.getByText(texto).closest('.ant-badge')
  if (distintivo === null) throw new Error(`no hay distintivo para «${texto}»`)
  return distintivo as HTMLElement
}

describe('IndicadorDeSalud', () => {
  it('nombra el estado con su texto y le pone el color como refuerzo', () => {
    render(<IndicadorDeSalud salud="ok" detalle={null} comprobadoEn={null} />)

    // El texto es el portador: se lee aunque el color no se distinga.
    expect(screen.getByText('Correcto')).toBeInTheDocument()
    expect(distintivoDe('Correcto').querySelector('.ant-badge-status-success')).not.toBeNull()
  })

  it.each(ETIQUETAS_ESPERADAS)('el estado %s se pinta con el texto «%s»', (salud, esperada) => {
    const { unmount } = render(<IndicadorDeSalud salud={salud} detalle={null} comprobadoEn={null} />)

    expect(screen.getByText(esperada)).toBeInTheDocument()
    unmount()
  })

  it('un estado fuera del contrato no deja el distintivo mudo ni en verde', () => {
    // El union es un tipo, no un dato: si el backend manda algo que este panel
    // no conoce, se pinta «Desconocido» en el gris neutro, nunca como sano.
    const fueraDelContrato = 'explotado' as SaludDeRepositorio

    render(<IndicadorDeSalud salud={fueraDelContrato} detalle={null} comprobadoEn={null} />)

    expect(screen.getByText('Desconocido')).toBeInTheDocument()
    expect(distintivoDe('Desconocido').querySelector('.ant-badge-status-default')).not.toBeNull()
    expect(distintivoDe('Desconocido').querySelector('.ant-badge-status-success')).toBeNull()
  })

  it('enseña el motivo saneado y la fecha de la última comprobación', () => {
    render(
      <IndicadorDeSalud
        salud="inaccesible"
        detalle="el destino no responde"
        comprobadoEn="2026-01-02T03:04:05Z"
      />,
    )

    expect(screen.getByText('el destino no responde')).toBeInTheDocument()
    expect(screen.getByText(/última comprobación/i)).toHaveTextContent('2026')
  })

  it('sin fecha de comprobación pinta el guion del panel, no una fecha inventada', () => {
    render(<IndicadorDeSalud salud="desconocido" detalle={null} comprobadoEn={null} />)

    expect(screen.getByText(/última comprobación/i)).toHaveTextContent('—')
  })

  it('un repositorio nunca comprobado se pinta como desconocido, en gris y nunca como sano', () => {
    render(<IndicadorDeSalud salud="desconocido" detalle={null} comprobadoEn={null} />)

    expect(screen.getByText('Desconocido')).toBeInTheDocument()
    expect(screen.queryByText('Correcto')).not.toBeInTheDocument()
    expect(distintivoDe('Desconocido').querySelector('.ant-badge-status-default')).not.toBeNull()
    expect(distintivoDe('Desconocido').querySelector('.ant-badge-status-success')).toBeNull()
  })
})
