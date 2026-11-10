# api-resources Specification

## Purpose
La superficie de recursos del §5: las rutas con las que el panel da de alta, consulta, edita y borra las fuentes de respaldo, los repositorios S3 y las tareas programadas, más el listado del histórico de ejecuciones con sus filtros, su paginación acotada y sus métricas, el disparo manual de una tarea, el stream de eventos en vivo y el explorador de snapshots. Es el consumidor de la capa de acceso del §3, así que recibe el pool abierto y migrado al arrancar —lo lleva al estado de la aplicación y no abre una segunda base— y sirve cada fila con tipos propios de la API en vez de las entidades de la base, con los identificadores como texto en su forma canónica con guiones y las fechas como cadenas RFC3339. Valida lo que la base no puede validar sola —la coherencia de los campos de un target con su tipo, la existencia de las referencias de una tarea antes de escribir y la sintaxis del `cron_expression` de una tarea— y devuelve, por cada credencial, un booleano que dice si está configurada en lugar del valor. Y el alta de un repositorio no es un apunte en la base: conecta o inicializa el repositorio de verdad.

## ADDED Requirements

### Requirement: El alta y la edición de una tarea validan su expresión cron

`POST /api/v1/jobs` y `PUT /api/v1/jobs/:id` SHALL validar el `cron_expression` informado con la **misma** validación que usa el planificador —un analizador, una gramática, un solo sitio—, SHALL contestar `400` con la envoltura y el motivo cuando la expresión no parsea, y NO SHALL escribir la fila en ese caso: el alta no deja ninguna fila nueva y la edición deja la fila **como estaba**. Una expresión válida de **cinco** campos SHALL aceptarse, y también una de **seis**.

La validación vive en la capability `scheduler`, que es quien conoce la gramática y quien la va a evaluar; aquí se declara lo que la API promete, que es lo que el panel consume. Y el reparto no es una preferencia de arquitectura: si el borde HTTP tuviera su propia idea de qué expresión es válida, lo que la API aceptara y lo que el reloj entendiera podrían discrepar, y discreparían en los bordes —los segundos, los días de la semana—, que es donde aparecen las expresiones raras. Una sola función, invocada desde los dos sitios, hace imposible esa discrepancia.

Las dos rutas y no solo el alta, porque la edición es cómo se arregla una expresión mala y también cómo se podría empeorar sin límite. Y **antes de escribir**, porque una fila inválida que se escribe y luego se rechaza es peor que un rechazo: la fila existe, el panel la lista como programada y la reconciliación del planificador la saltará en cada pasada, llenando el registro de avisos por un error que ya se había detectado en la petición.

Esta validación **no sustituye** a la robustez del planificador: la validación protege de lo que entra por la API y el «se registra, se salta y las demás siguen» del planificador protege de lo que entra por debajo —una fila escrita con `sqlite3`, una base restaurada de una versión anterior—. Las dos capas existen porque protegen de dos caminos distintos, como ya ocurre con la validación de la retención en el borde y el guardarraíl del motor.

El motivo SHALL nombrar el **campo** —`cron_expression`—, como el resto de los `400` de la superficie. La regla de no repetir el valor existe por las credenciales, y aquí el campo es lo que hace falta: quien envió la expresión la tiene delante.

#### Scenario: El alta con una expresión de cinco campos se escribe
- **GIVEN** un alta de tarea con `cron_expression` a `0 3 * * *` y el resto de campos válidos
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `201` con la envoltura
- **AND** la fila SHALL quedar escrita con esa expresión

#### Scenario: El alta con una expresión inválida contesta 400 y no deja fila
- **GIVEN** un alta de tarea con una expresión que el analizador no entiende
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el motivo SHALL decir por qué la expresión no vale
- **AND** la tabla de tareas NO SHALL tener ninguna fila nueva

#### Scenario: La edición con una expresión inválida deja la fila como estaba
- **GIVEN** una tarea existente con su expresión válida
- **WHEN** se edita con una expresión que el analizador no entiende
- **THEN** SHALL contestar `400` con la envoltura
- **AND** la fila SHALL quedar con la expresión que tenía

#### Scenario: El motivo nombra el campo
- **GIVEN** un alta de tarea con una expresión inválida
- **WHEN** se lee el cuerpo del `400`
- **THEN** SHALL nombrar el campo `cron_expression`
