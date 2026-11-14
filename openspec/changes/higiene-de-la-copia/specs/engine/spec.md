## ADDED Requirements

### Requirement: Una copia que no se da por buena no deja su instantánea en el repositorio

Cuando el motor decide que una ejecución acaba en `failed` **después** de que el archivado haya escrito una instantánea, el motor SHALL retirar esa instantánea del repositorio, de modo que el repositorio NO SHALL quedarse con una instantánea que no corresponde a ninguna ejecución en `success`. La retirada SHALL ser de **mejor esfuerzo**: si retirar la instantánea falla, la ejecución SHALL acabar igualmente en `failed`, con el motivo **original** del volcado, y el fallo de la retirada NO SHALL sustituir ese motivo ni tumbar al worker.

El porqué: la biblioteca de archivado `rustic_core` **silencia** el error del lector y sigue, devolviendo `Ok` y guardando la instantánea **sin el volcado dentro**. El motor ya no hereda ese `Ok` —comprueba la entrega del volcado antes de dar la copia por buena—, pero sin retirar la instantánea queda en el repositorio un residuo de una copia que no llegó a serlo: aparece en el explorador de instantáneas, cuenta como espacio consumido y no se limpia nunca, porque la retención solo corre tras copias exitosas. La retirada es de mejor esfuerzo y no sustituye al motivo porque el fallo que importa —y el que el operador tiene que leer— es el de la copia; convertir «no pude limpiar» en un error que oculta «no pude copiar» sería peor que el residuo.

#### Scenario: Una copia fallida no deja instantánea en el repositorio
- **GIVEN** una ejecución cuyo volcado no se entrega entero después de que el archivado haya escrito una instantánea
- **WHEN** la ejecución acaba en `failed`
- **THEN** el repositorio NO SHALL contener esa instantánea
- **AND** el número de instantáneas del repositorio NO SHALL haber crecido con motivo de esa ejecución

#### Scenario: Un fallo del productor tampoco deja instantánea
- **GIVEN** una ejecución cuyo hilo de extracción falla tras haberse escrito la instantánea
- **WHEN** la ejecución acaba en `failed`
- **THEN** el repositorio NO SHALL contener esa instantánea

#### Scenario: El veredicto de la ejecución no lo cambia la limpieza
- **GIVEN** una copia fallida cuya instantánea no se consigue retirar
- **WHEN** se lee la fila de la ejecución
- **THEN** su estado SHALL ser `failed`
- **AND** `error_message` SHALL llevar el motivo del volcado, NO SHALL llevar el de la retirada
- **AND** el worker del repositorio SHALL seguir atendiendo la siguiente ejecución
