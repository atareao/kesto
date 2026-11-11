# api-shell Specification (delta)

## ADDED Requirements

### Requirement: La salud contesta sin sesión y sin describir el despliegue

`GET /api/v1/health` SHALL contestar `200` sin exigir autenticación, y SHALL limitar su cuerpo a la versión del binario y el modo de autenticación efectivo. NO SHALL incluir la dirección de escucha, ni la cadena de base de datos, ni el emisor OIDC.

Es lo que consulta el `healthcheck` del contenedor, y un health check que exija sesión no sirve para su propósito; pero es también un endpoint sin autenticar, así que no tiene por qué describir el despliegue a quien lo consulte.

La ruta NO SHALL declararse con barra final.

#### Scenario: La salud contesta sin credenciales
- **GIVEN** el router de la aplicación
- **WHEN** se pide `GET /api/v1/health` sin cabecera de sesión
- **THEN** SHALL contestar `200`
- **AND** el cuerpo SHALL traer la versión del binario

#### Scenario: La salud no describe el despliegue
- **GIVEN** una configuración con una dirección de escucha y una cadena de base de datos propias
- **WHEN** se pide `GET /api/v1/health`
- **THEN** el cuerpo NO SHALL contener ni la dirección de escucha ni la cadena de base de datos

#### Scenario: La salud declara el modo de autenticación efectivo
- **GIVEN** una configuración sin ninguna variable `OIDC_*`
- **WHEN** se pide `GET /api/v1/health`
- **THEN** el modo declarado SHALL ser el de desarrollo

#### Scenario: La ruta sin barra final responde
- **WHEN** se pide `GET /api/v1/health`
- **THEN** SHALL contestar `200`

### Requirement: Una ruta desconocida contesta con la misma envoltura

Una petición a una ruta no declarada bajo `/api/v1` SHALL contestar `404` con el cuerpo JSON de error de la API, y NO SHALL contestar con un cuerpo vacío.

#### Scenario: Una ruta que no existe
- **GIVEN** el router de la aplicación
- **WHEN** se pide `GET /api/v1/no-existe`
- **THEN** SHALL contestar `404`
- **AND** el cuerpo SHALL ser JSON con `status` y `message`

### Requirement: Una envoltura única para las respuestas

Toda respuesta de la API SHALL ser JSON con `{status, message, data}` cuando la petición se atendió, y `{status, message}` cuando falló. El campo `status` SHALL duplicar el código HTTP de la respuesta.

#### Scenario: La forma de una respuesta correcta
- **WHEN** se atiende una petición con éxito
- **THEN** el cuerpo SHALL ser JSON con `status`, `message` y `data`
- **AND** `status` SHALL coincidir con el código HTTP

#### Scenario: La forma de una respuesta de error
- **WHEN** una petición falla
- **THEN** el cuerpo SHALL ser JSON con `status` y `message`
- **AND** `status` SHALL coincidir con el código HTTP

#### Scenario: Un método no declarado contesta con la misma envoltura
- **GIVEN** el router de la aplicación
- **WHEN** se pide `POST /api/v1/health`, ruta que solo admite `GET`
- **THEN** SHALL contestar `405`
- **AND** el cuerpo SHALL ser JSON con `status` y `message`

### Requirement: Un error interno no cuenta por qué falló

Una respuesta `5xx` SHALL llevar un mensaje genérico y NO SHALL incluir el texto de la causa. El detalle SHALL registrarse en el log del servidor.

Kesto va a manipular credenciales S3 y cadenas de conexión a bases de datos, y el mensaje de un error de biblioteca es justo donde acaban.

#### Scenario: Un fallo interno no filtra la causa
- **GIVEN** un error interno cuyo texto contiene una credencial
- **WHEN** se convierte en respuesta
- **THEN** el código SHALL ser `500`
- **AND** el cuerpo NO SHALL contener el texto de la causa

#### Scenario: Un error de entrada sí explica qué se esperaba
- **GIVEN** un error de entrada inválida
- **WHEN** se convierte en respuesta
- **THEN** el código SHALL ser `400`
- **AND** el cuerpo SHALL incluir el motivo

### Requirement: El cierre ordenado espera a las peticiones en curso

La aplicación SHALL cerrar el servidor cuando la señal de cierre se resuelva, y SHALL dar salida a las peticiones en curso antes de terminar, para que el `restart: always` del §8 no corte una respuesta a medias.

La señal de cierre SHALL ser inyectable, de modo que el mecanismo sea comprobable sin levantar un proceso ni enviarle señales.

#### Scenario: El servidor se apaga cuando la señal de cierre se resuelve
- **GIVEN** el servidor escuchando con una señal de cierre inyectada
- **WHEN** la señal se resuelve
- **THEN** el servidor SHALL dejar de aceptar conexiones
- **AND** la función de arranque SHALL devolver

#### Scenario: Las señales del sistema se atienden
- **GIVEN** el binario en ejecución
- **WHEN** recibe `SIGTERM`
- **THEN** SHALL cerrar el servidor de forma ordenada
- **AND** el proceso SHALL terminar con éxito
