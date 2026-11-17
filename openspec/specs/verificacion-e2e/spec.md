# verificacion-e2e Specification

## Purpose
Esta capability reúne la batería de pruebas de punta a punta que comprueba que la copia y la restauración funcionan de verdad, y no solo que los procesos terminan con éxito. Para cada tipo de origen soportado (MariaDB, PostgreSQL, SQLite y directorio) recorre el ciclo completo —extraer, subir a un almacén S3 real, restaurar y comparar el contenido con el sembrado—, e incluye la restauración hacia un contenedor, la sobrescritura de destinos que ya tienen contenido y la retención y la purga contra ese mismo S3 real. La batería se ejecuta en CI mediante un job dedicado que levanta el motor de contenedores y el S3, y también en local con una única receta reproducible que apunta al socket indicado en `DOCKER_HOST`. Su rasgo definitorio es que falla en voz alta: si falta el motor, el servidor S3 o una imagen, o si el contenido restaurado difiere en una sola fila o fichero, la ejecución se detiene y nombra el problema en lugar de omitir escenarios y dar un falso verde.

## Requirements

### Requirement: Batería de punta a punta por tipo de origen

La batería SHALL ejecutar, para cada tipo de origen soportado (mariadb, postgres, sqlite y directorio), el ciclo completo: extraer del contenedor de origen, subir a un repositorio S3 real, restaurar y verificar los datos. La verificación SHALL comparar el contenido restaurado con el sembrado antes de la copia, no limitarse al código de salida.

#### Scenario: Copia y restauración de MariaDB conserva las filas
- **GIVEN** un contenedor MariaDB con una base sembrada con filas conocidas
- **WHEN** se ejecuta la copia y después la restauración hacia un contenedor MariaDB limpio
- **THEN** la restauración termina con éxito y las filas restauradas coinciden exactamente con las sembradas

#### Scenario: Copia y restauración de PostgreSQL conserva las filas
- **GIVEN** un contenedor PostgreSQL con una base sembrada
- **WHEN** se ejecuta la copia y la restauración hacia un contenedor limpio
- **THEN** las tablas y filas restauradas coinciden con las sembradas

#### Scenario: Copia y restauración de SQLite conserva la base
- **GIVEN** un fichero SQLite sembrado con filas conocidas
- **WHEN** se ejecuta la copia y la restauración hacia un destino limpio
- **THEN** la base restaurada contiene exactamente las filas sembradas

#### Scenario: Copia y restauración de directorio conserva el árbol
- **GIVEN** un directorio con una jerarquía de ficheros conocida
- **WHEN** se ejecuta la copia y la restauración hacia un directorio limpio
- **THEN** la jerarquía y el contenido de cada fichero coinciden byte a byte con el original

#### Scenario: Una diferencia de datos rompe la prueba
- **GIVEN** un ciclo completo que termina con éxito a nivel de proceso
- **WHEN** el contenido restaurado difiere en una sola fila o fichero del sembrado
- **THEN** la prueba falla y nombra la diferencia encontrada

### Requirement: Almacén S3 real en la batería

La batería SHALL usar un servidor S3 real con credenciales conocidas, levantado por el propio arnés o por el job de CI, y SHALL limpiar sus datos entre ejecuciones para que dos ciclos consecutivos no compartan estado.

#### Scenario: El repositorio se crea contra S3 real
- **GIVEN** un servidor S3 real con sus credenciales disponibles
- **WHEN** la batería crea un repositorio y sube una copia
- **THEN** el snapshot existe en el servidor S3 y se lista con su tamaño

#### Scenario: El S3 está aislado entre ejecuciones
- **GIVEN** una ejecución previa que dejó snapshots en el S3 de la batería
- **WHEN** se lanza la batería de nuevo
- **THEN** los snapshots previos no aparecen en los listados de la nueva ejecución

#### Scenario: Una credencial incorrecta no se publica en el motivo
- **GIVEN** un repositorio con una clave de acceso S3 incorrecta
- **WHEN** se intenta subir una copia
- **THEN** la ejecución falla y el motivo no contiene la credencial

### Requirement: Ejecución en CI con motor de contenedores

El CI SHALL tener un job dedicado que levante el motor de contenedores y el S3 de la batería, ejecute la batería e2e y los tests `#[ignore]` existentes, y SHALL fallar si alguno falla. El job NO SHALL degradar el job `backend` actual.

#### Scenario: El job e2e ejecuta la batería
- **GIVEN** un push o una pull request a `development`
- **WHEN** el job `e2e` se ejecuta
- **THEN** la batería completa y los tests `#[ignore]` corren y el job queda verde solo si todos pasan

#### Scenario: Un fallo de la batería deja el job en rojo
- **GIVEN** una diferencia de datos restaurados o un error del motor
- **WHEN** el job `e2e` termina
- **THEN** el job falla y el registro identifica el tipo de origen y el escenario afectados

