# auth — delta

## Purpose

El contrato de la autenticación de Kesto: el modo, el flujo OIDC, la sesión y su cierre. Este delta hace que la cookie de sesión lleve `Secure` siempre y acota la ruta pública del login.

## REMOVED Requirements

### Requirement: La sesión se emite como cookie y no como cabecera

**Reason**: el atributo `Secure` deja de depender del esquema de la URL de retorno y pasa a ser incondicional, y el retorno por `http` deja de ser admisible. El escenario que ligaba el atributo al esquema deja de ser válido, y un bloque MODIFIED no puede perder escenarios, así que el requisito se sustituye por su versión endurecida.

**Migration**: ninguna en el cliente, que no cambia su forma de entrar. El despliegue que hoy declare `OIDC_REDIRECT_URL` por `http` —o que no la declare— tiene que declararla por `https` o el arranque falla; ver la capability `config`.

## ADDED Requirements

### Requirement: La sesión se emite como cookie, siempre con `Secure`

El callback SHALL emitir la sesión como una cookie con el JWT firmado, y NO SHALL devolver el token en el cuerpo ni esperar que el cliente lo guarde. Es la razón exacta de la divergencia con shuul, que responde una página HTML con `sessionStorage.setItem`: eso sirve para un panel que hace *polling*, no para uno que abre una `EventSource`, que no puede enviar cabeceras.

La cookie SHALL ser `HttpOnly`, `SameSite=Strict`, `Path=/`, `Secure` y llevar `Max-Age` igual a la vida de la sesión. El atributo `Secure` SHALL añadirse **siempre**, sin depender del esquema de la URL de retorno: la cookie **es** la credencial de sesión, vive ocho horas y viaja en cada petición protegida, así que dejarla viajar en claro es justo lo que el atributo impide. Que el despliegue entre por `https` SHALL ser una condición del **arranque** —la URL de retorno es obligatoria y `https` en modo OIDC, ver la capability `config`— y NO SHALL decidirse al construir la respuesta: un aviso que arranca igual deja la credencial en claro.

La vida de la sesión SHALL ser una constante del módulo —8 horas— y NO SHALL ser una variable de entorno nueva: el panel es de un solo operador y una sesión que caduque en mitad de una copia obliga a rehacer el flujo, y una constante evita inventar configuración que nadie ha pedido.

El JWT SHALL firmarse con `HS256` y con `KESTO_SECRET`, y SHALL llevar exactamente `sub`, `email`, `iat` y `exp`. NO SHALL llevar roles: el SRS no define ni uno y esa superficie no se inventa.

Tras emitir la cookie, el callback SHALL contestar una redirección `302` al panel.

#### Scenario: Los atributos de la cookie
- **GIVEN** un usuario que completa el flujo de entrada
- **WHEN** el callback emite la sesión
- **THEN** la `Set-Cookie` SHALL traer `HttpOnly`
- **AND** SHALL traer `SameSite=Strict`, `Path=/` y `Secure`
- **AND** SHALL traer un `Max-Age` igual a la vida de la sesión

#### Scenario: El `Secure` no depende del esquema de retorno
- **GIVEN** un proceso en modo `Oidc`
- **WHEN** se construye la cookie de sesión
- **THEN** la cookie SHALL llevar `Secure`
- **AND** el atributo NO SHALL depender del esquema de ninguna URL de retorno

#### Scenario: Los claims de la sesión son los cuatro, sin roles
- **GIVEN** una cookie de sesión emitida por el callback
- **WHEN** se decodifica su JWT
- **THEN** sus claims SHALL ser `sub`, `email`, `iat` y `exp`
- **AND** NO SHALL contener ningún claim de rol

#### Scenario: La vida de la sesión es la constante del módulo
- **GIVEN** una cookie de sesión emitida por el callback
- **WHEN** se comparan su `Max-Age` y su `exp` con la constante de vida de la sesión
- **THEN** los tres SHALL coincidir
- **AND** la vida SHALL ser de 8 horas

#### Scenario: Tras el callback se vuelve al panel
- **GIVEN** un flujo de entrada completado con éxito
- **WHEN** se atiende el callback
- **THEN** SHALL contestar `302`

### Requirement: El login está limitado en tasa y el mapa de logins tiene tope

`GET /api/v1/auth/login` SHALL atender como máximo `MAX_PETICIONES_DE_LOGIN` peticiones por IP y por `VENTANA_DE_LOGIN`, y SHALL contestar `429` con la envoltura `{status, message}` a la que exceda, sin guardar su `state` en el mapa. La IP SHALL tomarse del **socket** de la conexión y NO SHALL tomarse de ninguna cabecera de la petición, que el cliente puede falsificar.

El mapa de logins pendientes SHALL tener un tope duro (`MAX_LOGINS`): cuando una inserción lo alcanza, SHALL purgarse primero lo caducado y, si sigue lleno, SHALL desalojarse la entrada **más antigua** por su marca de tiempo. NO SHALL rechazarse el login nuevo por estar el mapa lleno: rechazarlo convertiría el tope en una denegación de servicio de disponibilidad.

El motivo: la ruta es pública y cada petición escribe en memoria del proceso. Sin límite de tasa ni tope, un caudal alto y no autenticado hace crecer el mapa dentro de la ventana del `state` sin más freno que la caducidad, y la caducidad no acota el tamaño —solo lo acota el caudal—.

#### Scenario: Pasado el cupo, la petición contesta 429
- **GIVEN** una IP que ya ha agotado `MAX_PETICIONES_DE_LOGIN` dentro de la ventana
- **WHEN** pide `GET /api/v1/auth/login`
- **THEN** SHALL contestar `429` con la envoltura `{status, message}`
- **AND** NO SHALL guardarse un `state` nuevo en el mapa

#### Scenario: Dentro del cupo, el login sigue redirigiendo
- **GIVEN** una IP dentro de su cupo
- **WHEN** pide `GET /api/v1/auth/login`
- **THEN** SHALL contestar `302` al proveedor

#### Scenario: Pasada la ventana, el cupo se renueva
- **GIVEN** una IP que agotó su cupo y una ventana ya vencida
- **WHEN** vuelve a pedir `GET /api/v1/auth/login`
- **THEN** SHALL contestar `302`

#### Scenario: El tope del mapa desaloja la entrada más antigua
- **GIVEN** un mapa de logins con `MAX_LOGINS` entradas vigentes y una inserción nueva
- **WHEN** se registra el login
- **THEN** el mapa SHALL seguir teniendo `MAX_LOGINS` entradas
- **AND** la desalojada SHALL ser la de menor marca de tiempo
- **AND** la entrada nueva SHALL estar presente

#### Scenario: La purga por caducidad va antes del desalojo
- **GIVEN** un mapa con entradas caducadas y entradas vigentes hasta el tope
- **WHEN** se registra un login
- **THEN** las caducadas NO SHALL contarse para el tope
- **AND** NO SHALL desalojarse ninguna entrada vigente mientras la purga deje hueco

#### Scenario: La IP sale del socket y no de una cabecera
- **GIVEN** dos peticiones con la misma cabecera `X-Forwarded-For` y sockets distintos
- **WHEN** se atienden
- **THEN** SHALL contarse como dos IP distintas
