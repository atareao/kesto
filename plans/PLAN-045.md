# PLAN-045 — Endurecimiento de cabeceras y del panel (SEC-12 + SEC-18)

- **Issue**: #168 (bloque F de la auditoría #156), hallazgos SEC-12 y SEC-18.
- **Change OpenSpec**: `openspec/changes/endurecimiento-cabeceras-y-panel/`.
- **Documento de la feature**: `docs/FEATURE-045-endurecimiento-cabeceras-y-panel.md`.
- **Rama**: `feature/endurecimiento-cabeceras-y-panel` desde `development`.

## Objetivo

Cerrar dos hallazgos de la misma superficie —el origen HTTP que sirve panel y API— sin romper el panel:

1. **SEC-12** — la HSTS debe cubrir el origen **entero**, no solo la mitad de la API.
2. **SEC-18** — la CSP debe dejar de ser solo `frame-ancestors 'none'` y pasar a ser una política completa, compatible con un SPA que hoy usa un script en línea y estilos en tiempo de ejecución.

## Alcance

- `src/panel.rs` — la capa `con_cabeceras_de_seguridad` y sus tests.
- `src/api/mod.rs` — se retira la capa de HSTS de `api_router` y sus dos tests.
- `ui/index.html` y `ui/public/tema-inicial.js` — el arranque de tema sale del documento.
- Specs: `api-shell` (requisito de cabeceras, MODIFIED) y `ui` (documento bajo CSP, ADDED).

Fuera de alcance: autenticación (SEC-16, SEC-17), `container_name` y el resto de SEC-22, `error_message` (SEC-15) y la superficie de la API.

## Decisiones clave

- La HSTS se mueve a `con_cabeceras_de_seguridad` (origen compuesto), no se duplica.
- El script de arranque de tema se externaliza en vez de añadir `'unsafe-inline'` o un hash a `script-src`.
- `style-src 'self' 'unsafe-inline'` se conserva y se documenta: antd v5 inyecta estilos en tiempo de ejecución.
- Sin dependencias nuevas.

## Tareas

### 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check`.
- [x] 0.2 Rama desde `development`: `just gf-feature endurecimiento-cabeceras-y-panel`.
- [x] 0.3 Change aprobado por el usuario.

### 1. La HSTS cubre el origen entero (SEC-12)

- [x] 1.1 RED — test en `src/panel.rs`: con `KESTO_HSTS_MAX_AGE` configurado, `GET /` y `GET /api/v1/health` llevan `Strict-Transport-Security: max-age=<n>`.
- [x] 1.2 RED — test en `src/panel.rs`: sin `KESTO_HSTS_MAX_AGE`, ninguna respuesta lleva la cabecera.
- [x] 1.3 GREEN — `con_cabeceras_de_seguridad` recibe el `Option<HeaderValue>` de la HSTS y añade la capa condicional.
- [x] 1.4 GREEN — `app` calcula el valor desde `state.config.hsts_max_age` y lo pasa.
- [x] 1.5 GREEN — `src/api/mod.rs`: se retira la capa de HSTS de `api_router` y sus dos tests.
- [x] 1.6 REFACTOR — doc de `con_cabeceras_de_seguridad` explicando por qué la HSTS vive ahí.

### 2. La CSP se completa (SEC-18)

- [x] 2.1 RED — test en `src/panel.rs`: la CSP contiene `default-src 'self'`, `script-src 'self'`, `style-src 'self' 'unsafe-inline'` y `frame-ancestors 'none'`.
- [x] 2.2 RED — test: `script-src` no contiene `'unsafe-inline'` ni `'unsafe-eval'`.
- [x] 2.3 RED — test en `ui`: el documento compilado no lleva `<script>` con cuerpo en línea.
- [x] 2.4 GREEN — `ui/public/tema-inicial.js`: se extrae el arranque de tema.
- [x] 2.5 GREEN — `ui/index.html`: `<script src="/tema-inicial.js"></script>` síncrono en el `<head>`.
- [x] 2.6 GREEN — `src/panel.rs`: la CSP pasa a la política completa.
- [x] 2.7 REFACTOR — doc de la CSP: límites del panel y porqué del `'unsafe-inline'` de estilos.

### 3. Verificación

- [x] 3.1 `just check`.
- [x] 3.2 `just ui-check`.
- [x] 3.3 `just check-all`.
- [x] 3.4 Comprobación manual: el panel compilado carga bajo la CSP del servidor sin errores de política en consola.

### 4. Documentación y cierre

- [x] 4.1 `docs/FEATURE-045-endurecimiento-cabeceras-y-panel.md`.
- [x] 4.2 `docs/ARQUITECTURA.md` (cabeceras y CSP del origen compuesto).
- [ ] 4.3 `openspec archive endurecimiento-cabeceras-y-panel`.
- [ ] 4.4 Renombrar este plan a `plans/PLAN-045-COMPLETED.md`.

## Criterios de aceptación

- Toda respuesta del origen —panel incluido— lleva la HSTS cuando `KESTO_HSTS_MAX_AGE` está configurado.
- La CSP incluye `default-src 'self'` y `script-src 'self'` sin `'unsafe-inline'` ni `'unsafe-eval'`.
- El documento del panel no lleva JavaScript en línea y el tema se aplica antes del primer pintado.
- El panel se ve y funciona igual que antes bajo la política nueva.
- `just check-all` en verde.
