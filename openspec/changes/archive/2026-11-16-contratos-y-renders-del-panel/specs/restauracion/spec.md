# restauracion — delta

## MODIFIED Requirements

### Requirement: La restauración de un target de base exige confirmación explícita de sobrescritura

*(El título es el que le puso el change que creó el requisito y se deja intacto; su alcance, hoy, es el de los dos tipos de target que la bandera gobierna.)*

Restaurar un target de base de datos SHALL exigir la confirmación explícita de sobrescritura, y el flujo SHALL rechazar la operación —antes de lanzar ningún trabajo— cuando esa confirmación no se ha dado. La confirmación SHALL ser una bandera booleana del contrato de la petición, no un supuesto: quien restaura una base autoriza perder lo que hoy tenga.

La misma bandera SHALL gobernar la **poda** de un target de directorio. Con la bandera en `true`, la restauración de un directorio SHALL, además de escribir la instantánea, **retirar del destino** lo que no está en ella; con la bandera en `false` —o ausente, porque el cuerpo la declara con su valor por defecto— el destino SHALL **solo recibir**, sin retirar nada. Para un directorio la bandera NO SHALL ser obligatoria: la petición sin ella SHALL atenderse sin rechazo por condición previa sin cumplir, que es una puerta reservada a las bases.

La bandera NO SHALL deducirse del tipo de target. El mismo `directory` se restaura con poda o sin ella según lo que se haya confirmado, y el contrato del cuerpo SHALL llevar la bandera para todos los tipos, no sólo para el de base. La confirmación que el panel ofrece —con el aviso de lo que se pierde, y el envío de la bandera sólo al aceptarla— se declara en `ui`.

El porqué: restaurar un directorio escribe ficheros; restaurar una base **pisa datos vivos** —no hay «modo consulta»—. Pero un directorio no es inocuo: la poda **retira** del destino lo que no está en la instantánea, y lo retirado no vuelve, porque el operador no tiene copia de lo que había. El efecto irreversible es el mismo en los dos casos, y por eso la bandera es la misma: separa un `POST` de prueba de una operación destructiva. Lo que cambia entre los dos no es el aviso, es la puerta: en la base la ausencia de la bandera **rechaza** —pisa datos vivos, así que hay que decir que sí de forma deliberada—, y en el directorio **degrada** a una restauración que sólo recibe, que sigue siendo una operación útil y no destructiva. Y no se deduce del tipo porque el tipo no dice si ese destino puede permitirse perder lo que no está en la copia.

#### Scenario: Sin confirmación explícita no se restaura

- **GIVEN** un snapshot de un target de base de datos
- **WHEN** se pide restaurarlo **sin** la confirmación de sobrescritura
- **THEN** NO SHALL lanzarse ninguna ejecución de restauración
- **AND** NO SHALL publicarse ningún `restore_progress`
- **AND** la operación SHALL rechazarse por condición previa sin cumplir

#### Scenario: Con confirmación explícita se restaura

- **GIVEN** un snapshot de un target de base de datos
- **WHEN** se pide restaurarlo **con** la confirmación de sobrescritura
- **THEN** SHALL lanzarse la ejecución de restauración
- **AND** SHALL publicarse `restore_progress`

#### Scenario: Un directorio con la bandera poda el destino

- **GIVEN** un snapshot de un target de directorio y un destino con ficheros que no están en la instantánea
- **WHEN** se pide restaurarlo con la bandera de sobrescritura en `true`
- **THEN** SHALL lanzarse la restauración
- **AND** el destino SHALL quedarse con la instantánea y **sin** lo que no estaba en ella

#### Scenario: Un directorio sin la bandera solo recibe, y no se rechaza

- **GIVEN** el mismo snapshot y el mismo destino
- **WHEN** se pide restaurarlo con la bandera en `false` o sin la bandera en el cuerpo
- **THEN** SHALL lanzarse la restauración
- **AND** el destino SHALL recibir la instantánea **sin** retirar lo que no está en ella
- **AND** NO SHALL rechazarse la petición por condición previa sin cumplir

#### Scenario: La bandera no se deduce del tipo de target

- **GIVEN** el mismo snapshot de un target de directorio
- **WHEN** se restauran dos veces, una con la bandera y otra sin ella
- **THEN** los dos desenlaces SHALL ser distintos —con poda y sólo recibiendo—
- **AND** la diferencia NO SHALL venir del tipo de target, que es el mismo en las dos
