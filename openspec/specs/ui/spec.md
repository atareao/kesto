# ui Specification

## Purpose
El panel web de Kesto: el SPA que convierte la API del §5 en un producto operable —layout y navegación, cliente HTTP y SSE, las pantallas de Dashboard, Tareas, Histórico, Snapshots, Ajustes y sesión— y que fija cómo se comporta cuando los datos llegan, cuando fallan y cuando no hay ninguno.

## Requirements

### Requirement: El layout ofrece las cinco pantallas del §6 y una ruta para cada una

El panel SHALL montarse en un layout con menú lateral y SHALL ofrecer, como mínimo, las rutas `/` (Dashboard), `/tareas`, `/historico`, `/snapshots`, `/ajustes` y `/sesion`. Cada entrada del menú SHALL navegar a su ruta sin recargar el documento, la entrada de la ruta activa SHALL distinguirse visualmente, y una ruta desconocida SHALL renderizar una pantalla de «no encontrada» **dentro** del layout, sin perder el menú. En una ventana estrecha el menú SHALL poder plegarse sin dejar ninguna entrada inalcanzable.

Las cinco pantallas y sus nombres salen del §6; la de sesión es la que falta en esa lista porque el §6 describe la pantalla ya dentro del producto, no la entrada. Las rutas son del cliente y no del servidor: por eso el backend sirve `index.html` como fallback y una recarga en `/tareas` tiene que seguir funcionando.

#### Scenario: Cada entrada del menú abre su pantalla
- **GIVEN** el panel cargado en `/`
- **WHEN** se pulsa cada una de las cinco entradas del menú
- **THEN** SHALL mostrarse la pantalla de esa entrada
- **AND** la URL SHALL cambiar a su ruta
- **AND** NO SHALL recargarse el documento

#### Scenario: La ruta activa se marca en el menú
- **GIVEN** el panel abierto en `/historico`
- **WHEN** se pinta el menú
- **THEN** la entrada de Histórico SHALL ser la marcada como activa
- **AND** las demás NO SHALL estarlo

#### Scenario: Una ruta desconocida no rompe el layout
- **GIVEN** una URL del cliente que no corresponde a ninguna pantalla
- **WHEN** se carga el panel
- **THEN** SHALL mostrarse la pantalla de «no encontrada»
- **AND** el menú lateral SHALL seguir visible y operativo

### Requirement: El cliente de la API consume la envoltura y no filtra secretos

Toda llamada del panel SHALL dirigirse a `/api/v1/*` en el mismo origen, con credenciales de cookie incluidas, y SHALL interpretar la respuesta según la envoltura de `api-shell`: una respuesta atendida trae `{status, message, data}` y una fallida trae `{status, message}`. El cliente SHALL comprobar que `status` coincide con el código HTTP, SHALL tratar un cuerpo que no sea la envoltura como error de protocolo y NO SHALL inventar un `data` vacío para disimularlo.

Las fechas SHALL presentarse al usuario en la zona horaria local a partir de la cadena RFC3339 que publica la API, sin reinterpretarla como si fuera un instante sin desplazamiento, y los identificadores UUID SHALL conservarse en su forma canónica con guiones en cualquier comparación, clave de lista o URL. El panel NO SHALL mostrar, registrar ni guardar en almacenamiento del navegador ningún campo de secreto; para las credenciales SHALL usar únicamente los booleanos `has_*` que publica la API.

Los mensajes de error SHALL ser el `message` de la envoltura, presentado como texto y nunca como HTML, y NO SHALL incluir el cuerpo de la petición ni el valor de un campo del formulario.

#### Scenario: Una respuesta correcta se entrega como datos
- **GIVEN** una respuesta `200` con `{status: 200, message, data}`
- **WHEN** el cliente la procesa
- **THEN** SHALL devolver `data` a la pantalla
- **AND** NO SHALL mostrar ningún error

#### Scenario: Un error de la API se muestra con su mensaje y sin el cuerpo enviado
- **GIVEN** una respuesta `400` cuyo `message` nombra el campo `cron_expression`
- **WHEN** el cliente la procesa
- **THEN** la pantalla SHALL mostrar ese `message`
- **AND** NO SHALL mostrar el valor que se había escrito en el formulario como parte del error

