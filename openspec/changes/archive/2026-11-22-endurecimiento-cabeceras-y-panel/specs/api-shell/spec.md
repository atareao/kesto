# api-shell — delta

## Purpose

La composición HTTP del servidor: cabeceras de seguridad, HSTS y el servido del panel. Este delta cierra SEC-12 (la HSTS no cubría el panel) y SEC-18 (la CSP era solo `frame-ancestors`).

## MODIFIED Requirements

### Requirement: Las respuestas del servidor declaran sus cabeceras de seguridad

Todas las respuestas del servidor SHALL llevar `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer` y una `Content-Security-Policy` que prohíba enmarcar el panel y restrinja el resto de fuentes a lo que el panel necesita. El `Content-Type` y el `Cache-Control` de cada respuesta NO SHALL verse alterados: lo que se añade son cabeceras de seguridad, y el tipo y la caché siguen siendo la decisión del servido de ficheros.

La `Content-Security-Policy` SHALL declarar, como mínimo, `default-src 'self'`, `script-src 'self'`, `style-src 'self' 'unsafe-inline'`, `img-src 'self' data:`, `connect-src 'self'`, `object-src 'none'`, `base-uri 'self'`, `form-action 'self'` y `frame-ancestors 'none'`.

`script-src` NO SHALL incluir `'unsafe-inline'` ni `'unsafe-eval'`: el documento del panel NO SHALL llevar JavaScript en línea, así que la política no necesita ninguna excepción para scripts. El único script en línea del documento —el arranque de tema— SHALL vivir en un fichero estático propio servido por el mismo origen y cargado de forma síncrona antes del primer pintado.

`style-src` SHALL incluir `'unsafe-inline'` y el motivo SHALL quedar escrito: antd v5 inyecta estilos en tiempo de ejecución y los componentes usan atributos `style`, así que una política sin `'unsafe-inline'` dejaría el panel sin estilos. Es la única excepción de la política y es deliberada.

Cuando el operador configure `KESTO_HSTS_MAX_AGE`, toda respuesta SHALL llevar además `Strict-Transport-Security: max-age=<n>` —ver la capability `deploy`—. La HSTS NO SHALL declararse desde una capa que cubra solo una mitad del origen: forma parte de la misma capa que el resto de cabeceras y SHALL cubrir el panel y la API por igual.

Las cabeceras SHALL declararse una sola vez, **alrededor** del router que compone la API y el panel, y NO SHALL declararse por ruta. El panel es el *fallback* de ese router, así que una capa colgada de él cubriría lo que el panel produce —el documento, los assets y también los `404` de `/api/` de su guardia de prefijo, que corre dentro del propio *fallback*—, pero dejaría fuera lo que contestan las rutas declaradas de la API. Al envolver el router compuesto, la política es la misma para todo lo que sale de este origen y no depende de qué mitad atendió la petición. Esta capa NO SHALL cambiar la envoltura de `/api/v1`: ni el cuerpo, ni el código de estado, ni los encabezados que ya fijaba.

#### Scenario: Las cabeceras de seguridad acompañan a toda respuesta del origen
- **GIVEN** el panel desplegado con `ui/dist/index.html` y `ui/dist/assets/app-abc123.js`
- **WHEN** se pide `GET /`, `GET /assets/app-abc123.js` y `GET /api/v1/health`
- **THEN** las tres respuestas SHALL contestar `200`
- **AND** las tres SHALL llevar `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY` y `Referrer-Policy: no-referrer`
- **AND** las tres SHALL llevar una `Content-Security-Policy` que contenga `default-src 'self'`, `script-src 'self'`, `style-src 'self' 'unsafe-inline'` y `frame-ancestors 'none'`
- **AND** el `Content-Type` y el `Cache-Control` del documento y del asset SHALL ser los que fija el servido del panel
- **AND** el cuerpo de `GET /api/v1/health` SHALL seguir siendo la envoltura de la API

#### Scenario: La política de scripts no admite código en línea
- **WHEN** se inspecciona la `Content-Security-Policy` de una respuesta
- **THEN** la directiva `script-src` SHALL ser `'self'` y NO SHALL contener `'unsafe-inline'` ni `'unsafe-eval'`
- **AND** el documento del panel NO SHALL contener ningún `<script>` con cuerpo en línea

#### Scenario: La HSTS cubre también el panel
- **GIVEN** el panel desplegado y `KESTO_HSTS_MAX_AGE` configurado
- **WHEN** se pide `GET /` y `GET /api/v1/health`
- **THEN** ambas respuestas SHALL llevar `Strict-Transport-Security: max-age=<n>`

#### Scenario: Sin HSTS configurada no hay cabecera
- **GIVEN** el panel desplegado y `KESTO_HSTS_MAX_AGE` sin configurar
- **WHEN** se pide `GET /`
- **THEN** la respuesta NO SHALL llevar `Strict-Transport-Security`
