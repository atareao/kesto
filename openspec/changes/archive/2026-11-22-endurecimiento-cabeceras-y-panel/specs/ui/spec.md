# ui — delta

## Purpose

El documento del panel: sus scripts y estilos, bajo la política de seguridad de contenido que el servidor declara.

## ADDED Requirements

### Requirement: El documento del panel se sirve bajo la política de contenido del servidor

El documento del panel (`ui/index.html`) NO SHALL contener JavaScript en línea, para que la `Content-Security-Policy` del servidor pueda exigir `script-src 'self'` sin `'unsafe-inline'` ni hashes. El arranque de tema —que fija el tema antes del primer pintado— SHALL vivir en un fichero estático propio bajo `ui/public/`, servido por el mismo origen y cargado de forma síncrona en el `<head>`, para que no haya destello de tema.

El código de la aplicación NO SHALL usar `eval` ni `new Function`.

Los estilos en línea SÍ SHALL permitirse: la política del servidor declara `style-src 'self' 'unsafe-inline'` porque antd v5 inyecta estilos en tiempo de ejecución y los componentes usan atributos `style`.

#### Scenario: El documento no lleva script en línea
- **GIVEN** `ui/index.html`
- **WHEN** se inspecciona
- **THEN** NO SHALL contener ningún `<script>` con cuerpo en línea
- **AND** el arranque de tema SHALL cargarse desde un fichero estático propio

#### Scenario: El tema se aplica antes del primer pintado
- **GIVEN** el panel servido con el arranque de tema en un fichero propio, cargado de forma síncrona en el `<head>`
- **WHEN** se carga la aplicación bajo `script-src 'self'`
- **THEN** el tema del documento SHALL quedar fijado antes del primer pintado y NO SHALL haber destello
