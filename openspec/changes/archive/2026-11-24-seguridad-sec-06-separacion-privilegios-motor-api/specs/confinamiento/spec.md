# confinamiento — delta

## Purpose

Las fronteras de alcance de las copias: rutas bajo la raíz, `db_path` canónico y etiqueta de fuente de copia. Este delta no cambia ninguna regla: traslada la aplicación de la etiqueta al proceso motor privilegiado y deja constancia de que el IPC no la rebaja.

## MODIFIED Requirements

### Requirement: El `exec` solo se crea en contenedores declarados fuentes de copia

El motor SHALL inspeccionar el contenedor de un target antes de crear ningún `exec` y SHALL exigir que lleve la etiqueta `io.kesto.backup-source` con el valor `true`. Sin la etiqueta, la ejecución SHALL quedar en `failed` con un motivo que nombre el contenedor, y NO SHALL crearse el `exec`.

La comprobación SHALL aplicarse en el **único punto** por el que pasan todos los `exec` de bollard —el cliente del daemon—, de modo que ningún camino del motor pueda saltársela. Tras la separación de privilegios, ese cliente vive en el proceso `kesto-motor`: la comprobación SHALL ejecutarse allí, antes de cualquier llamada a bollard, y SHALL NOT delegarse en la API ni sustituirse por la lista blanca del IPC. El error que vuelve a la API SHALL NOT filtrar credenciales ni rutas de socket.

Es **fail-closed**: sin etiqueta no se ejecuta nada, y no hay una lista que poblar ni un valor por defecto permisivo. La declaración viaja con el contenedor, así que quien lo crea decide si es fuente de copia y no hay dos sitios que mantener en sincronía. Y el propio contenedor de Kesto queda denegado por sí solo, porque nadie lo etiqueta: no hace falta cablear su nombre, que cambiaría con cualquier renombrado del despliegue.

#### Scenario: Un contenedor sin la etiqueta no recibe exec

- **GIVEN** un target de base cuyo `container_name` corresponde a un contenedor en marcha sin la etiqueta `io.kesto.backup-source=true`
- **WHEN** se ejecuta la copia
- **THEN** SHALL acabar en `failed`
- **AND** el motivo SHALL nombrar el contenedor
- **AND** NO SHALL crearse ningún `exec` en él

#### Scenario: Un contenedor con la etiqueta sí lo recibe

- **GIVEN** un target de base cuyo contenedor lleva la etiqueta `io.kesto.backup-source=true`
- **WHEN** se ejecuta la copia
- **THEN** SHALL completarse

#### Scenario: El propio contenedor de Kesto no es fuente de copia

- **GIVEN** un target `sqlite` cuyo `container_name` es el del propio Kesto
- **WHEN** se ejecuta la copia
- **THEN** SHALL acabar en `failed`
- **AND** NO SHALL crearse ningún `exec`

#### Scenario: La etiqueta se exige también al restaurar

- **GIVEN** una restauración hacia un target de base cuyo contenedor no lleva la etiqueta
- **WHEN** se ejecuta la restauración
- **THEN** SHALL acabar en `failed`
- **AND** NO SHALL crearse ningún `exec`

#### Scenario: La etiqueta se comprueba en el motor tras el IPC
- **GIVEN** una solicitud `CrearExec` o `IniciarExec` legítima enviada por la API, y un contenedor sin la etiqueta
- **WHEN** el motor la atiende
- **THEN** el motor SHALL inspeccionar el contenedor y rechazar la operación antes de crear el `exec`
- **AND** la ejecución SHALL acabar en `failed` nombrando el contenedor
- **AND** el error devuelto a la API SHALL NOT contener credenciales ni la ruta del socket
