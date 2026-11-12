import { Button, Input, InputNumber, Space, Typography } from 'antd'

/**
 * Generador visual de expresiones cron de **seis** campos —segundo, minuto,
 * hora, día del mes, mes y día de la semana— con presets y edición directa.
 *
 * El componente es controlado (`value`/`onChange`) para que lo use un
 * `Form.Item` de Ant Design sin estado duplicado. Los tres primeros campos se
 * editan con `InputNumber`; los tres últimos se conservan tal cual y se pueden
 * cambiar escribiendo la expresión completa.
 */

export interface PropsDeGeneradorCron {
  /** Expresión actual, p. ej. `0 0 3 * * *`. */
  value?: string
  /** Se llama con la expresión completa cada vez que cambia. */
  onChange?: (expresion: string) => void
  /** `id` del campo de texto, para que el `label` del formulario lo apunte. */
  id?: string
}

/** Expresión con la que arranca un alta: todos los días a las 03:00. */
export const CRON_POR_DEFECTO = '0 0 3 * * *'

interface Preset {
  etiqueta: string
  expresion: string
}

const PRESETS: readonly Preset[] = [
  { etiqueta: 'Diaria a las 03:00', expresion: '0 0 3 * * *' },
  { etiqueta: 'Cada hora', expresion: '0 0 * * * *' },
  { etiqueta: 'Cada 15 minutos', expresion: '0 */15 * * * *' },
  { etiqueta: 'Lunes a las 02:30', expresion: '0 30 2 * * 1' },
]

function partes(expresion: string): string[] {
  return expresion.trim().split(/\s+/).filter((parte) => parte !== '')
}

function numeroDe(parte: string | undefined): number | null {
  if (parte === undefined || !/^\d+$/.test(parte)) return null
  return Number(parte)
}

/** Recompone la expresión con los tres primeros campos y conserva el resto. */
function componer(segundo: string, minuto: string, hora: string, resto: readonly string[]): string {
  return [segundo, minuto, hora, ...resto].join(' ')
}

export function GeneradorCron({ value = '', onChange, id }: PropsDeGeneradorCron) {
  const campos = partes(value)
  const segundo = numeroDe(campos[0])
  const minuto = numeroDe(campos[1])
  const hora = numeroDe(campos[2])
  const resto = campos.length >= 6 ? campos.slice(3, 6) : ['*', '*', '*']

  const cambiarCampo = (indice: 0 | 1 | 2, nuevo: number | null) => {
    const base = [campos[0] ?? '0', campos[1] ?? '0', campos[2] ?? '0']
    base[indice] = nuevo === null ? '*' : String(nuevo)
    onChange?.(componer(base[0], base[1], base[2], resto))
  }

  return (
    <Space direction="vertical" size="small" style={{ width: '100%' }}>
      <Space wrap>
        <label>
          <Typography.Text type="secondary">Segundo</Typography.Text>
          <InputNumber
            aria-label="Segundo"
            min={0}
            max={59}
            value={segundo}
            onChange={(nuevo) => cambiarCampo(0, nuevo)}
            style={{ width: 80 }}
          />
        </label>
        <label>
          <Typography.Text type="secondary">Minuto</Typography.Text>
          <InputNumber
            aria-label="Minuto"
            min={0}
            max={59}
            value={minuto}
            onChange={(nuevo) => cambiarCampo(1, nuevo)}
            style={{ width: 80 }}
          />
        </label>
        <label>
          <Typography.Text type="secondary">Hora</Typography.Text>
          <InputNumber
            aria-label="Hora"
            min={0}
            max={23}
            value={hora}
            onChange={(nuevo) => cambiarCampo(2, nuevo)}
            style={{ width: 80 }}
          />
        </label>
      </Space>

      <Space wrap>
        {PRESETS.map((preset) => (
          <Button key={preset.expresion} size="small" onClick={() => onChange?.(preset.expresion)}>
            {preset.etiqueta}
          </Button>
        ))}
      </Space>

      <Input
        id={id}
        aria-label="Expresión cron"
        value={value}
        placeholder={CRON_POR_DEFECTO}
        onChange={(evento) => onChange?.(evento.target.value)}
      />
      <Typography.Text type="secondary">
        Seis campos: segundo, minuto, hora, día del mes, mes y día de la semana.
      </Typography.Text>
    </Space>
  )
}
