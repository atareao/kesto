# verificacion-e2e — delta

## Purpose

La batería de punta a punta que comprueba que la copia y la restauración funcionan de verdad. Este delta declara el permiso de endpoints inseguros en el arnés, para que la validación del `endpoint` no deje sin almacén real a la batería.

## ADDED Requirements

### Requirement: La batería declara el permiso de endpoints inseguros

El arnés de la batería SHALL declarar `KESTO_ALLOW_INSECURE_ENDPOINT=1` en el entorno del binario que arranca, porque su almacén real —SeaweedFS— escucha en `http://127.0.0.1:<puerto>`: sin el permiso, la validación del `endpoint` rechazaría todos los altas de repositorio de la batería y los escenarios que dependen de un almacén real dejarían de poder correr.

La batería SHALL seguir verificando el almacén real —crear el repositorio contra él, aislarlo entre ejecuciones y copiar y restaurar a través suyo—, de modo que el permiso sea lo único que cambia y NO SHALL perderse cobertura.

#### Scenario: El almacén real se sigue verificando con el permiso declarado
- **GIVEN** la batería arrancada con `KESTO_ALLOW_INSECURE_ENDPOINT=1`
- **WHEN** se ejecuta el escenario que crea un repositorio contra el almacén real
- **THEN** SHALL contestar `201`
- **AND** el escenario NO SHALL fallar por la validación del `endpoint`

#### Scenario: El aislamiento entre ejecuciones no depende del permiso
- **GIVEN** dos ejecuciones seguidas de la batería sobre el mismo almacén
- **WHEN** la segunda crea su repositorio
- **THEN** NO SHALL encontrar los datos de la primera
- **AND** el permiso declarado NO SHALL cambiar el aislamiento
