# db — delta

## ADDED Requirements

### Requirement: La fila de un repositorio guarda su estado de salud

La tabla `repositories` SHALL incorporar tres columnas anulables —el **estado** de salud, la **fecha** de la última comprobación y un **motivo** acotado— mediante una migración con prefijo numérico entero (la forma que la herramienta de migración exige). El estado SHALL estar restringido por un `CHECK` a un conjunto cerrado de valores, y las columnas SHALL ser anulables para que las filas ya existentes queden en «desconocido» sin reescribirse. La migración NO SHALL alterar ni borrar ninguna fila existente.

La capa de acceso SHALL exponer un método que anote en una fila el desenlace de una comprobación —estado, motivo y fecha—, y NO SHALL permitir que el alta ni la actualización del repositorio (reemplazo de los campos mutables) toquen esas columnas: la salud no es un campo que el operador edite, la escribe la comprobación.

#### Scenario: Las filas existentes quedan en desconocido
- **GIVEN** una base con filas de `repositories` anteriores a la migración
- **WHEN** se aplica la migración
- **THEN** cada fila SHALL conservar sus valores
- **AND** su estado de salud SHALL quedar a nulo, es decir «desconocido»

#### Scenario: La comprobación persiste el desenlace
- **GIVEN** una fila de repositorio
- **WHEN** se anota el desenlace de su comprobación
- **THEN** SHALL quedar guardados el estado, la fecha y el motivo
- **AND** SHALL leerse de vuelta con esos valores

#### Scenario: Editar el repositorio no reescribe su salud
- **GIVEN** una fila con un estado de salud anotado
- **WHEN** se actualiza el repositorio con sus campos mutables
- **THEN** el estado, la fecha y el motivo de salud SHALL conservarse
