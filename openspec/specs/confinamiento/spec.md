# confinamiento Specification

## Purpose
El alcance de lo que un target puede alcanzar. La API de targets comprueba la **forma** de los campos —que estén, que sean coherentes con su tipo—, pero un campo bien formado puede apuntar a lo que no debe: un directorio del propio Kesto, un fichero ajeno dentro de un contenedor, o un contenedor que no es fuente de copia. Esta capability fija el **alcance** de los tres campos que el operador escribe y que el motor usa sin más límite que su forma: `host_path` queda bajo una raíz declarada, `db_path` ha de ser la forma canónica dentro de un contenedor permitido, y el `exec` solo se crea en contenedores que se declaran fuentes de copia. Gobierna las dos direcciones —extracción y restauración—, porque las dos usan los mismos campos.

## Requirements

### Requirement: Un `host_path` se lee y se escribe dentro de la raíz de copias

La copia de un directorio SHALL leer su `host_path` dentro de la raíz de copias de `KESTO_BACKUP_ROOT` y NO SHALL leerlo fuera de ella. El motor SHALL **canonicalizar** la ruta —resolviendo los enlaces simbólicos de **todos** sus componentes, no solo el último— y SHALL rechazar la copia cuando el resultado no quede bajo la raíz, comparando **por componentes** y no por prefijo de cadena. El rechazo SHALL ocurrir antes de leer un solo byte.

La restauración a un directorio SHALL aplicar la misma regla a su destino, y SHALL hacerlo antes de escribir ni de podar: el `host_path` es destino de escritura y, con la confirmación de sobrescritura, también de poda, así que una ruta fuera de la raíz no solo leería lo que no debe, sino que podría sobrescribir el propio estado de Kesto.

La regla se aplica **también a las filas anteriores a ella**: el motor no SHALL fiarse de que el borde HTTP la haya comprobado, porque hay targets escritos antes de que la regla existiera y la API no es el único camino al motor. Un `host_path` fuera de la raíz SHALL dejar la ejecución en `failed` con un motivo que nombre el campo, y NO SHALL tocarse el sistema de ficheros.

El porqué: el `compose` del §8 monta los directorios del host que se respaldan bajo `/backup_sources`, en solo lectura, y esa es la raíz por defecto. Sin la comprobación, un `host_path` de `/` archiva a S3 todo lo que Kesto ve —incluida `/app/data/kesto.db`, con los tokens y los secretos cifrados— y un `host_path` de `/app/data` permite sobrescribirla al restaurar. Una ruta textualmente bien formada alcanza lo que no debe, y la única frontera que lo impide es esta.

#### Scenario: Un host_path fuera de la raíz no se lee

- **GIVEN** un target `directory` cuyo `host_path` está fuera de `KESTO_BACKUP_ROOT`
- **WHEN** se ejecuta la copia
- **THEN** SHALL acabar en `failed`
- **AND** el motivo SHALL nombrar el campo `host_path`
- **AND** NO SHALL leerse ningún byte del directorio

#### Scenario: Un enlace que escapa de la raíz se rechaza

- **GIVEN** un target `directory` cuyo `host_path` está textualmente bajo la raíz pero un directorio intermedio es un enlace simbólico que apunta fuera de ella
- **WHEN** se ejecuta la copia
- **THEN** SHALL acabar en `failed`
- **AND** NO SHALL leerse el directorio de destino del enlace

#### Scenario: Un host_path bajo la raíz se copia

- **GIVEN** un target `directory` cuyo `host_path` queda bajo `KESTO_BACKUP_ROOT` y existe
- **WHEN** se ejecuta la copia
- **THEN** SHALL completarse
- **AND** la instantánea SHALL contener el contenido de ese directorio

#### Scenario: La restauración tampoco escribe fuera de la raíz

- **GIVEN** una restauración hacia un target `directory` cuyo `host_path` está fuera de `KESTO_BACKUP_ROOT`
- **WHEN** se ejecuta la restauración
- **THEN** SHALL acabar en `failed`
- **AND** NO SHALL escribirse ni retirarse nada en ese destino

