# Spec Delta

## ADDED Requirements

### Requirement: El panel compilado se sirve en el mismo origen, después del router de la API

El servidor SHALL servir el contenido de `ui/dist` en el mismo origen que `/api/v1`, y SHALL devolver `ui/dist/index.html` con `200` y `text/html` para cualquier ruta GET o HEAD que no empiece por `/api/` y no corresponda a un fichero existente del panel. El fallback SPA SHALL resolverse **después** de que el router de la API haya declinado la ruta: NO SHALL interceptar ninguna ruta de `/api/v1`, que conserva su `404` y su `405` con la envoltura.

El documento del panel y sus assets NO SHALL exigir sesión, porque el panel es lo que pinta la pantalla de entrada y lo que permite iniciar el flujo OIDC; lo que queda detrás del *gate* es la superficie de datos, no el HTML que la consume. Servir el documento tras el *gate* dejaría al navegador sin aplicación con la que entrar y convertiría el `401` en la única pantalla posible. Esta excepción NO SHALL relajar la lista blanca de `/api/v1`: ninguna ruta de negocio se sirve sin sesión por servir el panel.

Los assets con hash en el nombre SHALL poder cachearse como inmutables, y `index.html` NO SHALL cachearse, para que un despliegue nuevo no deje a un navegador con un documento viejo apuntando a assets que ya no existen. El servido NO SHALL permitir salir de `ui/dist`: una ruta con `..` o un fichero oculto NO SHALL entregarse aunque exista en el sistema de ficheros. Si `ui/dist` no existe, el arranque SHALL seguir funcionando y la petición SHALL contestar el `404` con la envoltura, para que el backend sin panel compile, arranque y pase su suite.

#### Scenario: La raíz sirve el documento del panel
- **GIVEN** un despliegue con `ui/dist/index.html`
- **WHEN** se pide `GET /`
- **THEN** SHALL contestar `200`
- **AND** el tipo de contenido SHALL ser `text/html`
- **AND** el cuerpo SHALL ser el del `index.html`

#### Scenario: Una ruta del cliente se resuelve con el fallback
- **GIVEN** una ruta del cliente como `/tareas` que no es un fichero
- **WHEN** se pide `GET /tareas`
- **THEN** SHALL contestar `200` con `index.html`
- **AND** NO SHALL contestar `404`

#### Scenario: Un asset existente se sirve con su tipo
- **GIVEN** un fichero `ui/dist/assets/app-abc123.js`
- **WHEN** se pide
- **THEN** SHALL contestar `200`
- **AND** el tipo de contenido SHALL ser el de JavaScript

#### Scenario: Un asset con hash se cachea como inmutable y el documento no
- **GIVEN** una respuesta para un asset con hash y otra para `index.html`
- **WHEN** se comparan sus cabeceras de caché
- **THEN** la del asset SHALL permitir caché de larga duración
- **AND** la del documento SHALL exigir revalidación

#### Scenario: Una ruta de la API no cae en el fallback
- **GIVEN** el panel desplegado
- **WHEN** se pide `GET /api/v1/no-existe`
- **THEN** SHALL contestar `404` con la envoltura de la API
- **AND** NO SHALL servirse `index.html`

#### Scenario: Una ruta de la API sin sesión no se sirve por el panel
- **GIVEN** el modo OIDC y una petición sin cookie a `GET /api/v1/jobs`
- **WHEN** se atiende
- **THEN** SHALL contestar `401` con la envoltura
- **AND** NO SHALL servirse el documento del panel

#### Scenario: No se sale del directorio del panel
- **GIVEN** una petición a una ruta que intenta salir de `ui/dist`
- **WHEN** se atiende
- **THEN** SHALL contestar `404` o `400`
- **AND** NO SHALL entregarse ningún fichero de fuera del directorio

#### Scenario: Sin panel compilado el backend sigue en pie
- **GIVEN** un árbol sin `ui/dist`
- **WHEN** se pide `GET /`
- **THEN** SHALL contestar `404` con la envoltura
- **AND** el servidor NO SHALL entrar en pánico ni negarse a arrancar

#### Scenario: Los métodos del fallback se limitan a lectura
- **WHEN** se pide `POST /tareas`
- **THEN** SHALL contestar `405` con la envoltura
- **AND** NO SHALL servirse el documento del panel