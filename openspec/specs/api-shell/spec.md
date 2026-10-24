# api-shell Specification

## Purpose
La forma de la API HTTP: la envoltura JSON única de las respuestas y de los errores, el modelo de error con su traducción a códigos y los dos *fallbacks* que garantizan que ninguna respuesta salga sin envoltura, más el endpoint de salud que el contenedor necesita para su `healthcheck`. Fija la envoltura antes de que existan endpoints que la consuman, para que ninguno improvise la suya; y la salud se limita a la versión y al modo de autenticación porque es un endpoint sin sesión y no tiene por qué describir el despliegue.

## Requirements

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

### Requirement: El router se parte en público y protegido

El router SHALL partirse en dos mitades: una **pública** con la salud y las dos rutas del flujo de entrada —`/api/v1/auth/login` y `/api/v1/auth/callback`—, y una **protegida** con el resto de la superficie de negocio, a la que el *gate* de sesión SHALL colgarse con `route_layer` para que no alcance a las rutas públicas.

La partición NO SHALL construirse con el middleware sobre el router entero más una lista de excepciones: añadir una ruta pública obligaría a una excepción más, y olvidarla la dejaría detrás del *gate* sin que nada lo delate. La salud y el flujo de entrada son la lista blanca.

Con la autenticación apagada NO SHALL montarse la mitad protegida: la superficie queda pública, que es el modo de desarrollo, y las rutas de `/api/v1/auth/*` NO SHALL declararse.

Una ruta **no declarada** bajo `/api/v1` NO SHALL pasar por el *gate*: SHALL seguir contestando el `404` de la envoltura sin exigir sesión. Es lo medido —`route_layer` no envuelve el *fallback* que resuelve las rutas no declaradas—, y **es correcto**: un `404` no revela ningún recurso ni ningún método, así que no hay nada que proteger detrás. NO SHALL forzarse el *gate* sobre ese `404`. Queda escrito con escenario propio para que nadie lo lea mañana como un agujero y lo «arregle» envolviendo el *fallback* a mano: eso acoplaría las dos mitades y rompería la partición que este requisito fija.

#### Scenario: Las rutas públicas no pasan por el *gate*
- **GIVEN** el router en modo `Oidc`
- **WHEN** se pide `GET /api/v1/health` sin cookie
- **THEN** SHALL contestar `200`
- **AND** NO SHALL contestar `401`

#### Scenario: La superficie de negocio cuelga del router protegido
- **GIVEN** el router en modo `Oidc` con una ruta de negocio declarada en la mitad protegida
- **WHEN** se pide sin cookie
- **THEN** SHALL contestar `401` con la envoltura

#### Scenario: En modo desarrollo no hay mitad protegida
- **GIVEN** una configuración sin ninguna variable `OIDC_*`
- **WHEN** se construye el router
- **THEN** la superficie SHALL ser pública
- **AND** NO SHALL aplicarse ningún `401` por falta de sesión

#### Scenario: Una ruta no declarada no pasa por el *gate*
- **GIVEN** el router en modo `Oidc`
- **WHEN** se pide una ruta no declarada bajo `/api/v1` sin cookie
- **THEN** SHALL contestar `404` con la envoltura
- **AND** NO SHALL contestar `401`

### Requirement: Una petición sin sesión válida contesta con la misma envoltura

Una petición a la superficie protegida sin cookie de sesión válida SHALL contestar `401` con el mismo cuerpo JSON de error que el `404` y el `405` que ya existen —`{status, message}`— y NO SHALL contestar con un cuerpo vacío, ni con HTML, ni con una redirección.

El `status` del cuerpo SHALL duplicar el código de la cabecera, como en el resto de la envoltura.

En la mitad protegida el *gate* SHALL decidir **antes** que el enrutado por método: `method_not_allowed_fallback` SHALL declararse **antes** de colgar la capa con `route_layer`, de modo que la capa envuelva también ese *fallback*. Así, un **método no declarado** sobre una ruta protegida contesta `401` sin sesión y `405` —el de la envoltura— con sesión.

El motivo SHALL quedar escrito, porque no se deduce leyendo el router y la medición lo confirma: `method_not_allowed_fallback` **sustituye** el *fallback* del `MethodRouter`, y solo lo hace si el *fallback* es el de por defecto, así que declararlo **después** del `merge` reemplaza un *fallback* que ya estaba envuelto por otro que nunca vio la capa. Ese `405` sale **sin haber exigido sesión**, revelando a quien no ha entrado qué métodos existen. **El orden de las dos líneas de construcción del router es semántico, no cosmético**: invertirlas no rompe la compilación ni falla ninguna prueba de ruta válida; solo abre ese agujero.

#### Scenario: Un método no declarado sin sesión no contesta `405`
- **GIVEN** el router en modo `Oidc` y una ruta protegida que solo declara `GET`
- **WHEN** se pide esa ruta con un método no declarado y sin cookie
- **THEN** SHALL contestar `401` con la envoltura
- **AND** NO SHALL contestar `405`

#### Scenario: Un método no declarado con sesión contesta `405`
- **GIVEN** el router en modo `Oidc`, una ruta protegida que solo declara `GET` y una cookie de sesión válida
- **WHEN** se pide esa ruta con un método no declarado
- **THEN** SHALL contestar `405` con la envoltura
- **AND** el cuerpo SHALL ser JSON con `status` y `message`

#### Scenario: El `401` trae la envoltura
- **GIVEN** el router en modo `Oidc`
- **WHEN** se pide una ruta protegida sin cookie
- **THEN** el cuerpo SHALL ser JSON con `status` y `message`
- **AND** `status` SHALL ser `401`

#### Scenario: El `401` no es una redirección ni un cuerpo vacío
- **GIVEN** una petición sin cookie a una ruta protegida
- **WHEN** se atiende
- **THEN** el cuerpo NO SHALL estar vacío
- **AND** la respuesta NO SHALL ser `3xx` ni traer cabecera `Location`
