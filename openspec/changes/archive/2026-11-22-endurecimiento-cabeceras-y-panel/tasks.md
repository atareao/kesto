# Tasks — endurecimiento-cabeceras-y-panel

> SEC-12 y SEC-18 del bloque F, hijos de la auditoría #156 (#168).
> Change de producto: cabeceras de seguridad del origen compuesto y el documento del panel.
> Decisiones y alternativas descartadas, en `design.md`.
> TDD: primero los tests que hoy fallan (rojo), luego el mínimo (verde), luego el refactor.
> Sin dependencias nuevas.
> Plan: `plans/PLAN-045.md`. Documento de la feature: `docs/FEATURE-045-endurecimiento-cabeceras-y-panel.md`.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Rama desde `development`: `just gf-feature endurecimiento-cabeceras-y-panel` (verificación: `git branch --show-current`).
- [x] 0.3 Change aprobado por el usuario (verificación: aprobación explícita y `just check-spec` encuentra el change activo).

## 1. La HSTS cubre el origen entero (SEC-12)

### RED

- [x] 1.1 Rust: test en `src/panel.rs` — `app` con `KESTO_HSTS_MAX_AGE` configurado — `GET /` y `GET /api/v1/health` llevan `Strict-Transport-Security: max-age=<n>` (verificación: rojo hoy, que `GET /` no la lleva).
- [x] 1.2 Rust: test en `src/panel.rs` — `app` sin `KESTO_HSTS_MAX_AGE` — ninguna respuesta lleva la cabecera (verificación: verde hoy; se conserva como contrapeso).

### GREEN

- [x] 1.3 `src/panel.rs`: `con_cabeceras_de_seguridad` recibe el `Option<HeaderValue>` de la HSTS y añade la capa condicional (verificación: 1.1 en verde).
- [x] 1.4 `src/panel.rs`: `app` calcula el valor desde `state.config.hsts_max_age` y lo pasa (verificación: 1.1, 1.2 en verde).
- [x] 1.5 `src/api/mod.rs`: se retira la capa de HSTS de `api_router` y sus dos tests de HSTS (verificación: `cargo test --locked --bin kesto 'api::'` en verde).

### REFACTOR

- [x] 1.6 `src/panel.rs`: la doc de `con_cabeceras_de_seguridad` explica por qué la HSTS vive aquí y no en `api_router` (verificación: `cargo doc` sin warning).

## 2. La CSP se completa (SEC-18)

### RED

- [x] 2.1 Rust: test en `src/panel.rs` — la CSP de una respuesta contiene `default-src 'self'`, `script-src 'self'`, `style-src 'self' 'unsafe-inline'` y `frame-ancestors 'none'` (verificación: rojo hoy, que solo trae `frame-ancestors 'none'`).
- [x] 2.2 Rust: test — `script-src` no contiene `'unsafe-inline'` ni `'unsafe-eval'` (verificación: rojo hasta 2.1).
- [x] 2.3 UI: test en `ui` — el documento compilado no lleva `<script>` con cuerpo en línea (verificación: rojo hoy, que lleva el arranque de tema).

### GREEN

- [x] 2.4 `ui/public/tema-inicial.js`: se extrae el arranque de tema desde `ui/index.html` (verificación: 2.3 en verde).
- [x] 2.5 `ui/index.html`: el `<script>` en línea se sustituye por `<script src="/tema-inicial.js"></script>` en el `<head>` (verificación: 2.3 en verde).
- [x] 2.6 `src/panel.rs`: la CSP de `con_cabeceras_de_seguridad` pasa a la política completa (verificación: 2.1, 2.2 en verde).

### REFACTOR

- [x] 2.7 `src/panel.rs`: la doc de la CSP explica los límites del panel y por qué `style-src` lleva `'unsafe-inline'` (verificación: lectura).

## 3. Verificación de la feature

- [x] 3.1 `just check` (verificación: fmt + clippy + test en verde).
- [x] 3.2 `just ui-check` (verificación: typecheck + lint + test + build de `ui/` en verde).
- [x] 3.3 `just check-all` (verificación: el análogo local del CI en verde).
- [x] 3.4 Comprobación manual: el panel compilado (`ui/dist`) carga bajo la CSP del servidor, sin errores de política en consola (verificación: arranque con `podman` y navegación por las cinco pantallas).

## 4. Documentación y cierre

- [x] 4.1 `docs/FEATURE-045-endurecimiento-cabeceras-y-panel.md` con el porqué y las decisiones.
- [x] 4.2 Actualizar `docs/ARQUITECTURA.md` (cabeceras y CSP del origen compuesto).
- [x] 4.3 `openspec archive endurecimiento-cabeceras-y-panel`.
- [x] 4.4 Renombrar `plans/PLAN-045.md` a `plans/PLAN-045-COMPLETED.md`.
