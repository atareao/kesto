# arnes-de-pruebas — delta

## Purpose

El arnés de pruebas de Kesto es el conjunto de ayudantes que crean, aíslan y recogen lo que una prueba necesita del sistema —un directorio temporal, una base SQLite migrada, una clave—. Su trabajo es que cada prueba corra sobre recursos propios y que, al terminar la batería, no quede nada suyo acumulándose fuera de su sitio: ni un directorio que enturbie la siguiente corrida, ni un recurso del sistema —un inodo— agotado por no haberse recogido.

## ADDED Requirements

### Requirement: La espera de arranque de una base no se conforma con el servidor temporal

El arnés SHALL dar por arrancado un motor solo cuando su sonda acierte **dos veces**, separadas por una espera, de modo que el servidor que contesta **sobreviva** entre ambas. Un acierto único NO SHALL bastar.

La garantía real es acotada y se declara como es: **el servidor temporal que se apaga antes de la espera entre sondeos no confirma el arranque**. El residuo —un temporal que sobreviviera **más** que esa espera tras el primer acierto— **no** queda cerrado; exigiría que el entrypoint tardara más que la espera entre crear la base y apagar el temporal, muy por encima de la ventana medida (31 ms en MariaDB y 148 ms en PostgreSQL; la espera es de 2 s). Cerrarlo del todo pediría una señal exclusiva del definitivo, descartada por frágil (atada al `bind-address`/`listen_addresses` de cada imagen).

El motivo es que las imágenes oficiales de MariaDB y de PostgreSQL arrancan un **servidor temporal** para inicializarse —que ya tiene la base creada, porque la crea él— y lo apagan antes de arrancar el definitivo. La sonda nombra la base, y eso la protege del temporal **antes** de que la base exista, pero no de su tramo final: quien se conforme con el primer acierto puede encontrarse después el socket ausente. Exigir que el acierto se repita tras una espera descarta al temporal que se apaga en ese hueco.

El sondeo y la espera SHALL ser **parámetros inyectables**, no constantes leídas dentro de la espera, de modo que una prueba pueda simular la secuencia temporal —acierto durante el temporal, fallo después, acierto estable al final— sin contenedores y sin esperar la espera de producción.

La espera SHALL conservar su plazo y, agotado, SHALL fallar **en voz alta** nombrando el contenedor, la orden y el plazo con el último motivo, en vez de dar el arranque por bueno. El plazo SHALL acotar la espera del **primer** acierto; la confirmación —la espera y el segundo sondeo— SHALL disponer de su propia holgura acotada por la espera entre sondeos, de modo que un servidor que aparece al final del plazo se confirme en vez de fallar.

La espera con supervivencia SHALL aplicarse a todo motor cuyo arranque pase por un servidor temporal (MariaDB y PostgreSQL), y NO SHALL alterar la espera de un arranque que no lo tenga (SQLite).

#### Scenario: El servidor temporal no da la base por arrancada
- **GIVEN** un sondeo que acierta mientras el servidor temporal está en pie y falla después, con el socket ausente
- **WHEN** el arnés espera el arranque del motor
- **THEN** NO SHALL darlo por arrancado con ese primer acierto
- **AND** SHALL seguir sondeando hasta que dos sondeos consecutivos acierten

#### Scenario: El servidor definitivo da la base por arrancada
- **GIVEN** un sondeo que acierta siempre, como el servidor definitivo
- **WHEN** el arnés espera el arranque del motor
- **THEN** SHALL sondear al menos dos veces, separadas por la espera entre sondeos
- **AND** SHALL dar el arranque por bueno

#### Scenario: PostgreSQL exige la misma supervivencia que MariaDB
- **GIVEN** el arranque de un PostgreSQL, cuyo servidor temporal crea la base y se apaga antes del definitivo
- **WHEN** el arnés espera el arranque
- **THEN** SHALL exigir la misma supervivencia entre dos sondeos que en MariaDB
- **AND** NO SHALL darlo por arrancado con el primer acierto

#### Scenario: La espera entre sondeos no depende del reloj real
- **GIVEN** una prueba con un sondeo simulado y una espera inyectada
- **WHEN** el arnés espera el arranque del motor
- **THEN** SHALL poder correr sin contenedores y sin esperar la espera de producción
- **AND** el sondeo y la espera SHALL ser parámetros, no constantes internas

#### Scenario: Agotado el plazo, se falla en voz alta
- **GIVEN** un sondeo que nunca sobrevive a la espera entre sondeos
- **WHEN** vence el plazo de arranque
- **THEN** SHALL fallar nombrando el contenedor, la orden y el plazo, con el último motivo
- **AND** NO SHALL dar el arranque por bueno
