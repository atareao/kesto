# Tasks

## 0. Preparación

- [ ] 0.1 Confirmar que el issue [#15](https://github.com/atareao/kesto/issues/15) existe y describe el alcance del panel; si no existe, crearlo con `gh issue create` y asignarlo al usuario. Verificación: `gh issue view 15` devuelve el alcance acordado.
- [ ] 0.2 Medir la línea base antes de tocar nada: `just check` en verde, `ui/` ausente y `release.yml` saltándose la imagen. Verificación: los tres hechos quedan anotados en el plan `plans/PLAN-007.md`.
- [ ] 0.3 Fijar el contrato que el panel va a consumir leyendo `api-shell`, `api-resources` y `auth`, y anotar en el plan los campos exactos de cada DTO. Verificación: la lista de campos coincide con `src/api/{targets,repositories,jobs,executions,snapshots}.rs` y con `src/api/events.rs`.

## 1. Andamiaje y CI

- [ ] 1.1 Crear `ui/` con `package.json` (React 19, React Router, Ant Design v5, `@ant-design/icons`, TanStack Query, Vite, TypeScript, Vitest, Testing Library, ESLint), `tsconfig.json`, `vite.config.ts`, `index.html` y `src/main.tsx`, con `packageManager` fijado a pnpm. Verificación: `pnpm install` termina sin errores y genera `ui/pnpm-lock.yaml`.
- [ ] 1.2 Configurar el proxy de desarrollo de `/api` al backend y desactivar el buffering de SSE en esa ruta. Verificación: con `just dev` y `just ui-dev`, `curl http://localhost:5173/api/v1/health` devuelve la envoltura y el stream de `/api/v1/events` entrega eventos sin acumularse.
- [ ] 1.3 Añadir la configuración de Vitest con `jsdom`, `setupTests.ts` y una prueba de humo del montaje. Verificación: `cd ui && npx vitest run` pasa con al menos una prueba.
- [ ] 1.4 Añadir ESLint y el script `lint`, y comprobar que el job `frontend` del CI lo detecta sin tocar `.github/workflows/ci.yml`. Verificación: `pnpm run lint`, `npx tsc --noEmit`, `npx vitest run` y `pnpm run build` pasan en local, y `git diff -- .github/` está vacío.
- [ ] 1.5 Generar `ui/dist` con el build de producción y comprobar que el CI lo produce igual. Verificación: `just ui-build` deja `ui/dist/index.html` y `ui/dist/assets/*`; `just ui-check` pasa sus cuatro pasos.

## 2. Servicios del panel

- [ ] 2.1 RED: escribir las pruebas de tipos y de la envoltura —respuesta correcta, error con `message`, cuerpo fuera de envoltura, `status` que no coincide con el HTTP— antes de implementar. Verificación: `npx vitest run src/services/api` falla por las funciones que aún no existen.
- [ ] 2.2 GREEN: implementar `ui/src/services/api/types.ts` y `desenvolver` con los DTO del cable, y hacer pasar las pruebas de 2.1. Verificación: las pruebas pasan y `npx tsc --noEmit` no señala ningún `any` implícito.
- [ ] 2.3 RED/GREEN del cliente HTTP: `fetch` a `/api/v1` con `credentials: "same-origin"`, error de red, `400` con el `message`, `401` que dispara el estado de sesión y cuerpo no JSON. Verificación: las pruebas cubren los cinco desenlaces y pasan.
- [ ] 2.4 RED/GREEN de las utilidades de presentación: fecha RFC3339 a hora local con zona visible, UUID con guiones conservado y bytes formateados. Verificación: una fecha con desplazamiento distinto de cero se pinta en la zona local y el UUID no cambia de forma.
- [ ] 2.5 RED/GREEN del proveedor SSE con un `EventSource` falso: evento `execution_progress` con los siete campos, `percentage` nulo, evento de otra ejecución, reconexión con espera creciente, una sola conexión con dos consumidores y cierre al desmontar el último. Verificación: las pruebas pasan con temporizadores falsos y no queda ningún `EventSource` abierto al final.
- [ ] 2.6 RED/GREEN del servicio de sesión: `auth_mode` de `/health`, `GET /auth/session`, `401` como estado «sin sesión», `POST /auth/logout` y vaciado de la caché de Query. Verificación: en modo `disabled` no se llama a `/auth/*`; en modo `oidc`, el `401` no se reintenta en bucle.
- [ ] 2.7 Documentar en el propio código el contrato de la envoltura y del evento, y comprobar que ningún servicio importa tipos de la base ni inventa campos. Verificación: `rg "secret_access_key|passphrase|db_password" ui/src` solo aparece en los formularios de escritura, nunca en lecturas ni en `localStorage`.

## 3. Layout, rutas y sesión

- [ ] 3.1 RED: pruebas del layout con las cinco rutas, la marca de la ruta activa y la pantalla de «no encontrada» dentro del menú. Verificación: las pruebas fallan antes de existir el layout.
- [ ] 3.2 GREEN: implementar `Layout`, el menú lateral, el `BrowserRouter` y las rutas `/`, `/tareas`, `/historico`, `/snapshots`, `/ajustes` y `/sesion`. Verificación: cada entrada navega sin recargar y la ruta desconocida conserva el menú.
- [ ] 3.3 RED/GREEN del menú plegable en ventana estrecha, sin dejar entradas inalcanzables. Verificación: una prueba con ancho reducido abre y cierra el menú y alcanza las cinco entradas.
- [ ] 3.4 RED/GREEN de la pantalla de sesión: modo `disabled` entra directo; modo `oidc` con sesión muestra `sub` y `email`; sin sesión muestra el botón a `/api/v1/auth/login`; la salida llama a `/auth/logout` y vuelve a la pantalla. Verificación: las cuatro pruebas pasan y ninguna muestra un token.
- [ ] 3.5 Añadir la comprobación de accesibilidad del layout: `main`, navegación con nombre accesible, un enlace de salto al contenido y foco visible. Verificación: las pruebas por rol encuentran la navegación y el `main`, y `axe` no reporta violaciones críticas en el layout.

## 4. Pantallas

- [ ] 4.1 RED/GREEN del Dashboard con las tres tarjetas: repositorios, tareas activas y espacio sumado de las instantáneas, con el rótulo «al menos» cuando `truncated` sea `true`. Verificación: las pruebas cubren dos repositorios, tareas activas y un listado recortado.
- [ ] 4.2 RED/GREEN del `Progress` en vivo y de la tabla de tareas con `Badge` y texto por estado, incluida la tarea sin ejecuciones. Verificación: un evento SSE actualiza el porcentaje y el estado; `percentage` nulo pinta indeterminado; la tarea sin ejecuciones no se pinta como éxito.
- [ ] 4.3 RED/GREEN de la lista y el `Modal` de Tareas: alta con `Select` de target y repositorio, edición con datos de `GET /jobs/:id`, borrado con confirmación y refresco de la lista. Verificación: las pruebas comprueban el cuerpo enviado y que un `201` añade la fila.
- [ ] 4.4 RED/GREEN del generador cron de seis campos y de la edición directa de la expresión; un `400` del servidor se pinta en el campo `cron_expression` y el modal no se cierra. Verificación: el preset de las 03:00 produce `0 0 3 * * *` y el rechazo del servidor se ve junto al campo.
- [ ] 4.5 RED/GREEN de la retención y de las credenciales: tres `InputNumber` no negativos, bloqueo del envío con las tres a cero, `Input.Password` vacío y omisión del secreto al editar. Verificación: las pruebas comprueban que no se envía con `0/0/0` y que un campo vacío no viaja en el `PUT`.
- [ ] 4.6 RED/GREEN del disparo manual: `202` añade la ejecución `pending`, `409` explica la ejecución en curso y `404` refresca la lista sin la tarea. Verificación: las tres respuestas están simuladas y la pantalla reacciona como dice el requisito.
- [ ] 4.7 RED/GREEN del Histórico: paginación `limit`/`offset` con «siguiente» deshabilitado al final, filtros `job_id` y `status`, fechas locales, duración y métricas. Verificación: las pruebas comprueban los parámetros enviados y el estado del botón con una página llena y otra incompleta.
- [ ] 4.8 RED/GREEN de la consola en vivo: solo eventos de la ejecución seleccionada, límite de líneas y aviso de conexión caída. Verificación: un evento de otra ejecución no aparece en la consola y el aviso aparece al cerrarse el stream.
- [ ] 4.9 RED/GREEN de Snapshots: elección de repositorio, tabla con identificador, fecha local, etiqueta, ficheros y bytes, aviso de `truncated` y estado vacío; sin botón de purga. Verificación: las pruebas cubren listado, recorte y vacío, y no existe ningún control de purga.
- [ ] 4.10 RED/GREEN de Ajustes: versión y `auth_mode` desde `/health`, listado y CRUD de targets y repositorios, `has_*` sin revelar el valor y conservación del secreto al editar. Verificación: las pruebas comprueban que el secreto no se pinta y que el `PUT` omite el campo vacío.
- [ ] 4.11 RED/GREEN de los estados de carga, error y vacío en cada pantalla, con reintento y sin tabla vacía disfrazada. Verificación: una prueba por pantalla con la petición pendiente, con error y con `data` vacío.

## 5. Servido estático del panel en el backend

- [ ] 5.1 RED: escribir las pruebas de integración del servido estático con un directorio temporal —raíz, fallback de `/tareas`, asset con su tipo, `/api/v1/no-existe` con envoltura, `405` de `POST /tareas`, traversal y ausencia de `dist`— antes de implementar. Verificación: `cargo test` falla por las rutas que aún no existen y las pruebas actuales de la API siguen en verde.
- [ ] 5.2 GREEN: implementar el módulo de servido estático y componerlo en `serve`, sin tocar `api_router`, con el guardia de prefijo `api/` y el `404` de la envoltura. Verificación: las pruebas de 5.1 pasan y ninguna prueba existente de `api-shell` cambia.
- [ ] 5.3 Añadir la dependencia de ficheros estáticos de `tower-http` con la feature `fs` y comprobar que el árbol compila. Verificación: `cargo check --all-targets` y `cargo clippy --all-targets -- -D warnings` limpios; `Cargo.lock` se actualiza solo por la dependencia.
- [ ] 5.4 RED/GREEN de las cabeceras de caché y del tipo de contenido de los assets, y del `no-cache` de `index.html`. Verificación: las pruebas comparan las cabeceras de un asset con hash y del documento.
- [ ] 5.5 Probar el comportamiento sin panel: un árbol sin `ui/dist` arranca y `GET /` contesta `404` con envoltura. Verificación: la prueba de integración pasa y `just check` sigue verde en un clon sin build de UI.
- [ ] 5.6 Humo manual del conjunto: `just ui-build` y el binario sirviendo el panel; `curl /`, `curl /tareas`, `curl /api/v1/health` y `curl -N /api/v1/events` con sesión. Verificación: el documento y las rutas del cliente contestan `200 text/html`, la salud mantiene su envoltura y el stream entrega líneas `event:`/`data:`.

## 6. Cierre y documentación

- [ ] 6.1 Ejecutar `just check-all` completo, ahora con `ui-check` real, y anotar los números de pruebas del backend y del panel. Verificación: los siete pasos en verde y el recuento escrito en el plan.
- [ ] 6.2 Repasar el panel contra el §6 y la spec `ui` requisito a requisito, con una lista de comprobación marcada. Verificación: cada requisito tiene al menos una prueba que lo cubre y no queda ningún escenario sin medida.
- [ ] 6.3 Escribir `docs/FEATURE-007-ui-panel.md` con qué es, ficheros implicados, cómo funciona, variables/configuración, cómo construir y desplegar, decisiones y follow-ups. Verificación: el documento existe y los comandos que cita (`just ui-build`, `just check-all`) funcionan tal como están escritos.
- [ ] 6.4 Actualizar `HANDOFF.md` con el estado medido y marcar las tareas del plan `plans/PLAN-007.md`; renombrarlo a `PLAN-007-COMPLETED.md` solo cuando todo esté cerrado. Verificación: `HANDOFF.md` describe el estado real y el plan no tiene casillas pendientes.
- [ ] 6.5 Archivar el change con `openspec archive ui-panel` tras la aprobación, comprobar que `specs/ui` y el delta de `api-shell` se fusionan y que `openspec validate --specs --strict` pasa. Verificación: `openspec list --specs` muestra `ui` y `openspec validate --specs --strict` termina en verde.
- [ ] 6.6 Abrir el PR de `feature/ui-panel` a `development`, referenciando el issue #15, con el resumen de las pruebas y el aviso de que el `Dockerfile` queda para el change siguiente. Verificación: el PR existe, el CI está en verde y el issue queda enlazado.