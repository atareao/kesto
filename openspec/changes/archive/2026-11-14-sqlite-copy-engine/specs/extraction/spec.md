## ADDED Requirements

### Requirement: La copia de SQLite es consistente en caliente y compatible con WAL

Un target de tipo `sqlite` SHALL copiarse obteniendo una imagen **consistente** de la base mediante la API de copia de SQLite, ejecutada **dentro del contenedor del target**, y NO SHALL copiarse el fichero `.db` con un `cp` directo ni leyendo el fichero en vivo. Concretamente, el comando SHALL ser `sqlite3 -readonly <db_path> .dump`, cuyo `stdout` SHALL entregarse en streaming al repositorio `rustic` por el mismo camino que los volcados de MariaDB y PostgreSQL. El `-readonly` es obligatorio: sin él `sqlite3` **crea** el fichero si la ruta no existe, `.dump` sobre una base vacía no emite nada y sale con código 0, de modo que una ruta mal escrita daría una ejecución en `success` con un volcado vacío. La copia SHALL ser válida con el **modo WAL** activo y con la base **en uso**, y NO SHALL exigir detener el servicio que la usa.

El porqué: un `cp` del `.db` —y de su `-wal`— mientras la base está abierta no es una copia de la base, es una copia de bytes en un instante sin garantía de consistencia; con WAL activo, el `.db` puede ir por detrás de su `.wal`, y el resultado puede restaurarse con corrupción o con datos perdidos sin que nada lo delate. El §4 prohíbe además escribir el volcado a disco en el lado de Kesto: la copia SHALL salir del contenedor y llegar a `rustic` en streaming, sin fichero temporal.

Un target `sqlite` SHALL exigir `container_name` —el contenedor donde vive el fichero— y `db_path` —la ruta absoluta del `.db` dentro de ese contenedor— y NO SHALL usar `db_user`, `db_password` ni `db_name`. El contenedor del target SHALL disponer del binario `sqlite3`; su ausencia SHALL reportarse como el fallo de la extracción, con el error del contenedor **leído y no perdido**.

#### Scenario: una base en uso se copia sin detener el servicio
- **GIVEN** un target `sqlite` apuntando a un contenedor en marcha cuya base está abierta y en uso
- **WHEN** se ejecuta la copia
- **THEN** SHALL completarse con éxito
- **AND** el servicio que usa la base SHALL seguir en marcha

#### Scenario: una base con WAL produce una copia consistente
- **GIVEN** un target `sqlite` cuya base tiene el modo WAL activo y escrituras confirmadas recientes
- **WHEN** se ejecuta la copia
- **THEN** la copia SHALL restaurarse sin errores
- **AND** SHALL contener las transacciones confirmadas antes de la copia

#### Scenario: la copia no es un cp del fichero
- **GIVEN** un target `sqlite`
- **WHEN** se inspecciona el comando que ejecuta la extracción
- **THEN** SHALL usar la API de copia de SQLite (`sqlite3 -readonly <db_path> .dump`)
- **AND** NO SHALL ser un `cp` del `.db`

#### Scenario: el `db_path` falta o está en blanco
- **GIVEN** un target `sqlite` sin `db_path` o con `db_path` vacío
- **WHEN** se construye la ruta de extracción
- **THEN** la copia SHALL rechazarse nombrando el campo que falta
- **AND** NO SHALL ejecutarse ningún `exec`

#### Scenario: falta el binario sqlite3 en el contenedor
- **GIVEN** un target `sqlite` cuyo contenedor no tiene `sqlite3`
- **WHEN** se ejecuta la copia
- **THEN** la copia SHALL fallar
- **AND** el error SHALL reflejar el fallo del contenedor, sin silenciarse

#### Scenario: ningún byte del volcado pasa por el disco de Kesto
- **GIVEN** un target `sqlite` y una copia en marcha
- **WHEN** termina la copia
- **THEN** el volcado SHALL haber viajado por el `stdout` del `exec` hasta `rustic`
- **AND** NO SHALL haberse escrito ningún fichero temporal en el sistema de ficheros de Kesto