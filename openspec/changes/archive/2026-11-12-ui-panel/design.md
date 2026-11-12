# Design

## Context

El backend del §5 está cerrado y probado, y no existe `ui/`. El contrato que el panel consume ya está fijado por `api-shell`, `api-resources` y `auth`: envoltura única, fechas RFC3339, UUID con guiones, booleanos `has_*` en lugar de secretos, stream global con el evento `execution_progress` y sesión por cookie `HttpOnly`. El SRS pide el panel en `ui/` y el mismo origen que la API (§7 y §8), y el CI ya tiene un job `frontend` que se enciende solo cuando aparece `ui/package.json`.

La restricción que más condiciona el diseño es que **no se añaden endpoints**: el panel se escribe contra lo que hay. En particular no existe una ruta que liste contenedores, ni una que purgue una instantánea, ni un total en el listado de ejecuciones.

## Goals / Non-Goals

**Goals:**

- Un SPA tipado que cubra las pantallas del §6 y sea operable con teclado.
- Un cliente HTTP y un listener SSE únicos, compartidos por todas las pantallas, que respeten la envoltura y el payload del §5.
- Servir el panel compilado desde el mismo binario, sin abrir ni una ruta de negocio y sin romper la suite del backend cuando `ui/dist` no existe.
- Que el CI encienda el panel sin tocar los workflows.

**Non-Goals:**

- `Dockerfile` multi-stage y publicación de imagen: van en un change aparte, aunque el servido estático ya deje el hueco.
- Purga manual de instantáneas y listado de contenedores: la API no las expone y este change no las inventa.
- Renderizado en servidor, rutas internacionalizadas, temas oscuros y modo offline.
- Migración de datos: no hay ninguna.

## Decisions

### 1. Vite + React 19 + TypeScript + Ant Design v5 + TanStack Query

Vite es el andamiaje que el SRS y el entorno esperan, arranca en frío en segundos y produce `ui/dist` estático, que es justo lo que el backend necesita servir. React 19 es la versión del entorno para repos nuevos; TypeScript es obligatorio porque los DTO del cable se escriben a mano y el compilador es la única red que detecta una errata en un campo. Ant Design v5 aporta los componentes que pide el §6 —`Table`, `Form`, `Modal`, `Progress`, `Badge`, `Statistic`— sin construir un sistema de diseño. TanStack Query separa el estado del servidor del estado de la pantalla, cachea, invalida y reintenta sin que cada página lo reinvente.

Alternativas descartadas: Create React App (sin mantenimiento y sin build estático moderno), Next.js (SSR que el despliegue de un solo binario no necesita y que complica el mismo origen), Vue/Svelte (el SRS fija React), Redux (estado de servidor con un boilerplate que Query ya cubre), SWR (más simple, pero sin invalidación ni mutaciones tan directas).

### 2. `pnpm` y un lockfile versionado

El entorno fija pnpm 12 y el CI deduce el gestor del lockfile: con `ui/pnpm-lock.yaml` usará `pnpm install --frozen-lockfile` sin tocar `.github/workflows/ci.yml`. `packageManager` en `package.json` fija la versión y evita que un `npm install` accidental genere un árbol distinto.

Alternativa descartada: npm, que también funciona en el CI pero duplicaría el lockfile y rompería la convención del entorno.

### 3. Dev con proxy de Vite y producción con el mismo origen

En desarrollo el SPA corre en el puerto de Vite y el binario en `BIND_ADDRESS`; `vite.config.ts` proxya `/api` al backend. Eso evita CORS y hace que la cookie de sesión y el `EventSource` se comporten en desarrollo igual que en producción. El proxy SHALL desactivar el buffering para SSE (`Change-Origin` y sin compresión en esa ruta), porque un proxy que acumula la respuesta convierte el stream en una descarga que no llega.

En producción no hay proxy: el binario sirve el `index.html` y los assets, y el panel llama a `/api/v1` en el mismo origen. Un solo origen elimina CORS, `SameSite` y la necesidad de configurar una URL base en el cliente.

Alternativas descartadas: dos contenedores con nginx (dos orígenes y CORS, y el SRS describe un solo servicio), y una URL de API configurable por entorno (una variable más que puede estar mal y que el despliegue del §8 no define).

### 4. El servido estático se compone en `serve`, no dentro de `api_router`

`api_router` es la superficie de la API y sus pruebas miden la envoltura, el `404` y el `405`. El panel se compone **encima**: `serve` construye el router de la API y le añade las rutas del panel. Así el router de la API sigue siendo el mismo objeto que las pruebas conocen y el panel no puede cambiar por accidente el contrato de `/api/v1`.