#### Scenario: Un cuerpo fuera de la envoltura se trata como error de protocolo
- **GIVEN** una respuesta `200` cuyo cuerpo es JSON pero no trae `status` ni `data`
- **WHEN** el cliente la procesa
- **THEN** SHALL producirse un error de protocolo
- **AND** NO SHALL pintarse una pantalla vacía como si la petición hubiera ido bien

#### Scenario: Un 401 devuelve al usuario a la pantalla de sesión
- **GIVEN** una petición cualquiera que contesta `401` con la envoltura
- **WHEN** el cliente la recibe
- **THEN** SHALL descartarse la caché de datos de la sesión
- **AND** SHALL mostrarse la pantalla de sesión

### Requirement: El panel sigue el stream global y refleja el estado en vivo

El panel SHALL abrir **una sola** conexión `EventSource` contra `GET /api/v1/events`, SHALL escuchar el evento `execution_progress` y SHALL leer sus siete campos del §5: `execution_id`, `job_id`, `status`, `bytes_processed`, `bytes_uploaded`, `percentage` y `current_action`. NO SHALL inventar nombres de evento ni campos adicionales.

El estado recibido SHALL reflejarse en el Dashboard y en el Histórico sin recargar la página. Un `percentage` nulo SHALL pintarse como progreso indeterminado y NO SHALL convertirse en `0`, porque el motor lo publica nulo cuando no conoce el total. Cuando la conexión se cierre o falle, el panel SHALL reintentar con una espera creciente y SHALL mostrar el estado de la conexión; al desmontar la última pantalla que lo consume, SHALL cerrar el `EventSource`. NO SHALL abrirse una conexión por pantalla ni una por ejecución.

#### Scenario: Un evento actualiza la ejecución en vivo
- **GIVEN** el panel abierto y una ejecución en curso
- **WHEN** llega un evento `execution_progress` con `status` `running` y `percentage` `40.15`
- **THEN** el Dashboard SHALL mostrar ese porcentaje y ese estado
- **AND** el Histórico SHALL mostrar la misma ejecución actualizada
- **AND** NO SHALL hacer falta recargar la página

#### Scenario: Un porcentaje nulo se pinta como indeterminado
- **GIVEN** un evento con `percentage` nulo
- **WHEN** el panel lo procesa
- **THEN** el indicador SHALL mostrarse indeterminado
- **AND** NO SHALL mostrarse `0` ni una barra llena

#### Scenario: Una conexión caída se reintenta
- **GIVEN** una conexión SSE abierta
- **WHEN** el servidor cierra la conexión
- **THEN** el panel SHALL marcar la conexión como perdida
- **AND** SHALL reintentar con una espera creciente
- **AND** al volver SHALL seguir procesando eventos

#### Scenario: Solo hay una conexión abierta
- **GIVEN** el Dashboard y el Histórico montados a la vez
- **WHEN** se cuentan las conexiones al stream
- **THEN** SHALL haber exactamente una

### Requirement: El Dashboard resume el estado y pinta el avance de la copia en curso

El Dashboard SHALL mostrar tres tarjetas de estadísticas: espacio consumido en S3, número de repositorios y número de tareas activas. El espacio SHALL calcularse sumando los `bytes` de las instantáneas de cada repositorio; cuando el listado de un repositorio venga con `truncated` en `true`, la cifra SHALL presentarse como **mínimo** («al menos») y NO SHALL presentarse como total. Las tareas activas SHALL ser las tareas con `enabled` en `true`.

El Dashboard SHALL mostrar un `Progress` con el avance de la ejecución en curso alimentado por SSE, y una tabla de tareas con un `Badge` por estado. El estado de una tarea SHALL ser el de su ejecución más reciente —`success`, `processing` o `error`—, y una tarea sin ejecuciones SHALL pintarse como «sin ejecuciones» y no como correcta. El color NO SHALL ser el único portador del estado: el `Badge` SHALL ir acompañado de su texto.

