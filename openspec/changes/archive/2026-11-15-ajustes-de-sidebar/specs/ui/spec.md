## MODIFIED Requirements

### Requirement: El layout ofrece las cinco pantallas del §6 y una ruta para cada una

El panel SHALL montarse en un layout con menú lateral y SHALL ofrecer, como mínimo, las rutas `/` (Dashboard), `/tareas`, `/historico`, `/snapshots`, `/ajustes` y `/sesion`. Cada entrada del menú SHALL navegar a su ruta sin recargar el documento, la entrada de la ruta activa SHALL distinguirse visualmente, y una ruta desconocida SHALL renderizar una pantalla de «no encontrada» **dentro** del layout, sin perder el menú. En una ventana estrecha el menú SHALL poder plegarse sin dejar ninguna entrada inalcanzable. El control de plegado del menú SHALL residir en el pie de la barra lateral —dentro de ella y a continuación del menú—, no en una cabecera aparte.

Las cinco pantallas y sus nombres salen del §6; la de sesión es la que falta en esa lista porque el §6 describe la pantalla ya dentro del producto, no la entrada. Las rutas son del cliente y no del servidor: por eso el backend sirve `index.html` como fallback y una recarga en `/tareas` tiene que seguir funcionando. El control que pliega el menú pertenece a la barra que pliega: ponerlo dentro de la barra lateral lo deja donde el operador lo busca y libera la cabecera.

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

### Requirement: El panel declara su identidad de marca

El panel SHALL servir los assets de marca de Kesto y SHALL declararlos en el documento. En concreto SHALL servirse el favicon (`favicon.ico`, `favicon-16x16.png`, `favicon-32x32.png`, `favicon-48x48.png`, `favicon-64x64.png`), el icono para dispositivos Apple (`apple-touch-icon.png`), los iconos de Android (`android-chrome-192x192.png`, `android-chrome-512x512.png`), el icono enmascarable (`maskable-icon-512x512.png`), el icono de Windows (`mstile-150x150.png`) y el manifiesto (`site.webmanifest`). El documento SHALL declarar los `<link>` de iconos, el `<link rel="manifest">`, la meta `theme-color` con el azul de marca y las meta de Open Graph, y el menú lateral SHALL mostrar el logo de Kesto además del nombre cuando la barra lateral está desplegada. Con la barra lateral plegada el nombre NO SHALL renderizarse y el logo SHALL permanecer visible.

El porqué: hoy la pestaña del navegador no identifica a Kesto —no hay favicon ni manifest— y la marca del panel es solo texto. Una aplicación que el operador deja abierta todo el día debe poder reconocerse en una pestaña y en un marcador. Con la barra plegada el nombre no cabe y solo estorba: retirarlo deja el logo limpio y centrado.

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

### Requirement: El panel ofrece tema claro y oscuro, con oscuro por defecto y elección persistida

El panel SHALL arrancar en **tema oscuro** siempre que no exista una elección guardada. Un control en el layout SHALL permitir alternar entre tema claro y oscuro; al activarlo, el tema SHALL cambiar de inmediato en todo el panel y la elección SHALL persistir en el almacenamiento del navegador, de modo que una recarga la respete. El arranque NO SHALL seguir la preferencia del sistema por sí solo: sin elección guardada, el resultado SHALL ser el oscuro. El control de tema SHALL residir en el pie de la barra lateral, junto al control de plegado.

El control SHALL ser accesible: SHALL tener nombre accesible, SHALL ser alcanzable y activable solo con teclado, y SHALL anunciar el tema hacia el que cambia, no solo su color. Ningún color del panel SHALL quedar fijado a un tema: los colores SHALL proceder del tema activo, de modo que ambos sean legibles.

El porqué: el panel se usa en sesiones largas y la identidad visual pide el modo oscuro por defecto, pero el operador debe poder pasar a claro y que su elección se recuerde. Que el color esté fijado a mano es justo lo que hoy hace ilegible una mitad del producto. El control de tema se agrupa con el de plegado, al pie de la barra lateral, para que los dos mandos del chasis queden juntos.

#### Scenario: Por defecto, oscuro
- **GIVEN** un navegador sin ninguna elección de tema guardada
- **WHEN** se carga el panel
- **THEN** el tema activo SHALL ser el oscuro

#### Scenario: La elección persiste tras recargar
- **GIVEN** el panel en tema oscuro
- **WHEN** se activa el control de tema y se recarga el panel
- **THEN** el tema activo SHALL ser el claro
- **AND** NO SHALL volver al oscuro por defecto

#### Scenario: Un control accesible
- **GIVEN** el foco en el control de tema
- **WHEN** se activa con el teclado
- **THEN** el tema SHALL cambiar
- **AND** el control SHALL anunciar su estado al usuario

#### Scenario: El control de tema vive en el pie de la barra lateral
- **GIVEN** el panel con la barra lateral desplegada
- **WHEN** se pinta la barra lateral
- **THEN** el control de tema SHALL estar en el pie de la barra lateral, junto al de plegado
- **AND** SHALL ser alcanzable con el teclado

#### Scenario: Ambos temas son legibles
- **GIVEN** el panel en cualquiera de los dos temas
- **WHEN** se recorre la barra lateral, su pie y el contenido
- **THEN** NO SHALL haber texto sobre un fondo del mismo color
- **AND** los colores SHALL proceder del tema activo
