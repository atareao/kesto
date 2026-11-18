# ui Specification

## MODIFIED Requirements

### Requirement: El layout ofrece las cinco pantallas del §6 y una ruta para cada una

El panel SHALL montarse en un layout con menú lateral y SHALL ofrecer, como mínimo, las rutas `/` (Dashboard), `/tareas`, `/historico`, `/snapshots` y `/ajustes`. Cada entrada del menú SHALL navegar a su ruta sin recargar el documento, la entrada de la ruta activa SHALL distinguirse visualmente, y una ruta desconocida SHALL renderizar una pantalla de «no encontrada» **dentro** del layout, sin perder el menú. En una ventana estrecha el menú SHALL poder plegarse sin dejar ninguna entrada inalcanzable. El control de plegado del menú SHALL residir en el pie de la barra lateral —dentro de ella y a continuación del menú—, no en una cabecera aparte. La barra lateral SHALL montarse solo cuando haya panel (`auth_mode` `disabled` o `oidc` con sesión) y NO SHALL montarse sin sesión.

Las cinco pantallas y sus nombres salen del §6. Las rutas son del cliente y no del servidor: por eso el backend sirve `index.html` como fallback y una recarga en `/tareas` tiene que seguir funcionando. El control que pliega el menú pertenece a la barra que pliega: ponerlo dentro de la barra lateral lo deja donde el operador lo busca y libera la cabecera.

#### Scenario: Cada entrada del menú abre su pantalla
- **GIVEN** el panel cargado en `/`
- **WHEN** se pulsa cada una de las cinco entradas del menú
- **THEN** SHALL mostrarse la pantalla de esa entrada
- **AND** la URL SHALL cambiar a su ruta
- **AND** NO SHALL recargarse el documento

#### Scenario: La ruta activa se marca en el menú
- **GIVEN** el panel abierto en `/historico`
- **WHEN** se pinta el menú
- **THEN** la entrada de Histórico SHALL ser la marcada como activa
- **AND** las demás NO SHALL estarlo

#### Scenario: Una ruta desconocida no rompe el layout
- **GIVEN** una URL del cliente que no corresponde a ninguna pantalla
- **WHEN** se carga el panel
- **THEN** SHALL mostrarse la pantalla de «no encontrada»
- **AND** el menú lateral SHALL seguir visible y operativo

#### Scenario: El control de plegado vive en el pie de la barra lateral
- **GIVEN** el panel con la barra lateral desplegada
- **WHEN** se pinta la barra lateral
- **THEN** el control de plegado SHALL estar dentro de la barra lateral, a continuación del menú
- **AND** SHALL ser alcanzable y activable con el teclado
- **AND** SHALL conservar su nombre accesible y su estado plegado/desplegado

#### Scenario: Sin sesión la barra lateral no se monta
- **GIVEN** el modo `oidc` sin sesión
- **WHEN** se abre cualquier ruta
- **THEN** el menú lateral NO SHALL montarse

### Requirement: El cliente de la API consume la envoltura y no filtra secretos

Toda llamada del panel SHALL dirigirse a `/api/v1/*` en el mismo origen, con credenciales de cookie incluidas, y SHALL interpretar la respuesta según la envoltura de `api-shell`: una respuesta atendida trae `{status, message, data}` y una fallida trae `{status, message}`. El cliente SHALL comprobar que `status` coincide con el código HTTP, SHALL tratar un cuerpo que no sea la envoltura como error de protocolo y NO SHALL inventar un `data` vacío para disimularlo.

Las fechas SHALL presentarse al usuario en la zona horaria local a partir de la cadena RFC3339 que publica la API, sin reinterpretarla como si fuera un instante sin desplazamiento, y los identificadores UUID SHALL conservarse en su forma canónica con guiones en cualquier comparación, clave de lista o URL. El panel NO SHALL mostrar, registrar ni guardar en almacenamiento del navegador ningún campo de secreto; para las credenciales SHALL usar únicamente los booleanos `has_*` que publica la API.

Los mensajes de error SHALL ser el `message` de la envoltura, presentado como texto y nunca como HTML, y NO SHALL incluir el cuerpo de la petición ni el valor de un campo del formulario.

#### Scenario: Una respuesta correcta se entrega como datos
- **GIVEN** una respuesta `200` con `{status: 200, message, data}`
- **WHEN** el cliente la procesa
- **THEN** SHALL devolver `data` a la pantalla
- **AND** NO SHALL mostrar ningún error

#### Scenario: Un error de la API se muestra con su mensaje y sin el cuerpo enviado
- **GIVEN** una respuesta `400` cuyo `message` nombra el campo `cron_expression`
- **WHEN** el cliente la procesa
- **THEN** la pantalla SHALL mostrar ese `message`
- **AND** NO SHALL mostrar el valor que se había escrito en el formulario como parte del error

#### Scenario: Un cuerpo fuera de la envoltura se trata como error de protocolo
- **GIVEN** una respuesta `200` cuyo cuerpo es JSON pero no trae `status` ni `data`
- **WHEN** el cliente la procesa
- **THEN** SHALL producirse un error de protocolo
- **AND** NO SHALL pintarse una pantalla vacía como si la petición hubiera ido bien

#### Scenario: Un 401 devuelve al usuario a la pantalla de sesión
- **GIVEN** una petición cualquiera que contesta `401` con la envoltura
- **WHEN** el cliente la recibe
- **THEN** SHALL descartarse la caché de datos de la sesión
- **AND** SHALL mostrarse la portada pública en `/`

### Requirement: La sesión se adapta al modo de autenticación y no llama a rutas que no existen

