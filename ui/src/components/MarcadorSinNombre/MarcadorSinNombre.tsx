import { Typography } from 'antd'

/**
 * Red de seguridad para un nombre que el contrato ya no admite vacío. Pinta un
 * marcador explícito y atenuado en vez de un hueco en blanco: un hueco no
 * distingue «no tiene nombre» de «no se pintó», y el nombre del target **no** es
 * un sustituto válido —es justo la confusión que el nombre de la tarea resuelve—.
 */
export function MarcadorSinNombre() {
  return <Typography.Text type="secondary">sin nombre</Typography.Text>
}
