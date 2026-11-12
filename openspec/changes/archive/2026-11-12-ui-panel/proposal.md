# Proposal

## Why

La API del §5 está entregada y probada, pero no existe `ui/`: hoy no hay forma de dar de alta una tarea, ver una copia en marcha ni explorar un repositorio sin usar `curl` o `sqlite3`. El panel es la mitad del producto que convierte ese motor en algo que un operador puede usar, y su ausencia bloquea la imagen de producción —`release.yml` se salta la publicación mientras no existan `Dockerfile` y `ui/package.json`—.

Issue de entrada: [#15](https://github.com/atareao/kesto/issues/15).

## What Changes

- Se crea `ui/`, un SPA con Vite, React 19, TypeScript, Ant Design v5 y TanStack Query, con `pnpm` como gestor de paquetes.
- Se añade un **cliente de la API** que consume la envoltura `{status, message, data}` de `api-shell`, interpreta las fechas RFC3339 y los UUID con guiones, y presenta los errores sin filtrar secretos.
- Se añade un **listener SSE** sobre `GET /api/v1/events` que lee el evento `execution_progress` con los siete campos del §5 y se reconecta cuando la conexión se cae.
- Se implementan las pantallas del §6: Dashboard, Tareas, Histórico, Snapshots, Ajustes y sesión, con menú lateral, estados de carga/error/vacío y criterios de accesibilidad.
- El backend aprende a servir `ui/dist` en el mismo origen, con fallback a `index.html` para las rutas del cliente, sin interceptar `/api/v1/*` ni relajar la envoltura de la API.
- El job `frontend` del CI, hoy dormido, se enciende sin cambios porque detecta `ui/package.json` y el lockfile de `pnpm`.
- **No** se añaden endpoints nuevos: el panel consume los que existen. En particular, no hay ruta que liste contenedores, así que el campo `container_name` del formulario se resuelve con las etiquetas ya usadas por los targets y admite texto libre.

## Capabilities

### New Capabilities

- `ui`: el panel web que consume la API y el stream de eventos —layout, rutas, cliente HTTP, SSE, las cinco pantallas del §6, sesión, accesibilidad y estados de carga, error y vacío—.

### Modified Capabilities

- `api-shell`: se añade el servido estático del panel compilado en el mismo origen que la API, con fallback SPA, y se fija que esa superficie no altera la envoltura ni el `404`/`405` de `/api/v1`.

## Impact

- **Código nuevo:** `ui/` completo (`package.json`, `pnpm-lock.yaml`, `vite.config.ts`, `tsconfig*.json`, `index.html`, `src/{components,pages,services,hooks}` y pruebas colocadas).
- **Código modificado:** `src/main.rs` o `src/api/mod.rs` para componer el servido estático con el router existente; `Cargo.toml` para la dependencia de ficheros estáticos de `tower-http`; `src/config.rs` si la ruta de `ui/dist` pasa a ser configurable (con un defecto que no rompe el arranque sin panel).
- **API:** sin rutas nuevas. Se consumen `GET /api/v1/health`, `/auth/session`, `/auth/logout`, `/targets`, `/repositories`, `/jobs`, `/jobs/:id/run`, `/executions`, `/snapshots?repository_id=` y `/events`.
- **Dependencias del panel:** React 19, React Router, Ant Design v5, `@ant-design/icons`, TanStack Query, Vite, Vitest, Testing Library y ESLint.
- **CI:** el job `frontend` de `ci.yml` empieza a ejecutar `pnpm install --frozen-lockfile`, lint, `tsc --noEmit`, `vitest run` y build. No se modifica el workflow.
- **Despliegue:** `just ui-build` genera `ui/dist`; el binario lo sirve desde el mismo origen. El `Dockerfile` multi-stage y la publicación de imagen quedan **fuera** de este change (issue aparte), aunque el backend ya deja el hueco preparado.
- **Fuera de alcance:** planificador en caliente, purga manual de snapshots desde la UI (la API no la expone) y gestión de usuarios o permisos.