#### Scenario: El job backend no depende de contenedores
- **GIVEN** el CI ejecutándose sin motor de contenedores disponible
- **WHEN** se ejecuta el job `backend`
- **THEN** sigue ejecutando `cargo fmt`, `clippy` y `cargo test --locked` como hasta ahora y no requiere S3 ni contenedores

### Requirement: Ejecución local reproducible

La batería SHALL poder ejecutarse en local con una única receta documentada, apuntando a un socket de motor configurable mediante `DOCKER_HOST`, sin pasos manuales no documentados.

#### Scenario: Receta local única
- **GIVEN** un entorno local con podman activo y el socket indicado en `DOCKER_HOSt`
- **WHEN** se ejecuta la receta `just e2e`
- **THEN** la batería levanta su S3, ejecuta todos los tipos de origen y deja el resultado en la salida

#### Scenario: Sin socket de motor no se finge éxito
- **GIVEN** un entorno sin `DOCKER_HOST` válido
- **WHEN** se ejecuta la receta `just e2e`
- **THEN** la ejecución falla con un mensaje que nombra la variable y el socket esperado, y no se salta ningún escenario

### Requirement: La batería falla en voz alta

La batería NO SHALL omitir escenarios en silencio. Si falta el motor de contenedores, el servidor S3 o una imagen requerida, SHALL fallar indicando qué falta.

#### Scenario: Falta el motor de contenedores
- **GIVEN** un entorno donde el socket de `DOCKER_HOST` no responde
- **WHEN** arranca la batería
- **THEN** falla con un motivo explícito y ningún test se marca como omitido

#### Scenario: Falta el servidor S3
- **GIVEN** un entorno donde el S3 de la batería no llega a estar disponible
- **WHEN** arranca la batería
- **THEN** falla con un motivo explícito en lugar de degradar a un almacén local

### Requirement: Restauración hacia contenedor de punta a punta

La restauración hacia un contenedor SHALL estar cubierta por la batería de punta a punta, desde el snapshot en S3 hasta el `stdin` del proceso de restauración, sin fichero temporal intermedio.

#### Scenario: El snapshot viaja de S3 al stdin del contenedor
- **GIVEN** un snapshot en S3 con un volcado conocido
- **WHEN** se restaura hacia un contenedor
- **THEN** el volcado llega completo al `stdin` del proceso y el destino contiene los datos esperados

#### Scenario: Un contenedor que no drena su stdin no cuelga la restauración
- **GIVEN** un contenedor cuyo proceso de restauración no lee su `stdin`
- **WHEN** transcurre el plazo de inactividad
- **THEN** la restauración termina en error acotado y no queda bloqueada

### Requirement: Sobrescritura de un destino con contenido

La restauración sobre un destino que ya tiene contenido SHALL estar cubierta y SHALL tratar de forma distinta los dos tipos de destino. Para un target de **base de datos**, sin la confirmación explícita de sobrescritura la operación SHALL rechazarse y el destino vivo NO SHALL tocarse. Para un target de **directorio**, sin la confirmación la restauración SHALL ejecutarse igual y el contenido que sobraba en el destino SHALL permanecer —lo que la instantánea no trae se queda—, mientras que con la confirmación el destino SHALL quedar igual a la instantánea, porque la confirmación es lo que autoriza la poda.

La poda de `rustic_core` es de mejor esfuerzo: un fichero que no se deja retirar se registra y la restauración continúa. Además ocurre antes de escribir el contenido, de modo que un fallo a mitad deja el destino podado y a medias.

#### Scenario: Sobrescribir un directorio existente con la confirmación
- **GIVEN** un directorio de destino con ficheros previos
- **WHEN** se restaura con la confirmación de sobrescritura
- **THEN** el contenido previo se sustituye por el del snapshot y, como la confirmación autoriza la poda, el destino queda igual a la instantánea

#### Scenario: Sin confirmación, un directorio se restaura sin podar
- **GIVEN** un directorio de destino con ficheros previos
- **WHEN** se restaura sin la confirmación explícita
- **THEN** la restauración se ejecuta y el contenido del snapshot se escribe, pero los ficheros previos que la instantánea no trae permanecen en el destino

#### Scenario: Sin confirmación, un destino de base de datos se rechaza
- **GIVEN** un destino de base de datos con contenido vivo
- **WHEN** se intenta restaurar sin la confirmación explícita
- **THEN** la operación se rechaza y el contenido previo sigue intacto

### Requirement: Retención y purga contra S3 real

La retención SHALL estar cubierta contra S3 real: las reglas retiran los snapshots que no conservan y la purga los elimina del almacén.

#### Scenario: La retención deja solo lo que conserva
- **GIVEN** un repositorio S3 con más snapshots de los que la regla conserva
- **WHEN** se aplica la retención
- **THEN** solo quedan los snapshots que la regla conserva y el resto desaparece del almacén

#### Scenario: Una purga fallida no deja el repositorio a medias
- **GIVEN** un fallo del servidor S3 durante la purga
- **WHEN** se aplica la retención
- **THEN** la ejecución falla con el motivo del almacén y el estado del repositorio queda legible
