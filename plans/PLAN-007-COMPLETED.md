# Plan 007 — Panel web (`ui-panel`)

> **Estado: COMPLETED** — cerrado el **2026-11-12**. Issue [#15](https://github.com/atareao/kesto/issues/15). Change OpenSpec `ui-panel`, con delta de `api-shell`. Verificado: backend **425 pruebas de la suite + 5 de `tests/arranque.rs`** (`just check` en verde, clippy sin avisos) y panel **24 ficheros / 121 pruebas** (`just ui-check` en verde, `tsc --noEmit` y `eslint` limpios, `vite build` OK). Documento de la feature: `docs/FEATURE-007-ui-panel.md`. El `Dockerfile` y el `compose` del §8 quedan para el change siguiente.

- **Issue:** #15
- **SRS:** `docs/DEFINITION.md` §2 (frontend), §5 (API/SSE), §6 (pantallas), §7 (estructura)
- **Change OpenSpec:** `openspec/changes/ui-panel/`

## Objetivo

Construir `ui/`, el panel React + TypeScript + Ant Design que falta del SRS: dashboard con métricas y progreso en vivo por SSE, gestión de tareas con formularios, histórico de ejecuciones con consola de eventos y explorador de snapshots, servido sobre la API `/api/v1/*` que ya existe y servido en producción por el propio binario.

## Decisiones de partida (fase 0 las confirma)

- **Stack:** Vite + React 19 + TypeScript + Ant Design v5 + TanStack Query v5 + `@ant-design/icons` + `EventSource`.
- **Gestor de paquetes:** `pnpm` (lockfile `pnpm-lock.yaml`; el CI elige el gestor por el lockfile que encuentre en `ui/`).
- **Pruebas:** `vitest` + `@testing-library/react` + `@testing-library/user-event` + `jsdom`; lint con ESLint y tipos con `tsc --noEmit`.
- **Dev:** Vite en su puerto con proxy de `/api` a `127.0.0.1:8080`; sin CORS.
- **Producción:** Axum sirve `ui/dist` (assets + fallback SPA) detrás del gate de sesión; mismo origen, sin CORS.
- **Contratos:** la envoltura JSON `{status, message, data}`, fechas RFC3339, identificadores UUID con guiones, y el payload SSE de 7 campos del §5.

## Contratos de la API que consume

- `GET/POST /api/v1/targets`, `GET/PUT/DELETE /api/v1/targets/:id`
- `GET/POST /api/v1/repositories`, `GET/PUT/DELETE /api/v1/repositories/:id`
- `GET/POST /api/v1/jobs`, `GET/PUT/DELETE /api/v1/jobs/:id`, `POST /api/v1/jobs/:id/run`
- `GET /api/v1/executions` (filtros, paginación acotada, orden estable)
- `GET /api/v1/snapshots?repository_id=<id>`
- `GET /api/v1/events` (SSE global, `event: execution_progress`)
- `GET /api/v1/auth/session|login|logout|callback`, `GET /api/v1/health` (modo `disabled` en dev)

## Fases y módulos

### Fase 0 — Preparación y contratos
- Fijar versiones, estructura de `ui/`, y el mapa de tipos de la API (leer los DTO de `src/api/`).
- Decidir el servido estático en Axum (fallback SPA) y el modo dev con proxy.

### Fase 1 — Andamiaje y CI
- Crear `ui/` (Vite + React + TS + Antd), `package.json`, `vite.config.ts`, ESLint, `tsconfig`.
- Cablear `just ui-install`, `ui-typecheck`, `ui-lint`, `ui-test`, `ui-build` y `ui-check` en el `.justfile` (ya existen; solo deben pasar a verdes).
- Pruebas RED del layout y de un servicio trivial.

### Fase 2 — Servicios
- Cliente HTTP tipado (envoltura, errores, RFC3339) y modelos de `targets`, `repositories`, `jobs`, `executions`, `snapshots`.
- Hooks de TanStack Query por recurso.
- Hook SSE `useEventStream` con reconexión y estado en vivo.

### Fase 3 — Layout, rutas y sesión
- `Layout` con menú lateral (Dashboard, Tareas, Histórico, Snapshots, Ajustes).
- Router, pantalla de sesión (redirección OIDC o modo dev) y cierre de sesión.

### Fase 4 — Pantallas
- **Dashboard:** `Statistic`/`Card` (espacio en S3, repositorios, tareas activas), `Progress` en vivo por SSE y `Table` de tareas con `Badge` por estado.
- **Tareas:** alta/edición en `Modal`/`Form` con `Select` de contenedores, generador cron (6 campos, con segundos) y validación, `InputNumber` de retención y `Input.Password` de credenciales; disparo manual.
- **Histórico:** `Timeline`/`List` de ejecuciones con fechas, duración y bytes; consola de eventos en vivo.
- **Snapshots:** `Table` por repositorio (id, etiquetas, fecha, tamaño).

### Fase 5 — Servido estático en el backend
- Axum sirve `ui/dist` con fallback SPA, tras el gate de sesión; ruta raíz y assets.

### Fase 6 — Cierre
- `docs/FEATURE-007-ui-panel.md`, archivar el change, renombrar el plan a `PLAN-007-COMPLETED.md`.
- `just check-all` en verde, incluido `ui-check` (el panel ya existe).

## Riesgos y cabos

- Reconexión SSE y eventos perdidos entre reconexiones.
- Autenticación en dev (`disabled`) frente a OIDC en producción; cookies `HttpOnly` en mismo origen.
- Tamaño del bundle y `ui-build` dentro del tiempo de CI.
- Dependencias de terceros (Antd, Vite) que el `Cargo.lock` no cubre: su lockfile es `pnpm-lock.yaml`.

---

Fin del contenido.