En modo de desarrollo (`auth_mode` `disabled`) el panel SHALL entrar directamente al Dashboard, NO SHALL pedir `GET /api/v1/auth/session` y NO SHALL llamar a ninguna ruta `/api/v1/auth/*`, que en ese modo contesta `404`.

En modo OIDC el panel SHALL consultar `GET /api/v1/auth/session` al arrancar. Si contesta `200`, SHALL mostrar el `sub` y el `email` de la sesión en la barra lateral. Si contesta `401`, SHALL mostrar la portada pública en `/` con un botón que navegue a `GET /api/v1/auth/login` en el mismo documento —la redirección la hace el servidor— y NO SHALL reintentar la petición en bucle. El login SHALL ofrecerse desde la portada pública (`/`). Sin sesión, las rutas protegidas SHALL desviarse a `/`. El cierre de sesión SHALL llamarse desde el pie de la barra lateral, junto al conmutador de tema, SHALL llamar a `POST /api/v1/auth/logout`, SHALL vaciar la caché de consultas y SHALL volver a `/`.

#### Scenario: En desarrollo no se pide sesión
- **GIVEN** un `GET /api/v1/health` que declara `auth_mode: disabled`
- **WHEN** se carga el panel
- **THEN** SHALL mostrarse el Dashboard
- **AND** NO SHALL pedirse `GET /api/v1/auth/session`

#### Scenario: Con sesión se muestra la identidad
- **GIVEN** una sesión OIDC válida con `sub` y `email`
- **WHEN** se abre el panel con sesión
- **THEN** SHALL mostrarse el `sub` y el `email` en la barra lateral
- **AND** NO SHALL mostrarse ningún token

#### Scenario: Sin sesión se ofrece entrar
- **GIVEN** el modo OIDC y un `401` de `GET /api/v1/auth/session`
- **WHEN** se carga el panel
- **THEN** SHALL mostrarse la portada pública en `/`
- **AND** el botón de entrada SHALL navegar a `/api/v1/auth/login`
- **AND** NO SHALL repetirse la petición de sesión en bucle

#### Scenario: Sin sesión las rutas protegidas desvían a la portada
- **GIVEN** el modo OIDC sin sesión
- **WHEN** se abre una ruta protegida distinta de `/`
- **THEN** SHALL navegarse a `/`
- **AND** NO SHALL navegarse a `/sesion`

#### Scenario: Al salir se vacía la caché
- **GIVEN** una sesión abierta con datos cargados
- **WHEN** se cierra la sesión desde el pie de la barra lateral y el servidor contesta
- **THEN** SHALL volverse a `/`
- **AND** las consultas cacheadas NO SHALL seguir visibles

### Requirement: El panel declara su identidad de marca

El panel SHALL servir los assets de marca de Kesto y SHALL declararlos en el documento. En concreto SHALL servirse el favicon (`favicon.ico`, `favicon-16x16.png`, `favicon-32x32.png`, `favicon-48x48.png`, `favicon-64x64.png`), el icono para dispositivos Apple (`apple-touch-icon.png`), los iconos de Android (`android-chrome-192x192.png`, `android-chrome-512x512.png`), el icono enmascarable (`maskable-icon-512x512.png`), el icono de Windows (`mstile-150x150.png`) y el manifiesto (`site.webmanifest`). El documento SHALL declarar los `<link>` de iconos, el `<link rel="manifest">`, la meta `theme-color` con el azul de marca y las meta de Open Graph, y el menú lateral SHALL mostrar el logo de Kesto además del nombre cuando la barra lateral está desplegada. Con la barra lateral plegada el nombre NO SHALL renderizarse y el logo SHALL permanecer visible. Además, la portada pública SHALL mostrar el logo de Kesto, un único asset con fondo transparente legible en el tema activo, además de la marca de la barra lateral.

El porqué: hoy la pestaña del navegador no identifica a Kesto —no hay favicon ni manifest— y la marca del panel es solo texto. Una aplicación que el operador deja abierta todo el día debe poder reconocerse en una pestaña y en un marcador. Con la barra plegada el nombre no cabe y solo estorba: retirarlo deja el logo limpio y centrado. La portada pública es la primera impresión de Kesto para quien todavía no ha entrado, así que reutiliza el mismo logo, legible en cualquier tema.

#### Scenario: La pestaña identifica a Kesto
- **GIVEN** el panel cargado en el navegador
- **WHEN** se lee el documento servido
- **THEN** SHALL declararse un `<link rel="icon">` y un `<link rel="manifest">`
- **AND** el favicon y el manifest SHALL servirse desde la raíz del panel

#### Scenario: El menú lateral muestra el logo
- **GIVEN** el panel con el layout desplegado
- **WHEN** se pinta el menú lateral
- **THEN** SHALL verse el logo de Kesto
- **AND** el logo SHALL ser legible en el tema activo

#### Scenario: La marca se oculta al plegar
- **GIVEN** el panel con la barra lateral plegada
- **WHEN** se pinta la marca del panel
- **THEN** el nombre «Kesto» NO SHALL renderizarse
- **AND** el logo SHALL seguir visible

#### Scenario: La marca se ve desplegada
- **GIVEN** el panel con la barra lateral desplegada
- **WHEN** se pinta la marca del panel
- **THEN** SHALL verse el nombre «Kesto» junto al logo

#### Scenario: La portada pública muestra el logo de Kesto
- **GIVEN** la portada pública visible en `/`
- **WHEN** se pinta la portada
- **THEN** SHALL verse el logo de Kesto
- **AND** el logo SHALL ser legible en el tema activo
