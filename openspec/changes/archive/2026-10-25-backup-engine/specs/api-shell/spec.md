# api-shell Specification

## Purpose
La forma de la API HTTP: la envoltura JSON única de las respuestas y de los errores, el modelo de error con su traducción a códigos y los dos *fallbacks* que garantizan que ninguna respuesta salga sin envoltura, la única excepción a esa envoltura —el stream de eventos, que no es JSON— y el endpoint de salud que el contenedor necesita para su `healthcheck`. Fija la envoltura antes de que existan endpoints que la consuman, para que ninguno improvise la suya; y la salud se limita a la versión y al modo de autenticación porque es un endpoint sin sesión y no tiene por qué describir el despliegue.

## ADDED Requirements

### Requirement: El stream de eventos es la única respuesta que no lleva la envoltura

`GET /api/v1/events` SHALL contestar `text/event-stream` y NO SHALL envolver su cuerpo en la envoltura JSON, que NO SHALL aplicarse a ningún otro punto de la API. La envoltura SHALL seguir rigiendo todas las demás respuestas, incluidas las de error del propio stream antes de abrirse.

Un `text/event-stream` con la envoltura dentro de un `data:` no es un `text/event-stream`: el cliente que lo lee espera líneas `event:` y `data:` y recibiría un objeto JSON que tendría que desenvolver, con lo que el formato dejaría de ser el que el §5 publica. Es una excepción y hay que declararla precisamente para que siga siendo **una**: si se dejara como un caso particular sin escribirlo, el día que aparezca una segunda ruta que quiera saltarse la envoltura nadie sabría si está permitido.

La excepción NO SHALL extenderse a los errores: una petición al stream sin sesión en modo OIDC contesta `401` **con** la envoltura, porque ahí sí es una respuesta HTTP normal. La envoltura se cae cuando el stream ya está abierto, no antes.

#### Scenario: El stream no lleva la envoltura
- **GIVEN** una sesión válida y una ejecución en curso
- **WHEN** se lee el cuerpo de `GET /api/v1/events`
- **THEN** SHALL estar formado por líneas `event:` y `data:`
- **AND** NO SHALL haber una clave `data` con la envoltura dentro

#### Scenario: Un error antes de abrir el stream sí lleva la envoltura
- **GIVEN** el modo OIDC con la capa puesta y una petición al stream sin sesión
- **WHEN** se atiende
- **THEN** SHALL contestar `401`
- **AND** el cuerpo SHALL llevar la envoltura

#### Scenario: Las demás respuestas siguen envueltas
- **GIVEN** una petición al listado de recursos y otra a una ruta desconocida
- **WHEN** se atienden
- **THEN** las dos SHALL llevar la envoltura
- **AND** la única excepción SHALL ser el stream ya abierto

### Requirement: El stream se declara aparte en el router y no se le aplican las capas que envuelven

La ruta del stream SHALL declararse de forma que NO reciba las capas que añaden o transforman la envoltura, y SHALL seguir recibiendo la capa de sesión y la de traza. El *fallback* de ruta desconocida SHALL seguir contestando con la envoltura.

El requisito anterior fija **qué** sale por el cable; este fija **cómo** se consigue en el router, porque la forma evidente de añadirlo —colgarlo de la mitad protegida como las demás rutas— es la que menos cuesta y la que rompe el formato si alguna capa toca el cuerpo. Las dos capas que sí tienen que llegarle son la de sesión, que es lo que lo protege, y la de traza, que es lo que permite ver en el registro que alguien se conectó. Y el `fallback` no cambia: una ruta que no existe sigue siendo un `404` con envoltura.

#### Scenario: El stream no pasa por las capas que envuelven
- **GIVEN** el router montado
- **WHEN** se pide `GET /api/v1/events` con sesión válida
- **THEN** el cuerpo SHALL llegar en formato de eventos
- **AND** NO SHALL haberse envuelto

#### Scenario: La sesión sí llega al stream
- **GIVEN** el modo OIDC con la capa puesta
- **WHEN** se pide el stream sin sesión
- **THEN** SHALL contestar `401`

#### Scenario: Una ruta desconocida sigue contestando con la envoltura
- **GIVEN** una petición a una ruta que no existe
- **WHEN** se atiende
- **THEN** SHALL contestar `404` con la envoltura
