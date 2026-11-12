# FEATURE-007 — ui-panel

> Implementación del change proposal `ui-panel`, en `openspec/changes/ui-panel/` — una capability **nueva** (`ui`) con su `design.md`. Issue [#15](https://github.com/atareao/kesto/issues/15). Plan: `plans/PLAN-007.md` (COMPLETED). Rama: `feature/ui-panel`, desde `development`.
> Seis fases: andamiaje y CI, servicios, layout/rutas/sesión, pantallas, servido estático y cierre. Especificación de origen: `docs/DEFINITION.md` §2 (frontend), §5 (API/SSE), §6 (pantallas) y §7 (estructura).
> Estado: **implementado, verde y verificado** — backend **425 pruebas de la suite + 5 de `tests/arranque.rs`**, panel **24 ficheros / 121 pruebas**, `tsc --noEmit` y `eslint` limpios, `vite build` OK; `openspec validate ui-panel --strict` OK. `ui/` era el último frontal del SRS sin existir.

**Kesto ya tiene panel.** Hasta este change la única forma de usar el producto era `curl` contra `/api/v1/*`: el SRS pedía un frontal React desde el primer día y `ui/` era, con `Dockerfile`, las dos piezas que el layout del §7 declaraba y nadie había escrito. Ahora un operador entra con el navegador, ve el dashboard con el progreso en vivo por SSE, da de alta y edita tareas con sus credenciales, consulta el histórico con su consola de eventos, explora los snapshots y ajusta targets y repositorios — con el mismo binario Axum sirviendo el panel compilado en el mismo origen que la API.

## Qué es

`ui/` es la aplicación de una sola página que consume la API que ya existía. El stack es el que fijó la fase 0 del plan: **Vite + React 19 + TypeScript + Ant Design v5** (`antd`, `@ant-design/icons`), **TanStack Query v5** para el estado de servidor, **React Router v7** para las rutas y **`EventSource`** para el stream SSE global. No toca ni una línea del backend salvo el módulo nuevo `src/panel.rs`, que monta el panel compilado como *fallback* del router de la API.

Las pantallas del §6, una a una:

- **Dashboard** — tres tarjetas (`Statistic`/`Card`) con repositorios, tareas activas y espacio sumado en S3 (con el rótulo «al menos» cuando el listado viene recortado, `truncated`), un `Progress` en vivo que avanza con el SSE y la tabla de tareas con `Badge` por estado.
- **Tareas** — listado y `Modal`/`Form` de alta y edición: `Select` de target y repositorio, generador cron de **seis** campos (con segundos) y edición directa de la expresión, tres `InputNumber` de retención no negativos y `Input.Password` de credenciales que no viajan vacíos; disparo manual con sus tres desenlaces (`202`/`409`/`404`).
- **Histórico** — `Timeline`/`List` de ejecuciones con paginación `limit`/`offset`, filtros por `job_id` y `status`, fechas en hora local, duración y bytes, y una **consola de eventos en vivo** que solo muestra los de la ejecución seleccionada.
- **Snapshots** — `Table` por repositorio (identificador, etiqueta, fecha local, ficheros y bytes), aviso de `truncated`, estado vacío y **sin** botón de purga.
- **Ajustes** — versión y `auth_mode` desde `/health`, listado y CRUD de targets y repositorios, campos `has_*` que no revelan el secreto y edición que conserva el secreto cuando el campo se deja vacío.

## Ficheros implicados

**Frontend (`ui/`)** — el layout que pide el §7, `ui/src/{components,pages,services}`:

| Ruta | Qué es |
|---|---|
| `ui/src/services/api/` · `ui/src/api/` | Cliente HTTP tipado (envoltura `{status, message, data}`, errores, RFC3339) y un módulo por recurso: `targets`, `repositories`, `jobs`, `executions`, `snapshots`. |
| `ui/src/services/events.tsx` · `useEventStream.ts` | El proveedor SSE único y global, con reconexión de espera creciente y una sola conexión compartida por todos los consumidores. |
| `ui/src/services/session.ts` | Sesión: `auth_mode` de `/health`, `GET /auth/session`, `401` como «sin sesión» y `POST /auth/logout` con vaciado de la caché de Query. |
| `ui/src/hooks/` | Un hook de TanStack Query por recurso (`useTareas`, `useRepositorios`, `useSnapshots`, `useEjecuciones`, `useTargets`, `useEspacioEnS3`) y el mapa de claves (`claves.ts`). |
| `ui/src/components/` | `Layout` (menú lateral con las cinco entradas), `SoloConSesion` (la guarda de rutas), `GeneradorCron`, `ModalDeTarea`/`ModalDeTarget`/`ModalDeRepositorio`, `ProgresoEnVivo`, `ConsolaDeEjecucion`, `TablaDeTareasEnCurso`, `TarjetasDeResumen` y los estados `Cargando`/`EstadoDeError`/`EnConstruccion`. |
| `ui/src/pages/` | `dashboard`, `tareas`, `historico`, `snapshots`, `ajustes`, `sesion` y `noEncontrada`, cada uno con su `*.test.tsx`. |
| `ui/vite.config.ts` | Proxy de desarrollo de `/api` a `127.0.0.1:8080` con el *buffering* de SSE desactivado. |
| `ui/pnpm-workspace.yaml` | Configuración de pnpm 12 que permite el único script de instalación necesario (`allowBuilds: { esbuild: true }`). |

**Backend** — solo lo mínimo para servir el panel:

| Fichero | Qué es |
|---|---|
| `src/panel.rs` | **Nuevo.** Compone el servidor: API + panel como *fallback* SPA y la capa de cabeceras de seguridad. Publica `PANEL_POR_DEFECTO` y `app`. |
| `src/main.rs` | `serve()` arranca con `panel::app(state, panel::PANEL_POR_DEFECTO)` en vez del router pelado. |
| `src/api/mod.rs` | `not_found` y `metodo_no_permitido` pasan a `pub(crate)` para que el panel reutilice los mismos `404`/`405` con envoltura. Nada más cambia. |
| `Cargo.toml` | `tower-http` gana la feature `set-header` (la `fs` ya estaba) — sin crate nuevo en el árbol. |
| `.justfile` | Las recetas `ui-install`, `ui-typecheck`, `ui-lint`, `ui-test`, `ui-build` y `ui-check` (más el `ui-dev` del dev). |

## Cómo funciona

### El servido estático (`src/panel.rs`)

`panel::app(state, panel)` monta el router de la API y le cuelga el panel como **fallback**. El panel no declara rutas propias: solo atiende lo que el router de la API **declina**. Eso es lo que protege el contrato de `/api/v1`: lo que el router resuelve —sus rutas, su `405` por método y su `401` por sesión— se atiende antes de llegar aquí, y las pruebas de `api-shell` siguen mirando el mismo objeto.

El fallback resuelve cualquier ruta del cliente con `index.html` (SPA): `GET /tareas` devuelve el documento, no un `404`. El orden de las comprobaciones es el contrato:

1. **La API primero** — un guardia por segmento (`es_de_la_api`) manda `/api` y `/api/*` al `404` con envoltura de la casa: el panel nunca tapa una ruta de la API.
2. **El método** — solo `GET` y `HEAD`; lo demás contesta el `405` con envoltura.
3. **El camino** — `camino_seguro` rechaza `..`, cualquier segmento que empiece por `.` (ficheros ocultos y el propio `.`) y cualquier `\`, **antes** de tocar el sistema de ficheros. La URI cruda **no se decodifica** (`ServeFile(SingleFile)`), así que `%2e%2e%2f` no lleva ni una barra ni un punto de verdad y cae como ruta del cliente.
4. **El sistema de ficheros** — si existe el fichero pedido se sirve con su tipo; si no, se sirve el documento; y si tampoco hay documento (sin `ui/dist`), el `404` con envoltura de la API.

**El documento y sus assets no exigen sesión.** Es deliberado: es lo que pinta la entrada OIDC, así que el gate de sesión vive en el cliente (ver más abajo), no en las rutas del panel. Lo que sí conserva su capa es `/api/*`, que contesta su `401` con envoltura cuando el modo es `oidc`.

**Caché.** Un asset con hash en el nombre va con `Cache-Control: public, max-age=31536000, immutable` (el nombre cambia con el contenido); el documento va con `no-cache` (revalida antes de usarlo). El tipo lo adivina `ServeFile` por la extensión.

### Las cabeceras de seguridad

`con_cabeceras_de_seguridad` envuelve el **router completo** —las dos mitades, API y panel— con cuatro capas `SetResponseHeaderLayer::overriding`:

- `X-Content-Type-Options: nosniff`
- `Content-Security-Policy: frame-ancestors 'none'`
- `X-Frame-Options: DENY`
- `Referrer-Policy: no-referrer`

Van **alrededor** del router compuesto y no colgadas del fallback porque son la política de respuesta de este origen, y este origen sirve dos mitades: una capa solo del fallback dejaría a `/api/v1/health` sin `nosniff` ni prohibición de enmarcado, y las cabeceras de una misma superficie dependerían de qué mitad atendió la petición. Se usan `overriding` (y no `if_not_present`) porque son decisiones de este módulo, no un relleno de último recurso. La capa fija **su** cabecera y ninguna otra: no pisa el `Content-Type` ni el `Cache-Control` de `ServeFile`.

### La sesión y las rutas (cliente)

`SoloConSesion` envuelve todas las rutas **menos** `/sesion`. Su comportamiento por modo:

- **`disabled`** (desarrollo): entra directo y **no** llama a ninguna `/api/v1/auth/*`.
- **`oidc` con sesión:** pinta la ruta pedida.
- **`oidc` sin sesión:** lleva a `/sesion`, que ofrece el botón hacia `/api/v1/auth/login`.

Un manejador global de `401` invalida la caché de sesión sin realimentarse (la relectura que da `401` vuelve a pasar por la guarda y devuelve al usuario a la pantalla de sesión, sin bucle). `useSesion` usa `retryOnMount: false` y `refetchOnMount: false` para no relanzar `/health` y `/auth/session` en cada montaje.

### El SSE

Un **único stream SSE global**, `GET /api/v1/events`, con `event: <nombre>` (`execution_progress`) y el payload literal de 7 campos del §5. `useEventStream` mantiene **una sola** conexión `EventSource` compartida por todos los consumidores del proveedor, con reconexión de espera creciente, y la cierra cuando se desmonta el último consumidor.

## Variables y configuración

**Ninguna variable de entorno nueva.** El panel no lee configuración por entorno: la ruta del panel compilado es la constante `PANEL_POR_DEFECTO = "ui/dist"`, **relativa al directorio de trabajo del proceso** — el despliegue deja `ui/dist` al lado del binario y arranca desde ahí. La ruta entra por parámetro en `panel::app` para que las pruebas usen un directorio temporal sin abrir un puerto ni tocar el entorno.

Para levantar todo en dev siguen bastando las de siempre: `KESTO_MASTER_KEY` (obligatoria siempre), `DATABASE_URL`, `BIND_ADDRESS` y `DOCKER_HOST`. Sin ninguna `OIDC_*` el modo es `Disabled` y `/api/v1/health` contesta `auth_mode: "disabled"`, con lo que el panel entra directo. En modo `oidc` hacen falta las tres `OIDC_*` y `KESTO_SECRET`.

Versiones relevantes del frontal (las fija `ui/package.json`): React **19**, Ant Design **5**, TanStack Query **5**, React Router **7**, Vite **7**, Vitest **5**, TypeScript **5.9**, ESLint **9**.

## Cómo habilitarlo, construirlo y desplegarlo

**En local, verificación del panel:**

```sh
just ui-install   # pnpm install en ui/
just ui-check     # ui-typecheck → ui-lint → ui-test → ui-build (los cuatro pasos del panel)
```

**Desarrollo, con recarga en caliente:**

```sh
just dev          # backend en 127.0.0.1:8080
just ui-dev       # Vite en su puerto, con proxy de /api a 127.0.0.1:8080
```

El proxy de `ui/vite.config.ts` manda `/api` al backend y **desactiva el buffering** de SSE (quita `content-length` y `content-encoding`, y añade `no-cache`/`x-accel-buffering: no`) para que el stream de `/api/v1/events` entregue eventos sin acumularse. No hay CORS: mismo origen a través del proxy.

**Producción:** `just ui-build` deja `ui/dist` (`index.html` + `assets/*`), y el **propio binario Axum sirve el panel** en el mismo origen que la API — sin CORS y sin un segundo servidor. La ruta por defecto es `ui/dist` relativa al CWD, así que hay que arrancar el proceso desde el directorio donde esté ese `ui/dist`.

**⚠️ Aviso:** el `Dockerfile` y el `compose` del §8 **quedan para el change siguiente**. Este change entrega el panel y su servido; el empaquetado en imagen es deuda explícita.

## Decisiones

- **`DELETE` contesta `200` con `data: null`, no `204`.** El panel sigue al backend, que envuelve toda respuesta en `{status, message, data}`; un `204` rompería esa envoltura. El cliente no espera un cuerpo distinto para los borrados.
- **`packageManager` omitido a propósito en `ui/package.json`.** El job `frontend` del CI usa `pnpm/action-setup@v4` pasando `version: 12`, y esa acción **aborta** si el `package.json` declara el campo: la opción declarada choca con la del workflow. Se sugiere, como red de seguridad (follow-up), declarar `engines.pnpm` en vez de `packageManager`.
- **`ui/pnpm-workspace.yaml` necesario con pnpm 12.** pnpm 12 ya no lee el campo `pnpm` de `package.json`; los ajustes viven en el workspace. El fichero permite explícitamente el único script de instalación que hace falta —`allowBuilds: { esbuild: true }`—; sin esa lista, `pnpm install` acaba con `ERR_PNPM_IGNORED_BUILDS` y rompería el CI. Todo lo demás queda bloqueado por defecto.
- **Un único stream SSE global.** El panel sigue el esquema de §5 tal cual: `GET /api/v1/events`, `event: execution_progress`, payload literal de 7 campos. No inventa otro esquema ni abre un stream por pantalla.
- **Migraciones y el resto del backend no se tocan en esta feature.** No hay cambio de esquema ni de contrato de la API: `src/api/mod.rs` solo cambia la visibilidad de `not_found`/`metodo_no_permitido` y `Cargo.toml` activa una feature que ya venía con `tower-http`.
- **El panel como fallback, no como rutas propias.** Mantiene intacto el contrato de `/api/v1` y conserva el `404`/`405`/`401` con envoltura para todo `/api/*`.

## Follow-ups

1. **`Dockerfile` + `compose` del §8** — el empaquetado en imagen, explícitamente fuera de este change.
2. **Aviso de Vite: chunk > 500 kB** en el bundle de Antd — candidato a `manualChunks` para partirlo.
3. **Heurística de hash para `Cache-Control: immutable` frágil** (`src/panel.rs`): reconoce el hash de Vite por la forma del nombre (último trozo separado por `-`, alfanumérico y largo); un hash que contuviera `-` o `_` **perdería la marca en silencio** y el asset se cachearía con `no-cache`. Declarado en el código.
4. **`serve()` fija `PANEL_POR_DEFECTO` relativo al CWD sin inyectarlo ni informarlo** (`src/main.rs`): si el proceso arranca desde otro directorio, el panel no se encuentra y el `404` es la única pista.
5. **Assets inexistentes contestan `200 text/html` con el documento en vez de `404`** — decisión abierta, hoy conforme a la spec (el SPA resuelve rutas del cliente con `index.html`).
6. **El botón «Reintentar» de la pantalla de sesión no reintegra `/health`** cuando el que falló fue `useSalud`.
7. **`engines.pnpm` como red de seguridad** del CI, para no depender solo del `version: 12` del workflow.
8. **Valorar `Strict-Transport-Security`** si el TLS del panel no lo termina un proxy delante del binario.

## Pruebas

- **Backend:** `just check` en verde — `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` sin avisos y `cargo test --locked` con **425** pruebas de la suite **+ 5** de `tests/arranque.rs`. El módulo `src/panel.rs` trae sus propias pruebas de integración (raíz, fallback SPA, asset con su tipo, cabeceras de caché y de seguridad, `/api` con envoltura, traversal —incluidas las formas codificadas—, sin `dist` y `405`).
- **Panel:** `just ui-check` en verde — **24 ficheros / 121 pruebas**, `tsc --noEmit` y `eslint` limpios, `vite build` OK.
- **Spec:** `openspec validate ui-panel --strict` OK.

Este documento es el hermano de `plans/PLAN-007.md` y continúa a `docs/FEATURE-006-scheduler.md`.
