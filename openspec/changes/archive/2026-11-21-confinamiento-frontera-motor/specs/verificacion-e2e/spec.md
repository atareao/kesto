# verificacion-e2e — delta

## Purpose

La batería de punta a punta que comprueba que la copia y la restauración funcionan de verdad. Este delta añade la prueba de que la frontera del operador al motor rechaza lo que debe rechazar, extremo a extremo y contra un motor de contenedores real.

## ADDED Requirements

### Requirement: La frontera del operador al motor se verifica de punta a punta

La batería SHALL probar que el confinamiento rechaza, contra un motor de contenedores real, y no solo en la unidad: un contenedor de origen **sin** la etiqueta `io.kesto.backup-source=true` no recibe `exec`, y un `host_path` fuera de la raíz no se copia. Los contenedores de origen que la batería sí copia SHALL llevar la etiqueta, y la raíz de copias SHALL apuntar a la raíz temporal de la batería, de modo que los escenarios que hoy copian directorios sigan corriendo.

#### Scenario: Un contenedor de origen sin etiqueta no se copia

- **GIVEN** un contenedor de origen en marcha al que no se le ha puesto la etiqueta de fuente de copia
- **WHEN** se lanza una copia contra un target que lo nombra
- **THEN** la ejecución SHALL acabar en `failed`
- **AND** el motivo SHALL nombrar el contenedor

#### Scenario: Los contenedores de origen de la batería llevan la etiqueta

- **GIVEN** los contenedores que la batería crea para copiar
- **WHEN** se ejecuta el ciclo completo de copia y restauración
- **THEN** SHALL completarse para cada tipo de origen
- **AND** cada contenedor de origen SHALL haber llevado la etiqueta de fuente de copia

El caso **literalmente** fuera de la raíz no llega a la batería: el borde HTTP lo rechaza con `400` al dar de alta el target. El caso que solo el motor puede cerrar —y el que aquí se prueba de punta a punta— es el de una ruta textualmente bajo la raíz que un enlace simbólico lleva fuera.

#### Scenario: Un directorio fuera de la raíz no se copia

- **GIVEN** un target `directory` cuyo `host_path` resuelve —una vez canonicalizado— fuera de la raíz de copias de la batería
- **WHEN** se lanza la copia
- **THEN** la ejecución SHALL acabar en `failed`
- **AND** NO SHALL crearse instantánea en el repositorio
