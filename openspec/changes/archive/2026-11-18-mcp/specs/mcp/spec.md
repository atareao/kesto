# mcp Specification

## Purpose

El servidor que deja a un agente de IA operar Kesto con criterio: consultar el estado de las copias, lanzar una cuando toca y comprobar que el repositorio responde. Vive en un crate aparte, `kesto-mcp`, sobre el SDK oficial `rmcp`, y es un **cliente** de la API: no abre la base, no habla con Podman ni con rustic, y no decide nada por su cuenta. Reenvía el token del llamante y deja que la API aplique los scopes. El conjunto de herramientas es cerrado y deliberadamente corto: leer sí, actuar con `run` también, **restaurar y borrar nunca**. Arranca por `stdio` para el uso local y por `Streamable HTTP` para el uso remoto, y cuando falta el token o el scope lo dice con un error que se entiende, no con un `401` crudo.

## ADDED Requirements

### Requirement: El crate es un cliente de la API y no toca el estado

`kesto-mcp` SHALL ser un crate del workspace, separado del binario de Kesto, con dependencia de `rmcp`. SHALL hablar con la API por HTTP y NO SHALL abrir la base de datos, ni el socket de Podman, ni un repositorio rustic. La configuración SHALL llegar por entorno —`KESTO_URL` y `KESTO_API_TOKEN`, con el token por cabecera en el transporte HTTP— y NO SHALL escribirse en el código ni en un fichero versionado.

El token NO SHALL aparecer en logs, en mensajes de error ni en la salida del servidor.

#### Scenario: El servidor arranca sin base de datos propia
- **GIVEN** un `KESTO_URL` apuntando a una API viva y un `KESTO_API_TOKEN` válido
- **WHEN** se arranca `kesto-mcp`
- **THEN** SHALL quedar escuchando con su transporte
- **AND** NO SHALL abrir ninguna conexión a SQLite ni al socket de Podman

#### Scenario: Sin `KESTO_URL` el arranque falla con un mensaje claro
- **GIVEN** un entorno sin `KESTO_URL`
- **WHEN** se arranca `kesto-mcp`
- **THEN** SHALL salir con error
- **AND** el mensaje SHALL nombrar la variable que falta

### Requirement: Las herramientas de lectura exponen el estado sin modificarlo

El servidor SHALL exponer siete herramientas de lectura, todas con scope `read`:

- `list_repositories` — el listado de repositorios con su salud.
- `list_targets` — el listado de targets.
- `list_jobs` — el listado de tareas programadas.
- `list_snapshots` — las instantáneas de un repositorio.
- `list_executions` — el histórico de ejecuciones con sus filtros.
- `get_execution` — una ejecución concreta por identificador.
- `get_health` — la salud del servicio y su modo de autenticación.

Cada una SHALL devolver un resultado estructurado con los datos de la API y SHALL propagar los errores de la API como error de herramienta, sin convertirlos en texto libre que el agente no pueda interpretar. Ninguna SHALL escribir.

#### Scenario: `list_repositories` devuelve los repositorios
- **GIVEN** un token con scope `read` y dos repositorios dados de alta
- **WHEN** el agente invoca `list_repositories`
- **THEN** SHALL devolver las dos filas con su nombre, su destino y su salud
- **AND** NO SHALL modificar nada

#### Scenario: `list_targets` devuelve los targets
- **GIVEN** un token con scope `read` y un target dado de alta
- **WHEN** el agente invoca `list_targets`
- **THEN** SHALL devolver el target con su tipo y su configuración pública
- **AND** NO SHALL incluir credenciales

#### Scenario: `list_jobs` devuelve las tareas
- **GIVEN** un token con scope `read` y una tarea programada
- **WHEN** el agente invoca `list_jobs`
- **THEN** SHALL devolver la tarea con su `cron_expression` y su estado
- **AND** NO SHALL modificarla

#### Scenario: `list_snapshots` devuelve las instantáneas de un repositorio
- **GIVEN** un token con scope `read` y un repositorio con instantáneas
- **WHEN** el agente invoca `list_snapshots` con el identificador del repositorio
- **THEN** SHALL devolver las instantáneas con su identificador y su fecha
- **AND** NO SHALL restaurar ninguna

#### Scenario: `list_executions` devuelve el histórico con filtros
- **GIVEN** un token con scope `read` y ejecuciones de varias tareas
- **WHEN** el agente invoca `list_executions` con un filtro por tarea y un límite
- **THEN** SHALL devolver solo las ejecuciones que cumplen el filtro, hasta el límite
- **AND** SHALL respetar el tope de la API

#### Scenario: `get_execution` devuelve una ejecución concreta
- **GIVEN** un token con scope `read` y una ejecución existente
- **WHEN** el agente invoca `get_execution` con su identificador
- **THEN** SHALL devolver esa ejecución con sus métricas y su estado
- **AND** con un identificador inexistente SHALL devolver el error de la API, no una lista vacía

#### Scenario: `get_health` devuelve la salud del servicio
- **GIVEN** un token con scope `read` o una API en modo `disabled`
- **WHEN** el agente invoca `get_health`
- **THEN** SHALL devolver el estado del servicio y su `auth_mode`

### Requirement: Las herramientas de acción exigen scope `run`

El servidor SHALL exponer dos herramientas de acción, ambas con scope `run`:

- `run_backup` — lanza una copia de una tarea, o de un repositorio y un target si se indican.
- `check_repository_health` — comprueba la salud de un repositorio y la persiste.