#### Scenario: Las tarjetas cuentan lo que hay
- **GIVEN** dos repositorios, tres tareas de las cuales dos están activas y un total de instantáneas conocido
- **WHEN** se carga el Dashboard
- **THEN** la tarjeta de repositorios SHALL mostrar `2`
- **AND** la de tareas activas SHALL mostrar `2`
- **AND** la de espacio SHALL mostrar la suma de los bytes de las instantáneas

#### Scenario: Un listado recortado se presenta como mínimo
- **GIVEN** un repositorio cuya respuesta trae `truncated: true`
- **WHEN** se calcula el espacio consumido
- **THEN** la tarjeta SHALL indicar que la cifra es un mínimo
- **AND** NO SHALL presentarla como el total del repositorio

#### Scenario: El avance en vivo manda sobre el último dato del histórico
- **GIVEN** una ejecución en curso con un `percentage` conocido
- **WHEN** llega un evento `execution_progress`
- **THEN** el `Progress` SHALL mostrar el porcentaje del evento
- **AND** la tabla SHALL mostrar esa tarea como `processing`

#### Scenario: Una tarea sin ejecuciones no se pinta como correcta
- **GIVEN** una tarea activa que nunca se ha ejecutado
- **WHEN** se pinta la tabla
- **THEN** su estado SHALL ser «sin ejecuciones»
- **AND** NO SHALL mostrarse con el color de éxito

### Requirement: Tareas permite dar de alta, editar, disparar y borrar sin salir del panel

La pantalla de Tareas SHALL listar las tareas y SHALL permitir crear y editar en un `Modal` con `Form`, borrar con confirmación y disparar manualmente cada tarea. El formulario SHALL exigir `target_id`, `repository_id`, `cron_expression` y las tres reglas de retención, y SHALL usar `Select` para el target y el repositorio. El campo `container_name` del target SHALL ser un `AutoComplete` que sugiere los contenedores ya usados por los targets existentes y admite texto libre; NO SHALL introducirse un endpoint de contenedores en este change.

La expresión cron SHALL poder construirse con un generador visual de **seis** campos —segundo, minuto, hora, día del mes, mes y día de la semana— y SHALL poder editarse como texto. La validación del cliente SHALL ser una ayuda y NO SHALL sustituir a la del servidor: un `400` que nombre `cron_expression` SHALL mostrarse en el campo. Las tres reglas de retención SHALL ser `InputNumber` no negativos y el formulario NO SHALL enviar una tarea con las tres a cero.

Cuando haya que crear un target o un repositorio desde el propio formulario, las credenciales —`db_password`, `secret_access_key` y `passphrase`— SHALL escribirse con `Input.Password`, NO SHALL prerrellenarse nunca y, al editar, dejarlas vacías SHALL significar «conservar la que hay».

El disparo manual SHALL llamar a `POST /api/v1/jobs/:id/run`. Un `202` SHALL incorporar la ejecución devuelta con estado `pending` y refrescar el Histórico; un `409` SHALL explicar que la tarea ya tiene una ejecución en curso; un `404` SHALL retirar la tarea de la lista tras refrescarla.

#### Scenario: El alta envía los campos del §3 y refresca la lista
- **GIVEN** un formulario con target, repositorio, cron `0 0 3 * * *`, retención `7/4/6` y `enabled` marcado
- **WHEN** se guarda
- **THEN** SHALL enviarse `POST /api/v1/jobs` con esos campos
- **AND** con un `201` la lista SHALL mostrar la tarea nueva

#### Scenario: El generador produce una expresión de seis campos
- **GIVEN** el generador puesto a las 03:00 de todos los días
- **WHEN** se aplica al formulario
- **THEN** el campo `cron_expression` SHALL contener `0 0 3 * * *`
- **AND** SHALL poder enviarse sin edición manual

#### Scenario: Una retención que no conserva nada no se envía
- **GIVEN** un formulario con `keep_daily`, `keep_weekly` y `keep_monthly` a cero
- **WHEN** se intenta guardar
- **THEN** el formulario SHALL impedir el envío
- **AND** SHALL explicar que al menos una regla tiene que conservar algo

