# FEATURE-045 — Endurecimiento de cabeceras y del panel (#168)

- **Issue:** #168 (SEC-12 + SEC-18), hijo de la auditoría de seguridad **#156** (bloque F). **Change OpenSpec:** `endurecimiento-cabeceras-y-panel`. **Plan:** `plans/PLAN-045.md`.
- **Tipo:** cabeceras HTTP + panel. **No** toca el motor, el esquema de la base ni el contrato de `/api/v1`.

## Qué es

El cierre de dos hallazgos de la misma superficie —el origen HTTP que sirve el panel y la API— en la capa que ya envolvía el router compuesto (`con_cabeceras_de_seguridad`, `src/panel.rs`):

1. **SEC-12** — `Strict-Transport-Security` se declaraba **dentro** de `api_router`, y el panel se cuelga como *fallback* **después** de esa capa, así que el documento, los assets y las rutas del cliente salían sin HSTS. Ahora se declara en la capa del origen compuesto y cubre las dos mitades.
2. **SEC-18** — la `Content-Security-Policy` era solo `frame-ancestors 'none'`. Ahora es una política completa con `default-src 'self'`, `script-src 'self'`, `style-src 'self' 'unsafe-inline'`, etc. Para que `script-src 'self'` no rompiera el SPA, el `<script>` en línea de arranque de tema salió de `ui/index.html` a un fichero propio.

## Causa / motivo

- **SEC-12:** la spec `deploy` ya exigía que «toda respuesta» llevara la HSTS, pero la implementación la aplicaba solo a la mitad de la API. Una capa de `tower` colgada de un router cubre las rutas **y el *fallback* presentes en ese momento**; añadir un *fallback* después no queda cubierto por ella. El panel se añade después, así que quedaba fuera.
- **SEC-18:** sin `default-src`/`script-src`, una inyección de script se ejecuta como si no hubiera política. El change original lo había aparcado porque una `default-src` «inventada» rompía el panel; el motivo real es que el documento llevaba un `<script>` en línea (arranque de tema, para evitar el destello) y antd v5 inyecta estilos en tiempo de ejecución (`@ant-design/cssinjs`). El CSS empaquetado es de ~1,7 KB (solo estilos propios): los de antd no están en el bundle.

## Ficheros implicados

- `src/panel.rs` — `app` calcula el valor de la HSTS desde `state.config.hsts_max_age`; `con_cabeceras_de_seguridad(router, hsts)` añade la capa condicional y declara la CSP completa. Doc y tests nuevos.
- `src/api/mod.rs` — se retira de `api_router` la capa de HSTS, su cálculo y sus dos tests (se trasladan a `panel.rs`).
- `ui/index.html` — el `<script>` en línea se sustituye por `<script src="/tema-inicial.js"></script>` síncrono en el `<head>`.
- `ui/public/tema-inicial.js` (nuevo) — el arranque de tema, idéntico al IIFE que estaba en línea.
- `ui/tests/documentoSinScriptsEnLinea.test.ts` (nuevo) — guardia del documento y del fichero.
- `docs/ARQUITECTURA.md`, `docs/FEATURE-007-ui-panel.md`, `docs/FEATURE-016-identidad-visual-y-tema.md`.
- `openspec/changes/endurecimiento-cabeceras-y-panel/` · `plans/PLAN-045.md`.

## Cómo funciona

### La política de cabeceras (cinco capas)

`con_cabeceras_de_seguridad` envuelve el **router completo** —las dos mitades— con cinco capas `SetResponseHeaderLayer::overriding`:

- `X-Content-Type-Options: nosniff`
- `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'`
- `X-Frame-Options: DENY`
- `Referrer-Policy: no-referrer`
- `Strict-Transport-Security: max-age=<n>` — **solo** si `KESTO_HSTS_MAX_AGE` está configurado.

La HSTS se declara aquí y no en `api_router` porque el origen es uno solo: describir la superficie entera —el panel y la API— es lo que evita que una mitad salga sin la orden.

### Por qué `style-src` lleva `'unsafe-inline'` y `script-src` no

- `style-src`: antd v5 inyecta estilos en tiempo de ejecución y los componentes usan atributos `style`. Sin `'unsafe-inline'` el panel se queda sin estilos. Es la única excepción de la política y es deliberada.
- `script-src`: no hay ninguna excepción posible. El documento no lleva JavaScript en línea; el arranque de tema vive en `ui/public/tema-inicial.js`, servido por el mismo origen y cargado de forma síncrona en el `<head>` (antes del primer pintado, sin destello).

## Cómo verificar

- `cargo test --locked --bin kesto -- 'panel::'` — `la_hsts_cubre_tambien_el_panel`, `sin_hsts_no_hay_cabecera`, `la_csp_es_una_politica_completa` (igualdad exacta con la política) y `la_csp_no_permite_scripts_en_linea`.
- `cd ui && pnpm exec vitest run documentoSinScriptsEnLinea` — el documento no lleva scripts en línea y el fichero de arranque conserva su contenido.
- `just check-all` — el análogo local del CI.

## Variables / configuración

- `KESTO_HSTS_MAX_AGE` — sin cambios de semántica; su valor ahora llega a **toda** respuesta del origen (panel incluido). Sin configurar, ninguna cabecera.

## Cómo habilitar / desplegar

No hay pasos nuevos. La HSTS se enciende con `KESTO_HSTS_MAX_AGE` como antes; los despliegues que ya la tuvieran ahora la ven también en el panel.

## Decisiones

- **La HSTS se mueve, no se duplica.** Alternativa descartada: añadir otra capa en `panel::app` (dos sitios decidiendo el mismo valor). Detalle en `openspec/changes/endurecimiento-cabeceras-y-panel/design.md`.
- **El arranque de tema se externaliza.** Alternativas descartadas: `script-src 'unsafe-inline'` (anula media política) y el hash del script en línea mantenido a mano en Rust (frágil y silencioso).
- **`style-src 'unsafe-inline'` se acepta y se documenta**, porque el panel depende de estilos en tiempo de ejecución.

## Follow-ups

- El test de UI lee el **fuente** `ui/index.html`, no el `dist` compilado; el artefacto sigue cubierto solo por la comprobación manual.
- `ui/public/tema-inicial.js` queda fuera de `eslint` (que solo mira `*.{ts,tsx}`) y de `tsc`: es el único JS escrito a mano del repo y nada lo vigila.
