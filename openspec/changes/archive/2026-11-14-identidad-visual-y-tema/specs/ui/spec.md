## ADDED Requirements

### Requirement: El panel declara su identidad de marca

El panel SHALL servir los assets de marca de Kesto y SHALL declararlos en el documento. En concreto SHALL servirse el favicon (`favicon.ico`, `favicon-16x16.png`, `favicon-32x32.png`, `favicon-48x48.png`, `favicon-64x64.png`), el icono para dispositivos Apple (`apple-touch-icon.png`), los iconos de Android (`android-chrome-192x192.png`, `android-chrome-512x512.png`), el icono enmascarable (`maskable-icon-512x512.png`), el icono de Windows (`mstile-150x150.png`) y el manifiesto (`site.webmanifest`). El documento SHALL declarar los `<link>` de iconos, el `<link rel="manifest">`, la meta `theme-color` con el azul de marca y las meta de Open Graph, y el menú lateral SHALL mostrar el logo de Kesto además del nombre.

El porqué: hoy la pestaña del navegador no identifica a Kesto —no hay favicon ni manifest— y la marca del panel es solo texto. Una aplicación que el operador deja abierta todo el día debe poder reconocerse en una pestaña y en un marcador.

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

### Requirement: El panel ofrece tema claro y oscuro, con oscuro por defecto y elección persistida

El panel SHALL arrancar en **tema oscuro** siempre que no exista una elección guardada. Un control en el layout SHALL permitir alternar entre tema claro y oscuro; al activarlo, el tema SHALL cambiar de inmediato en todo el panel y la elección SHALL persistir en el almacenamiento del navegador, de modo que una recarga la respete. El arranque NO SHALL seguir la preferencia del sistema por sí solo: sin elección guardada, el resultado SHALL ser el oscuro.

El control SHALL ser accesible: SHALL tener nombre accesible, SHALL ser alcanzable y activable solo con teclado, y SHALL anunciar el tema hacia el que cambia, no solo su color. Ningún color del panel SHALL quedar fijado a un tema: los colores SHALL proceder del tema activo, de modo que ambos sean legibles.

El porqué: el panel se usa en sesiones largas y la identidad visual pide el modo oscuro por defecto, pero el operador debe poder pasar a claro y que su elección se recuerde. Que el color esté fijado a mano es justo lo que hoy hace ilegible una mitad del producto.

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

#### Scenario: Ambos temas son legibles
- **GIVEN** el panel en cualquiera de los dos temas
- **WHEN** se recorre la cabecera, el menú lateral y el contenido
- **THEN** NO SHALL haber texto sobre un fondo del mismo color
- **AND** los colores SHALL proceder del tema activo