#### Scenario: Un cron rechazado por el servidor se señala en su campo
- **GIVEN** un formulario con una expresión que el servidor rechaza con `400`
- **WHEN** llega la respuesta
- **THEN** el mensaje SHALL mostrarse junto al campo `cron_expression`
- **AND** el formulario NO SHALL cerrarse

#### Scenario: El disparo manual distingue el 202 del 409
- **GIVEN** una tarea sin ejecución en curso
- **WHEN** se pulsa «ejecutar» y el servidor contesta `202`
- **THEN** SHALL mostrarse la ejecución encolada como `pending`
- **AND** con un `409` SHALL mostrarse que ya hay una ejecución en curso

#### Scenario: Las credenciales se escriben enmascaradas y no se prerrellenan
- **GIVEN** el formulario de alta de un repositorio desde la pantalla de Tareas
- **WHEN** se pinta el campo `passphrase`
- **THEN** SHALL ser un `Input.Password` vacío
- **AND** al editar un repositorio existente, dejarlo vacío NO SHALL borrar la credencial guardada

### Requirement: El Histórico pagina, filtra y muestra la consola de la ejecución en vivo

El Histórico SHALL listar las ejecuciones con `GET /api/v1/executions`, SHALL paginar con `limit` y `offset` —`50` por defecto y `200` como tope—, y SHALL filtrar por `job_id` y por `status`. Como la respuesta es un array sin total, la paginación SHALL avanzar cuando la página venga llena y SHALL deshabilitar «siguiente» cuando venga vacía o incompleta; NO SHALL inventarse un total.

Cada fila SHALL mostrar las fechas en hora local, la duración cuando haya `finished_at`, los dos contadores de bytes y el estado con su `Badge` y su texto. La consola SHALL mostrar en vivo el `current_action` y el avance de la ejecución seleccionada a partir del stream SSE, SHALL conservar solo un número acotado de líneas y SHALL indicar cuando la conexión está caída.

#### Scenario: La paginación avanza y se detiene al final
- **GIVEN** una primera página con `limit=50` que devuelve 50 filas
- **WHEN** se pide la siguiente página
- **THEN** SHALL pedirse con `offset=50`
- **AND** si la respuesta viene vacía, «siguiente» SHALL quedar deshabilitado

#### Scenario: Los filtros se envían a la API
- **GIVEN** un filtro por `status=error` y una tarea elegida
- **WHEN** se aplica
- **THEN** la petición SHALL llevar `job_id` y `status`
- **AND** la tabla SHALL mostrar solo esas filas

#### Scenario: La consola sigue la ejecución seleccionada
- **GIVEN** una ejecución seleccionada en el Histórico
- **WHEN** llega un evento `execution_progress` de esa ejecución
- **THEN** la consola SHALL añadir su `current_action` y su avance
- **AND** los eventos de otras ejecuciones NO SHALL mezclarse en esa consola

#### Scenario: Sin ejecuciones se muestra un vacío, no un error
- **GIVEN** un histórico sin filas
- **WHEN** se carga la pantalla
- **THEN** SHALL mostrarse el estado vacío
- **AND** NO SHALL mostrarse un error

### Requirement: Snapshots explora un repositorio y dice cuándo el listado está recortado

La pantalla de Snapshots SHALL exigir elegir un repositorio de los que devuelve `GET /api/v1/repositories` y SHALL listar sus instantáneas con `GET /api/v1/snapshots?repository_id=<id>`. La tabla SHALL mostrar el identificador, la fecha en hora local, la etiqueta, los ficheros y los bytes, y SHALL presentar `paths` sin romper la fila. Cuando `truncated` sea `true`, SHALL avisar de que puede haber más instantáneas y NO SHALL presentar la lista como completa.

La pantalla NO SHALL ofrecer una acción de purga manual mientras la API no exponga una ruta para ella: un botón que no puede cumplir lo que promete es peor que su ausencia.

