# api-resources Specification

## Purpose
La superficie de recursos del §5: las rutas con las que el panel da de alta, consulta, edita y borra las fuentes de respaldo, los repositorios S3 y las tareas programadas, más el listado del histórico de ejecuciones con sus filtros, su paginación acotada y sus métricas, el disparo manual de una tarea, el stream de eventos en vivo y el explorador de snapshots. Es el consumidor de la capa de acceso del §3, así que recibe el pool abierto y migrado al arrancar —lo lleva al estado de la aplicación y no abre una segunda base— y sirve cada fila con tipos propios de la API en vez de las entidades de la base, con los identificadores como texto en su forma canónica con guiones y las fechas como cadenas RFC3339. Valida lo que la base no puede validar sola —la coherencia de los campos de un target con su tipo y la existencia de las referencias de una tarea antes de escribir— y devuelve, por cada credencial, un booleano que dice si está configurada en lugar del valor. Con este change entran las tres rutas del §5 que faltaban —el disparo manual, el stream de eventos y el explorador de snapshots— y el alta de un repositorio deja de ser un apunte en la base para conectar con el repositorio de verdad.

## ADDED Requirements

### Requirement: El alta de un repositorio conecta con él, y lo inicializa si todavía no existe

`POST /api/v1/repositories` SHALL, además de escribir la fila, poner el repositorio en marcha: SHALL conectar con el destino descrito por el cuerpo y, si ese repositorio no existe todavía, SHALL inicializarlo. La respuesta SHALL decir cuál de los dos desenlaces ocurrió. Si el destino no se alcanza o la credencial no abre un repositorio que ya existe, el alta SHALL fallar y NO SHALL dejar fila en la base.

El §5 describe la ruta con cuatro palabras —«Inicializar o conectar repositorio S3/Rustic»— y hasta ahora solo se hacía la mitad de atrás: se guardaba una fila que describía un repositorio con el que nadie había hablado. El resultado es un alta que contesta `201` sobre un bucket que no existe, una `passphrase` con una errata o un endpoint mal escrito, y el error aparece mucho después, al ejecutar la primera copia, cuando ya nadie lo relaciona con el alta. Hablar con el destino en el momento del alta es lo que convierte esos tres errores en un `400` o un `502` con motivo, en la misma petición en la que se cometieron.

Que no se deje fila es la otra mitad y no es un detalle de estilo: una fila que describe un repositorio inalcanzable hace que el panel lo liste como disponible, que una tarea pueda apuntarle y que el fallo se manifieste como una ejecución fallida en vez de como un alta rechazada. Es más barato no escribir que limpiar.

#### Scenario: Un alta contra un destino que ya tiene repositorio conecta
- **GIVEN** un destino que ya contiene un repositorio abierto con esa passphrase
- **WHEN** se da de alta
- **THEN** SHALL contestar `201` con la envoltura
- **AND** la respuesta SHALL decir que se ha conectado
- **AND** SHALL quedar la fila en la base

#### Scenario: Un alta contra un destino vacío lo inicializa
- **GIVEN** un destino alcanzable sin ningún repositorio
- **WHEN** se da de alta
- **THEN** SHALL contestar `201` con la envoltura
- **AND** la respuesta SHALL decir que se ha inicializado
- **AND** el destino SHALL quedar con un repositorio

#### Scenario: Un alta contra un destino inalcanzable no deja fila
- **GIVEN** un body con un endpoint que no responde
- **WHEN** se da de alta
- **THEN** SHALL contestar con un error y la envoltura
- **AND** NO SHALL quedar fila en la base
- **AND** el error NO SHALL publicar la clave secreta

#### Scenario: Un alta cuya passphrase no abre el repositorio existente no deja fila
- **GIVEN** un destino que ya contiene un repositorio y un cuerpo con otra passphrase
- **WHEN** se da de alta
- **THEN** SHALL contestar con un error y la envoltura
- **AND** NO SHALL quedar fila en la base
- **AND** el destino NO SHALL quedar modificado

### Requirement: El disparo manual de una tarea encola y contesta 202, y 409 si ya está corriendo

`POST /api/v1/jobs/:id/run` SHALL contestar `202` con la ejecución creada en estado `pending`, SHALL contestar `404` si la tarea no existe, y SHALL contestar `409` si esa tarea ya tiene una ejecución en curso. La ruta SHALL colgar de la mitad protegida, como las demás de recursos.