La composición concreta: dos rutas de lectura —`/` y `/{*path}`—, con un manejador que primero mira si el camino empieza por `api/`; si es así, devuelve el `404` de la envoltura y **no** sirve el documento. Para el resto, resuelve el fichero dentro del directorio del panel; si no existe, devuelve `index.html`. Los métodos que no sean `GET`/`HEAD` contestan `405` con la envoltura. El directorio es `ui/dist` relativo al directorio de trabajo, y la función que compone recibe la ruta como parámetro para que las pruebas usen un directorio temporal; **no** se añade una variable de entorno en este change.

Alternativas descartadas: `ServeDir` como *fallback* global (deja que una ruta desconocida de `/api/v1` caiga en el SPA si el orden o el merge cambian, y el fallo sería silencioso), incrustar `ui/dist` con `include_dir!` (obliga a recompilar Rust por un cambio de CSS y engorda el binario), y servir el panel desde el servidor de Vite en producción (no es un servidor de producción).

### 5. Los DTO se tipan a mano y se validan en el borde

`ui/src/services/api/types.ts` declara las vistas del cable —`Target`, `Repository`, `Job`, `Execution`, `Snapshot`, `Session`, `Health`— con los nombres literales del §3/§5, y una función `desenvolver` comprueba que la respuesta trae `status`, `message` y `data` antes de entregarla. No se genera código desde Rust: no hay tooling en el repositorio y un generador sería una dependencia más que mantener; el precio es que un cambio de campo en la API se detecta en el compilador o en la prueba de contrato, no en tiempo de ejecución.

Alternativa descartada: `zod` para validar en tiempo de ejecución (útil, pero duplica cada tipo y no aporta sobre la comprobación de la envoltura en un cliente que solo habla con un servidor del mismo repositorio).

### 6. SSE con una conexión por pestaña, referencias contadas y reconexión con espera creciente

El `EventSource` vive en un proveedor montado una sola vez, con un contador de consumidores: la primera pantalla que se suscribe abre la conexión y la última que se va la cierra. El evento se escucha por nombre (`execution_progress`) y se valida que trae los siete campos antes de publicarlo. `percentage: null` se conserva como nulo y la UI lo pinta indeterminado.

`EventSource` reconecta solo, pero su espera no se puede controlar y no se puede cancelar limpiamente; por eso el proveedor cierra el objeto al fallar y abre otro con una espera creciente y un tope, con reinicio al recibir un `open`. Los eventos de estado terminal (`success`, `failed`) invalidan las consultas de ejecuciones y tareas para que la verdad venga del histórico y no de un evento que se pudo perder. Si el stream devuelve `401` de forma repetida, se deja de reintentar y se muestra la pantalla de sesión, en vez de martillear el servidor.

Alternativas descartadas: `fetch` con `ReadableStream` (permite cabeceras, pero obliga a implementar el parser de SSE y no aporta nada aquí, porque la cookie va sola), y una conexión por pantalla (multiplica el fan-out del hub y duplica eventos).

### 7. Estado del servidor con TanStack Query y estado en vivo aparte

Las consultas usan claves estables (`['jobs']`, `['executions', filtros]`, `['snapshots', repositoryId]`) y una `staleTime` corta para lo que cambia con las copias. El estado en vivo —el último evento por ejecución— vive en un `Map` en memoria del proveedor SSE, no en la caché de Query: mezclar un evento efímero con una fila paginada haría que un `refetch` pisara el progreso o que un evento viejo resucitara una fila borrada.

Alternativa descartada: meter cada evento en la caché de `executions` (obliga a inventar filas que aún no están en la página y a decidir qué hacer al paginar).

### 8. Sesión: la salud decide el modo y la cookie decide el usuario

Al arrancar se pide `GET /api/v1/health`; `auth_mode: "disabled"` significa que no hay capa y el panel entra directo, sin llamar a `/auth/session` —que en ese modo contesta `404`—. Con `auth_mode: "oidc"` se pide `/auth/session`; un `401` no es un error de pantalla sino el estado «no ha entrado», y el botón de entrada navega a `/api/v1/auth/login` en el mismo documento para que el servidor haga la redirección. La salida es `POST /api/v1/auth/logout`, seguida de `queryClient.clear()` y del cierre del stream: si no se cierra, el `EventSource` sigue reconectando contra una sesión que ya no existe.

Alternativa descartada: leer el `auth_mode` de una variable de compilación (`import.meta.env`), que obligaría a construir un panel distinto por modo y podría discrepar del servidor.

### 9. Formularios: el servidor es la autoridad, el cliente solo evita el viaje obvio