#### Scenario: Se elige un repositorio y se listan sus instantáneas
- **GIVEN** dos repositorios dados de alta
- **WHEN** se elige uno
- **THEN** SHALL pedirse `GET /api/v1/snapshots` con su `repository_id`
- **AND** la tabla SHALL mostrar sus instantáneas

#### Scenario: Un listado recortado se avisa
- **GIVEN** una respuesta con `truncated: true`
- **WHEN** se pinta la tabla
- **THEN** SHALL mostrarse un aviso de que puede haber más
- **AND** NO SHALL afirmarse que ésas son todas

#### Scenario: Un repositorio sin instantáneas muestra el vacío
- **GIVEN** un repositorio sin instantáneas
- **WHEN** se consulta
- **THEN** SHALL mostrarse el estado vacío
- **AND** la tabla NO SHALL pintar filas inventadas

### Requirement: La sesión se adapta al modo de autenticación y no llama a rutas que no existen

En modo de desarrollo (`auth_mode` `disabled`) el panel SHALL entrar directamente al Dashboard, NO SHALL pedir `GET /api/v1/auth/session`, NO SHALL mostrar la pantalla de sesión como obligatoria y NO SHALL llamar a ninguna ruta `/api/v1/auth/*`, que en ese modo contesta `404`.

En modo OIDC el panel SHALL consultar `GET /api/v1/auth/session` al arrancar. Si contesta `200`, SHALL mostrar el `sub` y el `email` de la sesión en la pantalla de sesión y en el menú. Si contesta `401`, SHALL mostrar la pantalla de sesión con un botón que navegue a `GET /api/v1/auth/login` en el mismo documento —la redirección la hace el servidor— y NO SHALL reintentar la petición en bucle. El cierre de sesión SHALL llamar a `POST /api/v1/auth/logout`, SHALL vaciar la caché de consultas y SHALL volver a la pantalla de sesión.

#### Scenario: En desarrollo no se pide sesión
- **GIVEN** un `GET /api/v1/health` que declara `auth_mode: disabled`
- **WHEN** se carga el panel
- **THEN** SHALL mostrarse el Dashboard
- **AND** NO SHALL pedirse `GET /api/v1/auth/session`

#### Scenario: Con sesión se muestra la identidad
- **GIVEN** una sesión OIDC válida con `sub` y `email`
- **WHEN** se abre la pantalla de sesión
- **THEN** SHALL mostrarse el `sub` y el `email`
- **AND** NO SHALL mostrarse ningún token

#### Scenario: Sin sesión se ofrece entrar
- **GIVEN** el modo OIDC y un `401` de `GET /api/v1/auth/session`
- **WHEN** se carga el panel
- **THEN** SHALL mostrarse la pantalla de sesión
- **AND** el botón de entrada SHALL apuntar a `/api/v1/auth/login`
- **AND** NO SHALL repetirse la petición de sesión en bucle

#### Scenario: Al salir se vacía la caché
- **GIVEN** una sesión abierta con datos cargados
- **WHEN** se cierra la sesión y el servidor contesta
- **THEN** SHALL volverse a la pantalla de sesión
- **AND** las consultas cacheadas NO SHALL seguir visibles

### Requirement: Ajustes describe el despliegue y gestiona las fuentes y los destinos

La pantalla de Ajustes SHALL mostrar la versión y el `auth_mode` que publica `GET /api/v1/health`, sin pedir ni mostrar ningún secreto. SHALL permitir gestionar los targets y los repositorios con las rutas existentes —listar, crear, editar y borrar—, y en esos formularios `db_password`, `secret_access_key` y `passphrase` SHALL escribirse con `Input.Password`, NO SHALL prerrellenarse y, al editar, dejarlas vacías SHALL conservar la credencial. La pantalla SHALL mostrar para cada fila si la credencial está configurada, a partir de los booleanos `has_*`, y NO SHALL publicar el valor.

#### Scenario: La salud describe el despliegue sin secretos
- **GIVEN** un panel conectado
- **WHEN** se abre Ajustes
- **THEN** SHALL mostrarse la versión y el `auth_mode`
- **AND** el cuerpo de la respuesta NO SHALL contener ningún secreto