#### Scenario: Una raíz parecida no basta

- **GIVEN** un target `directory` cuyo `host_path` empieza por la raíz como **cadena** pero no está dentro de ella —por ejemplo `/backup_sources_x` con la raíz `/backup_sources`—
- **WHEN** se ejecuta la copia
- **THEN** SHALL acabar en `failed`
- **AND** el motivo SHALL nombrar el campo `host_path`

### Requirement: El `db_path` de un target `sqlite` es la forma canónica dentro del contenedor

Antes de ejecutar ninguna orden, el motor SHALL comprobar que el `db_path` declarado **coincide con su propia forma canónica** dentro del contenedor del target, obtenida con `readlink -f`, y SHALL rechazar la ejecución cuando difiera. La comparación SHALL ser exacta y la ejecución SHALL quedar en `failed` con un motivo que nombre el campo, sin ejecutar el volcado ni retirar ningún fichero.

La frontera de este campo es **el contenedor**, que ya está restringido a los que se declaran fuentes de copia: no se le inventa una raíz que Kesto no puede conocer en un sistema de ficheros ajeno. Dentro de un contenedor permitido, lo que queda por cerrar es que la ruta no escape por un enlace simbólico, y `readlink -f` cubre los tres vectores de una vez: un `..`, un enlace en el último componente y —el peligroso— un enlace en un directorio **padre**, que un `rm -f` seguiría hasta borrar un fichero ajeno.

En la restauración, el `rm -f` que retira el fichero vivo SHALL quedar acotado al `<db_path>` declarado y a sus hermanos `-wal` y `-shm`, con la ruta ya comprobada, y NO SHALL alcanzar ningún otro fichero.

Consecuencia aceptada: una base que viva bajo un directorio enlazado deja de copiarse hasta que se declare su ruta **canónica**. Es un coste explícito y pequeño frente a dejar abierto el vector del directorio padre.

El contenedor del target SHALL disponer de `readlink` —lo traen busybox y coreutils— además del binario `sqlite3`. La ausencia de cualquiera de los dos SHALL reportarse como el fallo de la ejecución, con el error del contenedor leído y no perdido.

#### Scenario: Un db_path con `..` se rechaza

- **GIVEN** un target `sqlite` cuyo `db_path` contiene `..`
- **WHEN** se ejecuta la copia
- **THEN** SHALL acabar en `failed`
- **AND** NO SHALL ejecutarse el volcado

#### Scenario: Un db_path que es un enlace simbólico se rechaza

- **GIVEN** un target `sqlite` cuyo `db_path` existe en el contenedor como enlace simbólico a otro fichero
- **WHEN** se ejecuta la copia
- **THEN** SHALL acabar en `failed`
- **AND** NO SHALL leerse el fichero enlazado

#### Scenario: Un enlace en un directorio padre se rechaza

- **GIVEN** un target `sqlite` cuyo `db_path` pasa por un directorio intermedio que es un enlace simbólico a otro directorio
- **WHEN** se ejecuta la restauración
- **THEN** SHALL acabar en `failed`
- **AND** NO SHALL retirarse ningún fichero del directorio enlazado

#### Scenario: La restauración retira solo el fichero declarado y sus hermanos

- **GIVEN** un target `sqlite` cuyo `db_path` es canónico y hay ficheros vecinos con otros nombres
- **WHEN** se ejecuta la restauración
- **THEN** el `<db_path>` y sus hermanos `-wal` y `-shm` SHALL retirarse
- **AND** los ficheros vecinos NO SHALL tocarse

#### Scenario: Falta readlink en el contenedor

- **GIVEN** un target `sqlite` cuyo contenedor no tiene el binario `readlink`
- **WHEN** se ejecuta la copia
- **THEN** SHALL acabar en `failed`
- **AND** el motivo SHALL reflejar el fallo del contenedor, sin silenciarse

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
