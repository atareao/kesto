# engine — delta

## Purpose

El motor de Kesto ejecuta las copias y restauraciones fuera del runtime de Tokio —un worker secuencial por repositorio— y es el único que escribe el ciclo de vida de una ejecución. Publica su progreso en un único stream SSE global. La fila de base de datos es la constancia del estado; el stream es el aviso.

## ADDED Requirements

### Requirement: El final de una ejecución se anuncia en el stream después de escribirse en su fila

El trabajador SHALL escribir el estado terminal —y las cifras medidas— en la fila de la ejecución **antes** de publicar el evento final en el stream global, y el evento final SHALL llevar el mismo `status`, `bytes_processed` y `bytes_uploaded` que la fila que acaba de escribirse.

Ese orden es deliberado y lo fija el §5: la fila queda constancia antes del aviso. Su consecuencia para quien observa es que la fila y el stream son **dos caminos con orden no garantizado entre sí**: ver la fila terminal no implica haber recibido ya el evento final. Por eso, quien mida el evento final —una prueba, un cliente— SHALL esperar en el **stream** hasta observar un estado terminal, y NO SHALL dar por hecho que el evento está entregado por el solo hecho de que la fila ya sea terminal.

#### Scenario: La fila terminal precede al aviso
- **GIVEN** una ejecución que termina
- **WHEN** el trabajador cierra la ejecución
- **THEN** SHALL escribir la fila terminal antes de publicar el evento final
- **AND** el evento final SHALL llevar el mismo `status` que la fila

#### Scenario: Las cifras del evento final coinciden con las de la fila
- **GIVEN** una ejecución que termina con cifras medidas
- **WHEN** se compara el evento final con la fila
- **THEN** `bytes_processed` y `bytes_uploaded` SHALL ser los mismos en ambos
- **AND** `percentage` SHALL ser el que corresponde al hecho medido —nulo sin total conocido, un número con él—

#### Scenario: Quien mide el evento final espera en el stream
- **GIVEN** una ejecución que termina
- **WHEN** un observador espera el evento final en el stream, no en la fila
- **THEN** SHALL recibir un evento con `status ∈ {Success, Failed}`
- **AND** NO SHALL quedarse con el último evento `running` por haber leído antes de que el aviso se publicara