#### Scenario: Una credencial configurada se anuncia sin revelarse
- **GIVEN** un repositorio con `has_passphrase: true`
- **WHEN** se pinta su fila
- **THEN** SHALL indicarse que la passphrase está configurada
- **AND** NO SHALL mostrarse su valor

#### Scenario: Editar sin escribir la credencial la conserva
- **GIVEN** un repositorio con passphrase guardada
- **WHEN** se edita el repositorio dejando el campo vacío
- **THEN** el cuerpo enviado NO SHALL incluir `passphrase`
- **AND** la credencial guardada SHALL seguir configurada

### Requirement: Cada pantalla tiene estados de carga, error y vacío, y es accesible con teclado

Cada pantalla que lee de la API SHALL mostrar un estado de carga mientras la petición está en vuelo, un estado de error con el `message` de la API y una acción para reintentar cuando la petición falla, y un estado vacío cuando la respuesta es correcta pero no hay filas. NO SHALL mostrar una tabla vacía como si no hubiera datos ni un error como si no hubiera nada.

El panel SHALL ser operable solo con teclado: todo control SHALL ser alcanzable y activable, los campos SHALL tener etiqueta asociada, el foco SHALL entrar en un `Modal` al abrirse y volver al control que lo abrió al cerrarse, y el avance en vivo SHALL anunciarse en una región `aria-live` sin robar el foco. El estado de una tarea NO SHALL depender solo del color y el contraste del texto SHALL cumplir el nivel AA.

#### Scenario: Una petición lenta muestra carga y no un vacío
- **GIVEN** una petición que aún no ha contestado
- **WHEN** se pinta la pantalla
- **THEN** SHALL mostrarse el estado de carga
- **AND** NO SHALL mostrarse el estado vacío

#### Scenario: Un error ofrece reintentar
- **GIVEN** una petición que falla con un `message` de la API
- **WHEN** se pinta la pantalla
- **THEN** SHALL mostrarse ese `message`
- **AND** SHALL ofrecerse una acción que repita la petición

#### Scenario: El modal devuelve el foco a quien lo abrió
- **GIVEN** un formulario abierto desde un botón
- **WHEN** se cierra el formulario con `Esc`
- **THEN** el foco SHALL volver al botón que lo abrió

#### Scenario: El avance se anuncia sin robar el foco
- **GIVEN** una ejecución en curso y el foco en otro control
- **WHEN** llega un evento de avance
- **THEN** el porcentaje SHALL anunciarse en una región `aria-live`
- **AND** el foco NO SHALL moverse

### Requirement: El formulario de fuente ofrece SQLite y sus campos propios

El selector de tipo de fuente de la pantalla de Ajustes SHALL ofrecer los cuatro valores `directory`, `mariadb`, `postgresql` y `sqlite`. Al elegir `sqlite` SHALL mostrarse `container_name` y `db_path`, y SHALL ocultarse `host_path`, `db_user`, `db_password` y `db_name`. Los tipos de la API en TypeScript SHALL incluir `'sqlite'` en la unión del tipo de target y `db_path` en las interfaces de lectura y de alta. El campo `db_path` SHALL ser un texto y NO SHALL tratarse como secreto.

El porqué: el formulario es el espejo del contrato de la API y del `CHECK` de la base. Si el selector no ofrece `sqlite`, nadie puede dar de alta una fuente SQLite desde el panel, y la feature queda a medias por la puerta de delante.

#### Scenario: El selector ofrece SQLite
- **GIVEN** un panel conectado y el formulario de fuente abierto
- **WHEN** se despliega el selector de tipo
- **THEN** SHALL ofrecerse `directory`, `mariadb`, `postgresql` y `sqlite`

#### Scenario: Elegir sqlite muestra solo sus campos
- **GIVEN** el formulario de fuente con el tipo `sqlite` elegido
- **WHEN** se pintan los campos
- **THEN** SHALL verse `container_name` y `db_path`
- **AND** NO SHALL verse `host_path`, `db_user`, `db_password` ni `db_name`

