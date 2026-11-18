# api-tokens Specification

## Purpose

La credencial que permite a una máquina —un agente, un script, el servidor MCP— llamar a la API sin pasar por el navegador. Hoy la única entrada es la cookie de sesión que emite el flujo OIDC, y una máquina no puede llevarla. Un token de API resuelve eso con una decisión que lo condiciona todo: **Kesto guarda solo su hash**, nunca el valor en claro, así que el token se enseña una única vez al crearlo y quien lo pierda tendrá que emitir otro. Cada token lleva un nombre para reconocerlo, unos scopes acotados —`read` y `run`— y una fecha de revocación que se aplica de inmediato. La capa de autenticación admite `Authorization: Bearer` **junto** a la cookie, sin sustituirla, y en modo `disabled` sigue sin exigir nada. Ningún token puede restaurar ni borrar: esas operaciones quedan fuera de su alcance por diseño.

## ADDED Requirements

### Requirement: El token se entrega una sola vez y solo se guarda su hash

`POST /api/v1/tokens` SHALL crear un token con un nombre obligatorio y un conjunto de scopes, y SHALL devolver el valor en claro **una única vez** en esa respuesta. La base SHALL almacenar únicamente su hash —nunca el valor en claro, ni en columnas, ni en logs, ni en la respuesta de un listado—, de modo que un token perdido no se pueda recuperar y haya que revocar y emitir otro.

El valor SHALL llevar un prefijo reconocible y suficiente entropía para no ser adivinable. El hash SHALL ser determinista para poder verificar un token entrante sin guardar el original.

#### Scenario: Al crear un token se devuelve el valor en claro una sola vez
- **GIVEN** una sesión OIDC válida y un cuerpo con `name` y `scopes: ["read"]`
- **WHEN** se pide `POST /api/v1/tokens`
- **THEN** SHALL contestar `201` con el token en claro en el cuerpo
- **AND** la base SHALL contener solo su hash, nunca el valor en claro

#### Scenario: El listado no devuelve el token
- **GIVEN** un token ya creado
- **WHEN** se pide `GET /api/v1/tokens`
- **THEN** cada fila SHALL traer su identificador, nombre, scopes, fechas y estado
- **AND** NO SHALL aparecer el valor en claro ni el hash del token

#### Scenario: Un token caducado no vale
- **GIVEN** un token con `expires_at` en el pasado
- **WHEN** se usa en `Authorization: Bearer`
- **THEN** la petición SHALL contestar `401` con la envoltura

### Requirement: La gestión de tokens exige sesión, no un token

Las rutas `/api/v1/tokens` SHALL quedar detrás de la sesión OIDC. Un token de API NO SHALL poder crear, listar ni revocar tokens: `read` y `run` no cubren la administración de credenciales, y un token que pudiera emitir otro se convertiría en una vía de escalada. En modo `disabled` estas rutas SHALL quedar abiertas como el resto de la superficie.

#### Scenario: Un token no puede crear otro token
- **GIVEN** un token válido con scope `run`
- **WHEN** se pide `POST /api/v1/tokens` con `Authorization: Bearer`
- **THEN** SHALL contestar `401` o `403` con la envoltura
- **AND** NO SHALL crearse ningún token

#### Scenario: En modo disabled la gestión queda abierta
- **GIVEN** una configuración sin ninguna variable `OIDC_*`
- **WHEN** se pide `GET /api/v1/tokens` sin credencial
- **THEN** SHALL contestar `200` con el listado
- **AND** NO SHALL contestar `401`

### Requirement: La revocación surte efecto de inmediato

`DELETE /api/v1/tokens/{id}` SHALL marcar el token como revocado y SHALL dejar de aceptarlo en la siguiente petición, sin caché ni ventana de gracia. La fila NO SHALL borrarse: se conserva para poder auditar cuándo se emitió y cuándo se revocó. Revocar un token ya revocado SHALL ser idempotente y NO SHALL fallar.

#### Scenario: Un token revocado deja de funcionar en la petición siguiente
- **GIVEN** un token válido con scope `read` que ya ha respondido `200`
- **WHEN** se revoca con `DELETE /api/v1/tokens/{id}` y se repite la petición con el mismo token
- **THEN** la primera petición tras la revocación SHALL contestar `401`
- **AND** la fila SHALL seguir en la base con su fecha de revocación

#### Scenario: Revocar dos veces no es un error
- **GIVEN** un token ya revocado
- **WHEN** se pide `DELETE /api/v1/tokens/{id}` otra vez
- **THEN** SHALL contestar `200` con la envoltura y `data` nulo
- **AND** NO SHALL contestar `204`, porque un `204` no tiene cuerpo y rompería la envoltura
- **AND** el estado SHALL seguir siendo «revocado»

### Requirement: `Authorization: Bearer` convive con la cookie

