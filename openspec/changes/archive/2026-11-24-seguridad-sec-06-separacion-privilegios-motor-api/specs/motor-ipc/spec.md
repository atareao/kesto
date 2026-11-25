# motor-ipc — delta

## Purpose

Contrato del canal local entre el proceso que sirve la API y `kesto-motor`, el único proceso que posee el socket del daemon. Define el framing, el tope, la lista blanca cerrada de operaciones, la identificación del par, la redacción de secretos y los errores tipados. La API NO SHALL poder reenviar HTTP crudo al daemon: solo SHALL poder pedir una operación de la lista blanca.

## ADDED Requirements

### Requirement: El canal es un socket Unix con framing de longitud big-endian y tope de 1 MiB

El canal SHALL ser un Unix domain socket cuya ruta se configura con `KESTO_MOTOR_SOCKET` (defecto `/run/kesto/motor.sock`). Cada mensaje SHALL llevar una longitud de `u32` en **big-endian** seguida de un cuerpo JSON UTF-8. El cuerpo NO SHALL superar **1 MiB**; los volcados NO SHALL viajar por este canal, sino por streaming directo entre el daemon y el archivador.

Al leer, una longitud inválida, una longitud mayor que el tope o un cuerpo incompleto SHALL descartarse sin pánico y cerrando la conexión. Los tipos del canal SHALL ser `SolicitudMotor` y `RespuestaMotor`, definidos en `src/ipc/`; el protocolo NO SHALL transportar URLs, cabeceras ni métodos HTTP arbitrarios.

#### Scenario: Un mensaje con longitud inválida se descarta
- **GIVEN** un cliente conectado al socket IPC
- **WHEN** envía una longitud que no corresponde a un `u32` utilizable o un cuerpo más corto de lo declarado
- **THEN** el mensaje SHALL descartarse sin pánico
- **AND** la conexión SHALL cerrarse sin que se emita ninguna llamada a bollard

#### Scenario: Un mensaje que excede el tope se descarta
- **GIVEN** un cliente conectado al socket IPC
- **WHEN** envía una longitud declarada mayor que 1 MiB, o un cuerpo de ese tamaño
- **THEN** el mensaje SHALL descartarse sin pánico
- **AND** el motor SHALL seguir aceptando otras conexiones

#### Scenario: Una solicitud legítima usa el framing acotado
- **GIVEN** un cliente con el UID autorizado
- **WHEN** envía una solicitud de la lista blanca con su longitud big-endian y su cuerpo JSON dentro del tope
- **THEN** el motor SHALL decodificarla y responder con `RespuestaMotor` tipada

### Requirement: La lista blanca de solicitudes es cerrada

`SolicitudMotor` SHALL tener exactamente seis variantes: `Comprobar`, `CrearContenedor`, `IniciarContenedor`, `CrearExec`, `InspeccionarExec` e `IniciarExec`. Cualquier otra variante, nombre desconocido o estructura que no corresponda a una de ellas SHALL rechazarse con un error tipado antes de llegar al daemon. `RespuestaMotor` SHALL distinguir éxito y error con variantes tipadas, y el error devuelto a la API SHALL NOT incluir la ruta del socket, `DOCKER_HOST` ni credenciales del `env`.

#### Scenario: Una operación fuera de la lista blanca se rechaza
- **GIVEN** un mensaje IPC bien formado con una variante distinta de las seis permitidas
- **WHEN** llega al motor
- **THEN** SHALL rechazarse con un error tipado
- **AND** NO SHALL emitirse ninguna llamada a bollard

#### Scenario: Las seis operaciones permitidas se ejecutan en el motor
- **GIVEN** solicitudes `Comprobar`, `CrearContenedor`, `IniciarContenedor`, `CrearExec`, `InspeccionarExec` e `IniciarExec` con datos válidos
- **WHEN** llegan al motor con el UID autorizado
- **THEN** cada una SHALL ejecutarse en el proceso motor con su contrato actual
- **AND** la etiqueta de fuente de copia y las rutas confinadas SHALL comprobarse antes de cualquier `exec`

### Requirement: El motor identifica al par por UID antes de procesar

El motor SHALL exigir `KESTO_API_UID` para arrancar y SHALL contrastarlo con la credencial del par obtenida mediante `SO_PEERCRED` al aceptar la conexión. Un UID distinto SHALL cerrar la conexión **antes de leer el cuerpo**, sin procesar la solicitud ni emitir llamadas al daemon. El motor SHALL NOT aceptar la identidad del par desde ningún campo del mensaje.

#### Scenario: Un par con UID distinto se cierra sin procesar
- **GIVEN** un proceso local conectado al socket IPC con un UID distinto de `KESTO_API_UID`
- **WHEN** envía una solicitud de la lista blanca
- **THEN** la conexión SHALL cerrarse sin procesar la solicitud
- **AND** NO SHALL crearse ningún `exec` ni llamarse al daemon

#### Scenario: Sin `KESTO_API_UID` el motor no arranca
- **GIVEN** un entorno sin `KESTO_API_UID`
- **WHEN** se intenta arrancar `kesto-motor`
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_API_UID`

### Requirement: El IPC no filtra secretos y no sustituye la frontera de proceso

`Debug` de `SolicitudMotor`, `RespuestaMotor` y `Orden` SHALL redactar el campo `env` y cualquier otra credencial. El proceso API SHALL NOT poseer `DOCKER_HOST` ni el socket del daemon y SHALL NOT enlazar `bollard`; un fallo, `panic` o inycción en su superficie SHALL NOT entregar el socket. El proceso motor SHALL permanecer vivo ante un fallo de la API.

#### Scenario: Un fallo de la API no entrega el socket
- **GIVEN** un `panic` o un `unwrap` explotable en un handler de la API
- **WHEN** se intenta alcanzar el daemon desde ese proceso
- **THEN** el binario API SHALL NOT poseer el socket ni `DOCKER_HOST`
- **AND** la petición SHALL NOT llegar al daemon
- **AND** el proceso motor SHALL seguir vivo

#### Scenario: `Debug` y los errores no publican credenciales
- **GIVEN** una solicitud IPC cuyo `Orden` lleva credenciales en `env`
- **WHEN** se imprime con `Debug` o se traduce su error a una respuesta de la API
- **THEN** el valor de `env` SHALL aparecer redactado
- **AND** la respuesta SHALL NOT contener la ruta del socket ni credenciales del daemon
