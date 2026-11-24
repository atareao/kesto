# engine — delta

## Purpose

El contrato del motor de copias y restauraciones. Este delta añade la frontera de proceso: la API solo conoce un cliente IPC y el proceso motor es el único que ejecuta las operaciones de bollard. Los requisitos existentes —hilos propios, cola por repositorio, estados, SSE, arranque y parada— siguen vigentes sin cambio.

## ADDED Requirements

### Requirement: La superficie de la API no enlaza el cliente Docker

El binario de la API SHALL compilarse sin `bollard`: la dependencia SHALL ser opcional y activarse solo con la feature `motor` del binario `kesto-motor`. Ningún handler ni estado de la API SHALL poseer `DOCKER_HOST` ni el socket del daemon; SHALL usar en su lugar un `MotorClient` que envía solicitudes tipadas por `KESTO_MOTOR_SOCKET`.

Un fallo, `panic`, `unwrap` explotable o inyección en `argv` en la superficie de la API SHALL NOT entregar el socket del daemon: la petición SHALL NOT llegar a bollard y el proceso motor SHALL seguir vivo. La propiedad SHALL comprobarse en el grafo de dependencias del binario API, no solo por inspección del código fuente.

#### Scenario: Un fallo en un handler no alcanza el daemon
- **GIVEN** un `panic` o un `unwrap` explotable en un handler de la API
- **WHEN** se intenta continuar hasta el daemon desde ese proceso
- **THEN** el binario API SHALL NOT poseer el socket ni `DOCKER_HOST`
- **AND** la petición SHALL NOT llegar al daemon
- **AND** el proceso motor SHALL seguir vivo

#### Scenario: El binario API no contiene bollard en su grafo
- **GIVEN** el árbol de dependencias del workspace con `bollard` marcado como opcional
- **WHEN** se inspecciona el grafo del binario de la API sin la feature `motor`
- **THEN** `bollard` SHALL NOT aparecer en el grafo de ese binario
- **AND** el binario `kesto-motor` SHALL compilarse con la feature `motor`

### Requirement: El motor ejecuta la lista blanca de operaciones del daemon

Todas las operaciones de bollard SHALL ejecutarse en el proceso `kesto-motor`, y solo como respuesta a una de las seis solicitudes permitidas por `motor-ipc`: `Comprobar`, `CrearContenedor`, `IniciarContenedor`, `CrearExec`, `InspeccionarExec` e `IniciarExec`. La API SHALL NOT enviar HTTP crudo ni pedir una operación fuera de esa lista.

Las solicitudes legítimas SHALL conservar el comportamiento actual: la etiqueta `io.kesto.backup-source=true` y las rutas confinadas se comprueban en el motor antes de cualquier `exec`, y la respuesta vuelve a la API como un tipo de `RespuestaMotor` sin credenciales ni rutas de socket.

#### Scenario: Un `exec`, `inspect`, `create` o `start` legítimo se ejecuta en el motor
- **GIVEN** solicitudes legítimas de la lista blanca enviadas por la API
- **WHEN** llegan al proceso motor con el UID autorizado
- **THEN** el motor SHALL ejecutarlas contra el daemon con el contrato actual
- **AND** la comprobación de la etiqueta y de las rutas confinadas SHALL ocurrir en el motor antes de cualquier `exec`

#### Scenario: La API no puede pedir una operación arbitraria
- **GIVEN** un handler de la API comprometido
- **WHEN** intenta pedir al motor una operación fuera de la lista blanca
- **THEN** el motor SHALL responder con un error tipado
- **AND** NO SHALL emitir ninguna llamada al daemon

### Requirement: Los argumentos de un `exec` llegan como argv literal

`Orden` SHALL construirse en el proceso motor y sus argumentos SHALL pasarse a bollard como una secuencia de cadenas literales. Un valor con metacomandos, punto y coma, sustitución de shell o saltos de línea SHALL permanecer dentro de un único argumento; NO SHALL interpretarse como shell ni como metacomando del cliente.

#### Scenario: Un argumento con metacomandos no se interpreta
- **GIVEN** un nombre de base o ruta con `;`, `$()` o un salto de línea
- **WHEN** el motor construye la `Orden` y crea el `exec`
- **THEN** el valor SHALL viajar como un argumento literal del argv
- **AND** NO SHALL ejecutarse ningún metacomando ni interpretarse como shell