#### Scenario: Una fuente sqlite se da de alta desde el panel
- **GIVEN** el formulario relleno con un `container_name` y un `db_path` válidos
- **WHEN** se envía el alta
- **THEN** SHALL llamarse a la API con `target_type = 'sqlite'` y el `db_path`
- **AND** la fuente SHALL aparecer en el listado

### Requirement: El panel declara su identidad de marca

El panel SHALL servir los assets de marca de Kesto y SHALL declararlos en el documento. En concreto SHALL servirse el favicon (`favicon.ico`, `favicon-16x16.png`, `favicon-32x32.png`, `favicon-48x48.png`, `favicon-64x64.png`), el icono para dispositivos Apple (`apple-touch-icon.png`), los iconos de Android (`android-chrome-192x192.png`, `android-chrome-512x512.png`), el icono enmascarable (`maskable-icon-512x512.png`), el icono de Windows (`mstile-150x150.png`) y el manifiesto (`site.webmanifest`). El documento SHALL declarar los `<link>` de iconos, el `<link rel="manifest">`, la meta `theme-color` con el azul de marca y las meta de Open Graph, y el menú lateral SHALL mostrar el logo de Kesto además del nombre.

El porqué: hoy la pestaña del navegador no identifica a Kesto —no hay favicon ni manifest— y la marca del panel es solo texto. Una aplicación que el operador deja abierta todo el día debe poder reconocerse en una pestaña y en un marcador.

#### Scenario: La pestaña identifica a Kesto
- **GIVEN** el panel cargado en el navegador
- **WHEN** se lee el documento servido
- **THEN** SHALL declararse un `<link rel="icon">` y un `<link rel="manifest">`
- **AND** el favicon y el manifest SHALL servirse desde la raíz del panel

#### Scenario: El menú lateral muestra el logo
- **GIVEN** el panel con el layout desplegado
- **WHEN** se pinta el menú lateral
- **THEN** SHALL verse el logo de Kesto
- **AND** el logo SHALL ser legible en el tema activo

### Requirement: El panel ofrece tema claro y oscuro, con oscuro por defecto y elección persistida

El panel SHALL arrancar en **tema oscuro** siempre que no exista una elección guardada. Un control en el layout SHALL permitir alternar entre tema claro y oscuro; al activarlo, el tema SHALL cambiar de inmediato en todo el panel y la elección SHALL persistir en el almacenamiento del navegador, de modo que una recarga la respete. El arranque NO SHALL seguir la preferencia del sistema por sí solo: sin elección guardada, el resultado SHALL ser el oscuro.

El control SHALL ser accesible: SHALL tener nombre accesible, SHALL ser alcanzable y activable solo con teclado, y SHALL anunciar el tema hacia el que cambia, no solo su color. Ningún color del panel SHALL quedar fijado a un tema: los colores SHALL proceder del tema activo, de modo que ambos sean legibles.

El porqué: el panel se usa en sesiones largas y la identidad visual pide el modo oscuro por defecto, pero el operador debe poder pasar a claro y que su elección se recuerde. Que el color esté fijado a mano es justo lo que hoy hace ilegible una mitad del producto.

#### Scenario: Por defecto, oscuro
- **GIVEN** un navegador sin ninguna elección de tema guardada
- **WHEN** se carga el panel
- **THEN** el tema activo SHALL ser el oscuro

#### Scenario: La elección persiste tras recargar
- **GIVEN** el panel en tema oscuro
- **WHEN** se activa el control de tema y se recarga el panel
- **THEN** el tema activo SHALL ser el claro
- **AND** NO SHALL volver al oscuro por defecto

#### Scenario: Un control accesible
- **GIVEN** el foco en el control de tema
- **WHEN** se activa con el teclado
- **THEN** el tema SHALL cambiar
- **AND** el control SHALL anunciar su estado al usuario