El generador cron construye seis campos y muestra la expresión resultante, pero la valida el servidor con el analizador del planificador; un `400` se pinta junto al campo `cron_expression`. La retención se valida en el cliente para el caso que el servidor también rechaza —las tres a cero— porque ahí el viaje no aporta nada. Los identificadores se eligen con `Select` sobre los listados; el `container_name` es un `AutoComplete` con las etiquetas ya usadas y texto libre, porque no hay endpoint de contenedores y bloquear el campo a una lista vacía impediría crear el primer target.

Las credenciales se escriben con `Input.Password`, `autoComplete="new-password"` y sin valor inicial. Al editar, un campo vacío se omite del cuerpo, que es lo que la API interpreta como «conservar»; el panel nunca recibe el secreto, solo el booleano `has_*`.

### 10. Pruebas: Vitest + Testing Library, con MSW y un `EventSource` falso

Cada pantalla tiene su prueba colocada (`Dashboard.test.tsx`, `Jobs.test.tsx`…) con Testing Library y consultas por rol y texto, no por clase. Las peticiones se interceptan con MSW para probar también los errores y la envoltura; el `EventSource` se sustituye por un doble controlable para probar el evento, el `percentage` nulo y la reconexión. El backend añade pruebas de integración del servido estático con un directorio temporal: raíz, fallback, asset, `404` de `/api/v1`, `405`, traversal y ausencia de `dist`.

Alternativas descartadas: Playwright (valioso, pero un segundo runtime de navegador en un CI que hoy no lo tiene; se puede añadir después sin cambiar el diseño) y pruebas de instantáneas (frágiles y no dicen nada del comportamiento).

## Risks / Trade-offs

- **El fallback SPA se come una ruta de la API** → el manejador comprueba el prefijo `api/` antes de servir y hay una prueba explícita para `/api/v1/no-existe`; si el `404` deja de llevar la envoltura, la suite del backend lo delata.
- **El espacio en S3 se calcula con una consulta por repositorio y puede quedarse corto** → las consultas van en paralelo, se cachean y, con `truncated`, la cifra se rotula como mínimo; nunca se presenta como total.
- **Un proxy intermedio buffea el SSE y el panel parece congelado** → el proxy de desarrollo desactiva el buffering y el panel muestra el estado de la conexión, de modo que «sin eventos» se distingue de «conexión caída».
- **La sesión caduca con el stream abierto** → el `EventSource` reintenta contra un `401`; tras varios fallos sin `open` se deja de reintentar y se muestra la pantalla de sesión.
- **El histórico no devuelve total** → la paginación se basa en «página llena» y el botón «siguiente» se deshabilita con una página incompleta; se acepta no poder saltar a la última página.
- **No hay endpoint de contenedores** → el campo es texto libre con sugerencias; un nombre inexistente no se detecta hasta la ejecución y el fallo aparece en el histórico, que es donde el operador lo puede leer.
- **El panel todavía no tiene imagen** → `ui/dist` se sirve desde el binario, pero `release.yml` sigue sin publicar imagen hasta que exista `Dockerfile`; se declara como follow-up y no como parte de este change.
- **Ant Design engorda el bundle** → se importan componentes concretos, se separan las rutas con `lazy` y se vigila el tamaño del build; el panel es una herramienta interna y no una página pública.
- **Las fechas se muestran en hora local y el servidor programa en UTC** → la interfaz etiqueta la zona y el formulario explica que el cron se interpreta en UTC, para que un `0 0 3 * * *` no se lea como las 3 de la mañana locales.

## Migration Plan

1. Crear `ui/` y compilar: `pnpm install`, `pnpm build` → `ui/dist`. El backend no cambia de comportamiento mientras no exista `dist`.
2. Añadir el servido estático y sus pruebas; con `ui/dist` presente, `/` sirve el panel; sin él, todo sigue como hoy.
3. Desplegar el binario con `ui/dist` al lado. No hay migraciones de base ni cambios de contrato en `/api/v1`.
4. Rollback: retirar `ui/dist` del despliegue o revertir el commit del servido estático; la API y el motor no dependen del panel.

## Open Questions

- ¿El `Dockerfile` multi-stage y la publicación de imagen entran en el issue siguiente o en el mismo? No cambia el diseño: el binario ya sirve `ui/dist`.
- ¿Se expondrá una ruta para listar contenedores y otra para purgar instantáneas? Si llegan, el `AutoComplete` pasa a `Select` y la tabla gana su acción, sin tocar el resto del panel.
- ¿Se añadirá paginación con total en `/api/v1/executions`? Si llega, el botón «siguiente» puede pasar a un paginador completo; hoy la heurística de página llena es suficiente.