Ambas SHALL devolver el resultado de la API tal como llega. Un token `read` que las invoque SHALL recibir un error de herramienta que diga que falta el scope `run`, no un fallo genérico. Ninguna herramienta SHALL restaurar, borrar ni editar recursos.

#### Scenario: `run_backup` lanza una copia con un token run
- **GIVEN** un token con scope `run` y una tarea existente
- **WHEN** el agente invoca `run_backup` con el identificador de la tarea
- **THEN** SHALL devolver la ejecución creada con su estado inicial
- **AND** la API SHALL haber recibido `POST /api/v1/jobs/{id}/run`

#### Scenario: `run_backup` con un token read se rechaza con un mensaje claro
- **GIVEN** un token con scope `read`
- **WHEN** el agente invoca `run_backup`
- **THEN** SHALL devolver un error de herramienta que mencione el scope `run`
- **AND** NO SHALL crear ninguna ejecución

#### Scenario: `check_repository_health` comprueba y persiste
- **GIVEN** un token con scope `run` y un repositorio accesible
- **WHEN** el agente invoca `check_repository_health` con su identificador
- **THEN** SHALL devolver la salud resultante
- **AND** la API SHALL haber persistido el resultado

### Requirement: Los transportes `stdio` y `Streamable HTTP`

El binario SHALL arrancar por `stdio` cuando se invoca sin argumentos de transporte, y SHALL arrancar por `Streamable HTTP` cuando se le pide explícitamente, escuchando en la dirección configurada. En HTTP, el token SHALL poder llegar por la cabecera `Authorization: Bearer` de cada petición y SHALL tener prioridad sobre el `KESTO_API_TOKEN` del entorno. Ambos transportes SHALL exponer el mismo conjunto de herramientas y el mismo comportamiento.

#### Scenario: Arranque por stdio
- **GIVEN** un `KESTO_URL` y un `KESTO_API_TOKEN` válidos
- **WHEN** se arranca `kesto-mcp` sin argumentos de transporte
- **THEN** SHALL hablar el protocolo MCP por la entrada y la salida estándar
- **AND** SHALL anunciar las nueve herramientas

#### Scenario: Arranque por Streamable HTTP
- **GIVEN** un `KESTO_URL` y un `KESTO_API_TOKEN` válidos
- **WHEN** se arranca `kesto-mcp` con el transporte HTTP en una dirección y puerto
- **THEN** SHALL escuchar en esa dirección
- **AND** SHALL atender el protocolo MCP por HTTP

#### Scenario: El token de la cabecera manda sobre el del entorno
- **GIVEN** un `KESTO_API_TOKEN` de solo lectura en el entorno
- **WHEN** una petición HTTP trae un `Authorization: Bearer` con scope `run` e invoca `run_backup`
- **THEN** SHALL usar el token de la cabecera
- **AND** la copia SHALL lanzarse

### Requirement: Los errores dicen qué falta

Cuando falte el token, cuando el token sea inválido o esté revocado, o cuando el scope no alcance, la herramienta SHALL devolver un error de MCP con un mensaje que distinga los tres casos y diga qué hacer —configurar `KESTO_API_TOKEN`, emitir uno nuevo o pedir scope `run`—. El servidor NO SHALL caerse ni cerrar la sesión por un error de credencial: la siguiente llamada con un token válido SHALL funcionar.

#### Scenario: Sin token el error lo dice
- **GIVEN** un entorno sin `KESTO_API_TOKEN` y una petición HTTP sin `Authorization`
- **WHEN** el agente invoca `list_repositories`
- **THEN** SHALL devolver un error de herramienta que mencione `KESTO_API_TOKEN`
- **AND** el servidor SHALL seguir vivo

#### Scenario: Un token revocado se explica
- **GIVEN** un token revocado en `KESTO_API_TOKEN`
- **WHEN** el agente invoca `list_jobs`
- **THEN** SHALL devolver un error de herramienta que diga que el token no es válido o está revocado
- **AND** NO SHALL devolver una lista vacía

#### Scenario: Un scope insuficiente se explica
- **GIVEN** un token con scope `read`
- **WHEN** el agente invoca `check_repository_health`
- **THEN** SHALL devolver un error de herramienta que mencione el scope `run`
- **AND** el servidor SHALL seguir vivo

### Requirement: No hay herramientas destructivas

El servidor NO SHALL exponer ninguna herramienta que restaure una instantánea, borre un recurso, edite un repositorio o cancele una ejecución. El conjunto de herramientas SHALL ser exactamente el de las dos secciones anteriores; ampliarlo exige un change nuevo. La ausencia es una defensa: un agente no puede deshacer una copia ni vaciar un repositorio por mucho que se lo pidan.

#### Scenario: El catálogo no incluye herramientas destructivas
- **GIVEN** el servidor arrancado por cualquier transporte
- **WHEN** el agente pide el catálogo de herramientas
- **THEN** SHALL contener exactamente las nueve herramientas especificadas
- **AND** NO SHALL contener `restore_snapshot`, `delete_repository`, `delete_job` ni ninguna equivalente

#### Scenario: Una petición de restauración no tiene herramienta
- **GIVEN** un token con scope `run`
- **WHEN** el agente intenta invocar una herramienta de restauración por su nombre
- **THEN** SHALL recibir el error de herramienta desconocida del protocolo
- **AND** NO SHALL llegar ninguna petición de restauración a la API
