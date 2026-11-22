# Proposal — endurecimiento de cabeceras y del panel

Issues **SEC-12** y **SEC-18** del bloque **F** de la auditoría de seguridad **#156** (#168).

## Why

Los dos hallazgos viven en la misma superficie —el origen HTTP que sirve el panel y la API— y se cierran en la misma capa, `con_cabeceras_de_seguridad` (`src/panel.rs`), que envuelve el router compuesto.

1. **SEC-12 (HSTS a medias)** — `Strict-Transport-Security` se declara hoy dentro de `api_router` (`src/api/mod.rs:241-245`), no en la capa que envuelve el origen entero. El panel se cuelga como *fallback* **después** de esa capa (`src/panel.rs:64-70`), así que su documento, sus assets y las rutas del cliente se sirven **sin** HSTS: el navegador visita el panel por HTTPS y nunca recibe la orden de no volver por HTTP. La cabecera existe y se declara una sola vez; lo que falta es que cubra las dos mitades del origen. La spec de `deploy` ya dice «toda respuesta SHALL llevar» (`openspec/specs/deploy/spec.md:225`), así que la implementación incumple la spec: no hay que cambiar el contrato, sino el sitio donde se aplica la cabecera.
2. **SEC-18 (CSP incompleta)** — la `Content-Security-Policy` es solo `frame-ancestors 'none'` (`src/panel.rs:110-112`). No hay `default-src` ni `script-src`, así que un script inyectado se ejecuta como si no hubiera política. El motivo de la renuncia —«una `default-src` inventada aquí rompería el panel»— es real: `ui/index.html` lleva un `<script>` en línea (el arranque de tema) y antd v5 inyecta estilos en tiempo de ejecución. La política hay que escribirla **con** esas dos servidumbres resueltas, no alrededor de ellas.

## What Changes

### La HSTS cubre el origen entero (SEC-12)

- La capa de `Strict-Transport-Security` pasa de `api_router` (`src/api/mod.rs:241-245`) a `con_cabeceras_de_seguridad` (`src/panel.rs`), que envuelve el router compuesto API + panel.
- La condición no cambia: `KESTO_HSTS_MAX_AGE` ausente, ninguna cabecera; presente, `max-age=<n>` en toda respuesta del origen.
- `api_router` deja de declarar la HSTS; sus tests de HSTS de `api-shell` pasan al nivel de `panel::app`, que es donde vive la cabecera.

### La CSP se completa sin romper el panel (SEC-18)

- `con_cabeceras_de_seguridad` declara una `Content-Security-Policy` real: `default-src 'self'`, `script-src 'self'`, `style-src 'self' 'unsafe-inline'`, `img-src 'self' data:`, `connect-src 'self'`, `object-src 'none'`, `base-uri 'self'`, `form-action 'self'`, `frame-ancestors 'none'`.
- El `<script>` de arranque de tema de `ui/index.html` se mueve a un fichero estático propio (`ui/public/tema-inicial.js`) cargado en el `<head>` de forma síncrona: así `script-src 'self'` no necesita `'unsafe-inline'` ni un hash que mantener.
- `style-src 'unsafe-inline'` se conserva y se documenta: antd v5 (`@ant-design/cssinjs`) inyecta estilos en tiempo de ejecución y los componentes usan atributos `style`. Sin `'unsafe-inline'` el panel se queda sin estilos.

### Documentación

- `docs/ARQUITECTURA.md` describe la política de cabeceras del origen compuesto.
- `docs/FEATURE-045-endurecimiento-cabeceras-y-panel.md` recoge el porqué y las decisiones.

## Non-goals

- No se construye un modelo de roles ni se toca la autenticación (SEC-16 y SEC-17 son de otro change).
- No se añade `nonce` ni hash para estilos: el panel depende de estilos en tiempo de ejecución y la política se queda en `'unsafe-inline'` para estilos, con el motivo escrito.
- No se toca la superficie de la API: ni forma de respuesta, ni códigos, ni rutas.
- No se cambia el contrato de `api_router` más allá de retirarle la HSTS.

## New Capabilities

_(ninguna)_

## Modified Capabilities

- `api-shell`: la HSTS se declara en la capa del origen compuesto, no en `api_router`, y cubre también la superficie del panel; la CSP deja de ser solo `frame-ancestors` y pasa a ser una política completa.
- `ui`: el documento del panel no lleva JavaScript en línea; el arranque de tema pasa a un asset externo.

## Impact

- **Código**: `src/panel.rs` (la capa de cabeceras y sus tests), `src/api/mod.rs` (se retira la HSTS y sus tests).
- **UI**: `ui/index.html` (se retira el `<script>` en línea), `ui/public/tema-inicial.js` (nuevo asset del arranque de tema).
- **API**: ninguna ruta cambia de forma; las respuestas ganan una CSP completa y el panel gana HSTS.
- **Despliegue**: sin variables nuevas. `KESTO_HSTS_MAX_AGE` conserva su semántica.
- **Dependencias**: ninguna nueva.
