## MODIFIED Requirements

### Requirement: Los campos del target son coherentes con su tipo, y las referencias existen antes de escribir

`target_type = directory` SHALL exigir `host_path` y NO SHALL admitir `container_name`, `db_user`, `db_password`, `db_name` ni `db_path`. `target_type = mariadb` y `target_type = postgresql` SHALL exigir `container_name`, `db_user` y `db_password`, NO SHALL admitir `host_path` ni `db_path`, y SHALL admitir `db_name` como campo opcional, donde informado significa el volcado de **esa sola base** y ausente significa el volcado global —la semántica que fijó `db-schema` y que el motor implementará—. `target_type = sqlite` SHALL exigir `container_name` y `db_path`, y NO SHALL admitir `host_path`, `db_user`, `db_password` ni `db_name`.

El porqué: la tabla del §3 **acepta** las combinaciones incoherentes. Sus columnas son anulables, y su único `CHECK` restringe `target_type`, así que un `directory` con `container_name` y sin `host_path` es una fila que la base guarda tan contenta y que el motor no sabrá usar; el fallo aparecería en la primera copia, como un error que habla de un contenedor que no existe y no de un formulario mal rellenado. La coherencia se exige, por tanto, en el borde HTTP, que es el único sitio donde se conoce la intención de quien escribe.

El alta de una tarea SHALL comprobar que `target_id` y `repository_id` **existen** antes de escribir, y SHALL contestar `400` nombrando el campo que no resuelve, para que el cliente reciba un motivo preciso —«el target indicado no existe»— en vez del error de una restricción, que no distingue si lo que falló fue el target o el repositorio. Esa comprobación NO SHALL sustituir a la clave foránea del §3, que sigue siendo la última línea: entre la comprobación y el `INSERT` cabe otra petición que borre la fila referenciada, y ese hueco lo cubre la base, no el manejador.

Los campos **desconocidos** del cuerpo SHALL rechazarse con `400`. Sin esta regla, una errata en el nombre de un campo —`endpont`— se interpreta como un campo que nadie informa y el valor se pierde en silencio: la petición contesta `201` y el recurso queda escrito sin el dato que quien llamó creía haber mandado. Rechazarlo convierte ese silencio en un error que nombra el campo de más.

#### Scenario: Un directory con container_name se rechaza
- **GIVEN** un alta de target con `target_type` a `directory`, su `host_path` y un `container_name`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `container_name`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un sqlite con sus campos es válido
- **GIVEN** un alta de target con `target_type = sqlite`, `container_name` y `db_path`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `201`
- **AND** la fila escrita SHALL tener el `db_path` indicado

#### Scenario: Un sqlite sin db_path se rechaza nombrando el campo
- **GIVEN** un alta de target con `target_type = sqlite` y `container_name`, sin `db_path`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400`
- **AND** el mensaje SHALL nombrar el campo `db_path`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un sqlite con db_user se rechaza
- **GIVEN** un alta de target con `target_type = sqlite`, sus `container_name` y `db_path`, y un `db_user`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `db_user`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un mariadb sin db_password se rechaza nombrando el campo
- **GIVEN** un alta de target con `target_type` a `mariadb`, su `container_name` y su `db_user`, y sin `db_password`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400`
- **AND** el mensaje SHALL nombrar el campo `db_password`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un db_name ausente es NULL en la columna
- **GIVEN** un alta de target de base de datos que no informa `db_name`
- **WHEN** se lee la fila y se mira la columna `db_name` con SQL directo
- **THEN** el valor almacenado SHALL ser `NULL`
- **AND** NO SHALL ser la cadena vacía

#### Scenario: Una tarea con un target_id inexistente no escribe nada
- **GIVEN** un alta de tarea cuyo `target_id` no existe
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `target_id`
- **AND** la tabla de tareas NO SHALL tener ninguna fila nueva

#### Scenario: Un campo de más en el cuerpo se rechaza
- **GIVEN** un alta con un cuerpo válido al que se le añade un campo que no existe en el §3
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400`
- **AND** NO SHALL escribirse ninguna fila