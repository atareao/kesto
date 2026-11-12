import { Empty, Typography } from 'antd'

/**
 * Placeholder honesto de una pantalla que llega en la Fase 4: un título real y
 * un estado vacío, no un `<div>` mudo. Cada pantalla de datos lo sustituirá por
 * su implementación.
 */
export interface PropsDeEnConstruccion {
  /** Título de la pantalla; es el `h1` de la página. */
  titulo: string
  /** Qué mostrará la pantalla cuando esté terminada. */
  descripcion: string
}

export function EnConstruccion({ titulo, descripcion }: PropsDeEnConstruccion) {
  return (
    <section aria-labelledby="titulo-de-la-pantalla">
      <Typography.Title id="titulo-de-la-pantalla" level={1} style={{ marginTop: 0 }}>
        {titulo}
      </Typography.Title>
      <Empty description={descripcion} />
    </section>
  )
}
