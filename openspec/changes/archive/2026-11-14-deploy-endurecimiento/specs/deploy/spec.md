## Purpose

El empaquetado y el despliegue de Kesto: la imagen de contenedor que reúne el binario y el panel compilado sin toolchain y sin privilegios, el fichero de compose del §8 que la pone en marcha, la forma en que la configuración y los secretos entran en el contenedor, y la verificación de que el artefacto arranca y sirve de verdad. Fija el contrato entre lo que se construye, lo que se despliega y lo que el operador tiene que proveer.

## ADDED Requirements

### Requirement: Cabecera HSTS opcional

La aplicación SHALL poder emitir la cabecera `Strict-Transport-Security` cuando se configure la variable de entorno `KESTO_HSTS_MAX_AGE`. Ausente, la aplicación NO SHALL emitir la cabecera. Presente con un valor entero `>= 0`, toda respuesta SHALL llevar `Strict-Transport-Security: max-age=<n>`. Presente pero vacía o no numérica SHALL ser un error de arranque.

#### Scenario: sin configuración no hay cabecera
- **GIVEN** la aplicación arrancada sin `KESTO_HSTS_MAX_AGE`
- **WHEN** se pide cualquier recurso
- **THEN** la respuesta NO SHALL incluir `Strict-Transport-Security`

#### Scenario: configurada, toda respuesta la lleva
- **GIVEN** la aplicación arrancada con `KESTO_HSTS_MAX_AGE=31536000`
- **WHEN** se pide cualquier recurso, incluidas las rutas no declaradas y los métodos no permitidos
- **THEN** la respuesta SHALL incluir `Strict-Transport-Security: max-age=31536000`

#### Scenario: un valor inválido mata el arranque
- **GIVEN** `KESTO_HSTS_MAX_AGE` con un valor vacío o no numérico
- **WHEN** la aplicación resuelve la configuración
- **THEN** SHALL devolver un error de arranque

### Requirement: Respaldo del volumen de datos

SHALL existir un procedimiento reproducible para respaldar y recuperar el volumen nombrado de datos de la aplicación, y una receta `just backup-datos` que lo ejecute de forma consistente.

#### Scenario: la receta produce un archivo consistente
- **GIVEN** un despliegue en marcha con el volumen `kesto_data`
- **WHEN** se ejecuta `just backup-datos <destino>`
- **THEN** SHALL producirse un archivo comprimido del volumen
- **AND** el contenedor SHALL quedar en marcha al terminar

#### Scenario: la recuperación está documentada
- **GIVEN** un archivo de respaldo del volumen
- **WHEN** el operador consulta la documentación
- **THEN** SHALL encontrar el procedimiento para restaurarlo
