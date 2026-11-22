# Design — endurecimiento de cabeceras y del panel

## Contexto

El origen HTTP de Kesto sirve, desde el mismo router, la API (`/api/v1`) y el panel (su *fallback*). `panel::app` (`src/panel.rs:61`) compone `api_router(state).fallback(panel)` y envuelve el resultado en `con_cabeceras_de_seguridad` (`:104`). Dentro de `api_router`, y **antes** de esa composición, se declara hoy la HSTS (`src/api/mod.rs:241-245`).

## SEC-12 — dónde va la HSTS

### Alternativas

1. **Dejar la HSTS en `api_router` y añadir otra capa en `panel::app`.** Duplicaría la declaración y el valor se decidiría dos veces. `SetResponseHeaderLayer::overriding` evita la cabecera repetida, pero la política viviría en dos sitios. Descartada.
2. **Moverla a `con_cabeceras_de_seguridad`.** La HSTS pasa a ser una cabecera del origen, como las otras cuatro, y la decisión de su valor —`KESTO_HSTS_MAX_AGE`— vive con el resto. `api_router` deja de conocerla. **Elegida.**
3. **Dejarla en `api_router` y no hacer nada.** Incumple la spec de `deploy` («toda respuesta») y deja el panel sin HSTS. Descartada.

### Decisión

La capa de `Strict-Transport-Security` se mueve de `api_router` a `con_cabeceras_de_seguridad`. El valor se calcula una sola vez, al componer, a partir de `state.config.hsts_max_age`. Los tests de HSTS de `api-shell` (`src/api/mod.rs:566`, `:577`) se re-homed a `src/panel.rs` (nivel `app`). `api_router` conserva el resto de su contrato.

## SEC-18 — la política de contenido

### Alternativas

1. **`default-src 'self'` a secas.** Rompe el panel: el `<script>` de arranque de tema es en línea y antd inyecta estilos en tiempo de ejecución. Es la razón por la que el change original la aparcó. Descartada.
2. **`default-src 'self'` + `'unsafe-inline'` en `script-src`.** Deja el script en línea funcionando, pero anula media política: `'unsafe-inline'` en `script-src` es precisamente lo que una CSP debe cerrar. Descartada.
3. **`default-src 'self'` + hash del script en línea en `script-src`.** Requiere mantener a mano en la cabecera Rust el hash del script en línea: cualquier cambio del script rompería la política en silencio. Descartada.
4. **Externalizar el script de arranque y aceptar `style-src 'unsafe-inline'`.** `script-src 'self'` sin excepciones, y `style-src 'unsafe-inline'` para los estilos de antd. **Elegida.**

### Decisión

El `<script>` de arranque de tema de `ui/index.html` se mueve a `ui/public/tema-inicial.js` y se carga con un `<script src="/tema-inicial.js"></script>` síncrono en el `<head>`. Un script clásico síncrono en el `<head>` corre antes del primer pintado, así que no hay destello de tema.

La política final:

| directiva | valor | porqué |
|---|---|---|
| `default-src` | `'self'` | cierra lo no declarado |
| `script-src` | `'self'` | el único JS es el del bundle; ya no hay script en línea |
| `style-src` | `'self' 'unsafe-inline'` | antd v5 inyecta `<style>` en tiempo de ejecución y los componentes usan atributos `style` |
| `img-src` | `'self' data:` | favicons propios y cualquier `data:` que antd emita |
| `connect-src` | `'self'` | la API y el stream SSE son del mismo origen |
| `object-src` | `'none'` | no hay plugins |
| `base-uri` | `'self'` | evita el secuestro de `<base>` |
| `form-action` | `'self'` | los formularios no salen del origen |
| `frame-ancestors` | `'none'` | se conserva: no enmarcar |

La `Content-Security-Policy` se sirve como cabecera (no como `<meta>`) desde `con_cabeceras_de_seguridad`, una sola vez para todo el origen.

### Riesgo verificado

El bundle de producción (`ui/dist/index.html`) hoy solo lleva **un** script en línea: el de tema. No hay polyfill `modulepreload` en línea ni `eval` en el bundle. El CSS de antd no está en el paquete (`ui/dist/assets/index-*.css` es de ~1,7 KB, solo estilos propios): antd lo inyecta en tiempo de ejecución, de ahí el `'unsafe-inline'` de estilos. Verificado antes de fijar la política.

## Solapes

- SEC-15 (`error_message`) y el resto de SEC-22 son de otros changes de F.
- No se toca la autenticación (SEC-16, SEC-17) ni `container_name` (SEC-22).