#### Scenario: Ambos temas son legibles
- **GIVEN** el panel en cualquiera de los dos temas
- **WHEN** se recorre la cabecera, el menú lateral y el contenido
- **THEN** NO SHALL haber texto sobre un fondo del mismo color
- **AND** los colores SHALL proceder del tema activo

### Requirement: El código de aplicación no depende de tipos de Node

El proyecto de UI SHALL separar la configuración de TypeScript por fronteras: aplicación, tooling y tests. El `tsconfig.json` de la **aplicación** (`ui/src`) NO SHALL declarar tipos de Node ni globales de Vitest. Los tipos de Node SHALL acotarse a los dos proyectos que sí los necesitan y SHALL declararse en el tsconfig de cada uno, no en el de la aplicación: el proyecto de **tooling** (`ui/tsconfig.node.json`, que typechequea `vite.config.ts`, ejecutado en Node) y el de **tests** (`ui/tsconfig.test.json`, que extiende del de aplicación y declara `"types": ["vitest/globals", "node"]`). El código de aplicación NO SHALL importar ni referenciar APIs o tipos de Node, y el andamiaje de pruebas (setup y dobles) SHALL vivir fuera de `ui/src`. El typecheck de la aplicación SHALL usar el tsconfig de aplicación y el de tests SHALL usar el tsconfig de tests.

El porqué: el panel es código de navegador; permitir tipos de Node en toda la app invita a dependencias accidentales del entorno y oculta errores que solo aparecerían en el bundle. Acotarlos al tooling y a los tests mantiene la frontera explícita: el tooling los necesita para `vite.config.ts` (que corre en Node) y los tests para leer el sistema de ficheros.

#### Scenario: La aplicación compila sin tipos de Node
- **GIVEN** el código de aplicación de `ui/src`
- **WHEN** se ejecuta el typecheck con el tsconfig de aplicación
- **THEN** la compilación SHALL completarse sin resolver tipos de Node
- **AND** el tsconfig de aplicación NO SHALL contener `"types": ["node"]` ni `"types": ["vitest/globals"]`

#### Scenario: El proyecto de tooling conserva sus tipos de Node
- **GIVEN** `ui/vite.config.ts`, que se ejecuta en Node
- **WHEN** se resuelve el tsconfig de ese proyecto de tooling (`ui/tsconfig.node.json`)
- **THEN** los tipos de Node SHALL estar disponibles para él
- **AND** esos tipos SHALL declararse en el tsconfig de tooling, no en el de aplicación

#### Scenario: Los tests conservan los tipos de Node y de Vitest
- **GIVEN** los tests de UI
- **WHEN** se ejecutan con el tsconfig de tests
- **THEN** los tipos de Node y los globales de Vitest SHALL estar disponibles
- **AND** el tsconfig de tests SHALL extender del tsconfig de aplicación

### Requirement: El tema conserva su comportamiento tras el refactor de memoización

El provider y el hook de tema SHALL mantener su API pública y su comportamiento observable: tema oscuro por defecto sin elección guardada, persistencia de la elección en `localStorage` bajo la clave `kesto-tema`, y conmutación inmediata y accesible. La eliminación de `useCallback`/`useMemo` redundantes NO SHALL alterar el tema aplicado, la persistencia ni la accesibilidad del control.

El porqué: la memoización sobrante añade complejidad sin beneficio; retirarla no debe cambiar lo que el operador ve ni cómo se guarda su elección.

#### Scenario: El tema por defecto sigue siendo oscuro
- **GIVEN** un navegador sin elección guardada
- **WHEN** se carga el panel
- **THEN** el tema activo SHALL ser el oscuro

#### Scenario: La elección persiste tras el refactor
- **GIVEN** el panel en tema claro
- **WHEN** se recarga el panel
- **THEN** el tema activo SHALL seguir siendo el claro
- **AND** `localStorage` SHALL conservar la clave `kesto-tema`

#### Scenario: El conmutador sigue siendo accesible
- **GIVEN** el panel cargado
- **WHEN** se navega con teclado hasta el control de tema y se activa
- **THEN** el tema SHALL cambiar
- **AND** el control SHALL anunciar el tema hacia el que cambia