El comportamiento vive en la capability `engine`, que es quien conoce la cola y el ciclo de vida; aquí se declara lo que la API promete, que es lo que el panel consume. Los tres códigos están elegidos en el requisito del motor y el porqué está escrito allí.

#### Scenario: El disparo contesta 202 con la ejecución
- **GIVEN** una tarea sin ejecución en curso
- **WHEN** se pide `POST /api/v1/jobs/:id/run`
- **THEN** SHALL contestar `202` con la envoltura
- **AND** `data` SHALL traer el `id` de la ejecución y su `job_id`

#### Scenario: Un identificador que no es un UUID contesta 400
- **GIVEN** una ruta cuyo identificador de tarea no es un UUID
- **WHEN** se pide el disparo
- **THEN** SHALL contestar `400` con el motivo

#### Scenario: Sin sesión no se dispara nada
- **GIVEN** el modo OIDC con la capa puesta y una petición sin sesión
- **WHEN** se pide el disparo
- **THEN** SHALL contestar `401` con la envoltura
- **AND** NO SHALL crearse ninguna ejecución

### Requirement: El stream de eventos es de la tarea que se pide, y el listado de snapshots es del repositorio que se pide

`GET /api/v1/events` SHALL contestar `200` con `text/event-stream` y NO SHALL exigir ningún parámetro. `GET /api/v1/snapshots` SHALL exigir el parámetro `repository_id`, SHALL contestar `400` si falta o no es un UUID, y SHALL contestar `404` si el repositorio no existe.

Las dos rutas leen de sitios distintos —el hub de eventos, que está en memoria, y el repositorio, que está en el almacenamiento—, pero las dos tienen la misma forma de fallar de la que ya se ocupó `api-resources`: un parámetro que falta o que no se puede interpretar es un `400` con el motivo, y un recurso que no existe es un `404`. Que el stream no lleve parámetros es deliberado y lo fija el §5: es un stream **global**, no uno por ejecución, así que no hay nada que filtrar aquí.

#### Scenario: El stream contesta su tipo de contenido
- **GIVEN** una sesión válida
- **WHEN** se pide `GET /api/v1/events`
- **THEN** SHALL contestar `200`
- **AND** el tipo de contenido SHALL ser `text/event-stream`

#### Scenario: El listado de snapshots exige su parámetro
- **GIVEN** una petición a `GET /api/v1/snapshots` sin `repository_id`
- **WHEN** se atiende
- **THEN** SHALL contestar `400` nombrando el parámetro

#### Scenario: Un repository_id que no es un UUID contesta 400
- **GIVEN** una petición con `repository_id=no-es-un-uuid`
- **WHEN** se atiende
- **THEN** SHALL contestar `400` con el motivo

#### Scenario: Un repositorio que no existe contesta 404
- **GIVEN** una petición con un `repository_id` bien formado que no corresponde a ninguna fila
- **WHEN** se atiende
- **THEN** SHALL contestar `404` con la envoltura

### Requirement: Las tres rutas nuevas cuelgan de la mitad protegida y ninguna escapa de la capa

`POST /api/v1/jobs/:id/run`, `GET /api/v1/events` y `GET /api/v1/snapshots` SHALL colgar de la mitad protegida del router, SHALL exigir sesión cuando el modo de autenticación la exige, y NO SHALL añadirse a la lista blanca de rutas públicas.

Las tres son superficie de negocio: el disparo escribe, y el stream y el explorador publican datos del despliegue —qué se está copiando y qué hay en el repositorio—. Dejarlas públicas por comodidad del panel sería publicar el estado del sistema a quien pregunte, y el stream lo publicaría además en tiempo real. La regla ya está declarada en el requisito de la partición; lo que hace falta aquí es que las tres rutas nuevas no se cuelen.

#### Scenario: Las tres rutas exigen sesión en modo OIDC
- **GIVEN** el modo OIDC con la capa puesta
- **WHEN** se piden las tres rutas sin sesión
- **THEN** las tres SHALL contestar `401` con la envoltura

#### Scenario: El stream no se sirve sin sesión
- **GIVEN** el modo OIDC con la capa puesta y una petición sin sesión a `GET /api/v1/events`
- **THEN** SHALL contestar `401` y NO SHALL quedar un stream abierto

#### Scenario: La lista blanca no ha crecido
- **GIVEN** la lista de rutas que no exigen sesión
- **WHEN** se revisa
- **THEN** NO SHALL contener ninguna de las tres rutas nuevas