La capa de autenticación SHALL aceptar, en cualquier ruta protegida de `/api/v1`, una sesión válida **o** un token válido. Si llegan las dos credenciales, SHALL preferir el token y NO SHALL rechazar la petición por la presencia de la otra. Sin ninguna de las dos, SHALL contestar `401` con la envoltura; con una credencial inválida, también `401`, sin distinguir en el mensaje si el token no existe, está revocado o ha caducado.

#### Scenario: Una petición con cookie sigue funcionando
- **GIVEN** una sesión OIDC válida en cookie y ninguna cabecera `Authorization`
- **WHEN** se pide `GET /api/v1/repositories`
- **THEN** SHALL contestar `200`
- **AND** el comportamiento SHALL ser el mismo que antes de este change

#### Scenario: Una petición con Bearer y sin cookie se acepta
- **GIVEN** un token válido con scope `read`
- **WHEN** se pide `GET /api/v1/repositories` con `Authorization: Bearer <token>` y sin cookie
- **THEN** SHALL contestar `200`
- **AND** NO SHALL exigirse sesión

#### Scenario: Bearer y cookie a la vez no se estorban
- **GIVEN** una sesión válida en cookie y un token válido con scope `read`
- **WHEN** se pide `GET /api/v1/repositories` con las dos credenciales
- **THEN** SHALL contestar `200`
- **AND** la identidad resuelta SHALL ser la del token

#### Scenario: Sin credencial se contesta 401 con la envoltura
- **GIVEN** una configuración con las tres variables `OIDC_*` y `KESTO_SECRET`
- **WHEN** se pide `GET /api/v1/repositories` sin cookie y sin `Authorization`
- **THEN** SHALL contestar `401` con la envoltura de error

### Requirement: Los scopes se aplican por ruta

Cada token SHALL llevar uno o más scopes de un conjunto cerrado: `read` y `run`. `read` SHALL cubrir las lecturas —salud, repositorios, targets, tareas, ejecuciones y snapshots— y `run` SHALL cubrir las lecturas **y** las acciones que disparan trabajo: lanzar una copia y comprobar la salud de un repositorio. Un token `read` que intente una acción de `run` SHALL recibir `403` con la envoltura; un token `run` SHALL poder con todo lo anterior. Las rutas de restauración y de borrado SHALL quedar fuera del alcance de cualquier token: no existe scope que las habilite.

La regla SHALL expresarse por ruta y por método, no por un `if` suelto en cada manejador, para que una ruta nueva no quede abierta por olvido.

#### Scenario: Un token read consulta todo
- **GIVEN** un token con scope `read`
- **WHEN** se piden `GET /api/v1/repositories`, `GET /api/v1/jobs`, `GET /api/v1/executions` y `GET /api/v1/snapshots`
- **THEN** todas SHALL contestar `200`

#### Scenario: Un token read no puede lanzar una copia
- **GIVEN** un token con scope `read`
- **WHEN** se pide `POST /api/v1/jobs/{id}/run`
- **THEN** SHALL contestar `403` con la envoltura
- **AND** NO SHALL crearse ninguna ejecución

#### Scenario: Un token run sí puede lanzar una copia
- **GIVEN** un token con scope `run`
- **WHEN** se pide `POST /api/v1/jobs/{id}/run`
- **THEN** SHALL contestar `202`
- **AND** SHALL crearse la ejecución

#### Scenario: Ningún token restaura ni borra
- **GIVEN** un token con scope `run`
- **WHEN** se pide `POST /api/v1/snapshots/{id}/restore` o `DELETE /api/v1/repositories/{id}`
- **THEN** SHALL contestar `403` con la envoltura
- **AND** NO SHALL ejecutarse la operación

### Requirement: En modo `disabled` no se exige nada

Con el modo `Disabled` —sin ninguna variable `OIDC_*`— NO SHALL exigirse token ni cookie: la superficie de `/api/v1` sigue abierta y las rutas de `/api/v1/tokens` también, como cualquier otra. La capa de tokens SHALL existir pero NO SHALL ponerse delante cuando no hay proveedor de identidad. Este modo es para desarrollo y no para un despliegue expuesto.

#### Scenario: En modo disabled una ruta protegida no pide token
- **GIVEN** una configuración sin ninguna variable `OIDC_*`
- **WHEN** se pide `GET /api/v1/repositories` sin cookie y sin `Authorization`
- **THEN** SHALL contestar `200`
- **AND** NO SHALL contestar `401`

#### Scenario: En modo disabled un Bearer inválido no bloquea
- **GIVEN** una configuración sin ninguna variable `OIDC_*`
- **WHEN** se pide `GET /api/v1/repositories` con `Authorization: Bearer basura`
- **THEN** SHALL contestar `200`
- **AND** NO SHALL contestar `401`
