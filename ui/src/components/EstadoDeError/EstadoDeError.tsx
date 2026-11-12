import { Alert, Button } from 'antd'

export interface PropsDeEstadoDeError {
  /** Qué no se pudo cargar, en una frase. */
  titulo: string
  /** El detalle ya legible (nunca JSON crudo). */
  mensaje: string
  onReintentar: () => void
}

/**
 * Error de carga con reintento. El botón no es decorativo: una petición que
 * falló por un corte de red se arregla volviendo a pedirla, no recargando la
 * pestaña.
 */
export function EstadoDeError({ titulo, mensaje, onReintentar }: PropsDeEstadoDeError) {
  return (
    <Alert
      type="error"
      showIcon
      message={titulo}
      description={mensaje}
      action={
        <Button size="small" onClick={onReintentar}>
          Reintentar
        </Button>
      }
    />
  )
}
