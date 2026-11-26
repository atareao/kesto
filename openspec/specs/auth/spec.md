# auth Specification

## Purpose
La puerta de entrada: cómo Kesto decide quién eres antes de dejarte tocar nada. El flujo de entrada contra un proveedor OIDC externo —descubrimiento del emisor, `state` y `nonce`, canje del código y verificación del ID token contra el JWKS—, la sesión como JWT firmado que viaja en una cookie `HttpOnly` y se verifica por firma en cada petición protegida, y el modo de desarrollo, que no pone capa ninguna. La identidad se delega en el proveedor a propósito: Kesto no guarda contraseñas ni administra usuarios, y el día que esto se despliegue sobre una red que no sea de confianza, esta es la única defensa que hay. El requisito que la recorre de arriba abajo es el último: ninguna credencial del proveedor puede acabar en el log ni en la cookie.

## Requirements

### Requirement: El modo de autenticación decide si hay capa

Con el modo `Disabled` la aplicación NO SHALL montar capa de autenticación alguna: la superficie queda pública, como hoy, y las rutas de `/api/v1/auth/*` NO SHALL declararse —sin proveedor no hay flujo que ofrecer—, de modo que contestan el `404` de la envoltura en vez de un `401`.

Con el modo `Oidc` toda ruta bajo `/api/v1` SHALL exigir una sesión válida, y SHALL ser pública **solo** la salud y las dos rutas del flujo de entrada. La regla SHALL expresarse como una lista blanca de rutas públicas, no como una lista negra de rutas protegidas: así el stream SSE de `/api/v1/events` queda del lado protegido por construcción, sin que nadie tenga que acordarse cuando llegue el change que lo declara.

Que el SSE quede detrás del *gate* es deliberado: lleva el progreso de las copias y no lo ve quien no ha entrado. Y es viable precisamente porque la sesión va en cookie —la `EventSource` del navegador no permite enviar cabeceras, pero manda la cookie sola al ser mismo origen—.

#### Scenario: En modo desarrollo no hay capa de autenticación y todo es público
- **GIVEN** una configuración sin ninguna variable `OIDC_*`
- **WHEN** se pide `GET /api/v1/auth/session` sin cookie
- **THEN** NO SHALL contestar `401`
- **AND** la ruta SHALL contestar `404` con la envoltura, porque el flujo no está montado

#### Scenario: En modo OIDC una ruta protegida sin cookie contesta 401 con la envoltura
- **GIVEN** una configuración con las tres variables `OIDC_*` y `KESTO_SECRET`
- **WHEN** se pide `GET /api/v1/auth/session` sin cookie
- **THEN** SHALL contestar `401`
- **AND** el cuerpo SHALL ser JSON con `status` y `message`, y NO SHALL estar vacío

#### Scenario: La salud sigue siendo pública con la autenticación encendida
- **GIVEN** una configuración en modo `Oidc`
- **WHEN** se pide `GET /api/v1/health` sin cookie
- **THEN** SHALL contestar `200`

#### Scenario: La superficie pública es una lista blanca
- **GIVEN** el router en modo `Oidc`
- **WHEN** se enumeran las rutas que no exigen sesión
- **THEN** SHALL ser exactamente `/api/v1/health`, `/api/v1/auth/login` y `/api/v1/auth/callback`
- **AND** `/api/v1/events` NO SHALL estar entre ellas

### Requirement: El arranque no depende de que el proveedor de identidad esté vivo

La aplicación SHALL intentar el descubrimiento del proveedor al arrancar —`GET {issuer}/.well-known/openid-configuration`— y, si el proveedor no contesta, SHALL arrancar igualmente con un aviso que declare el fallo. El descubrimiento SHALL reintentarse en el primer login y su resultado SHALL cachearse, para no repetir la petición en cada intento de entrada.

Del documento de descubrimiento SHALL usarse `authorization_endpoint`, `token_endpoint` y `jwks_uri`, y NO SHALL exigirse `userinfo_endpoint`.

Las claves del proveedor se obtienen del `jwks_uri` a través de una **caché con TTL de una hora**, servida desde `AuthState`. El JWKS descargado se reutiliza entre verificaciones mientras no haya caducado. Si al verificar un ID token llega un `kid` que la caché no conoce, se forza **una recarga** del JWKS antes de rechazar; si tras recargar el `kid` sigue ausente, la verificación falla y NO SHALL emitir cookie de sesión. Una recarga fallida con una caché fresca válida NO SHALL tumbar una verificación que la caché pueda servir: la verificación SHALL usar el JWKS ya cacheado.

El motivo se deja escrito para que nadie lo «arregle» mañana: Kesto es el orquestador de las **copias**, y negarse a arrancar porque el proveedor de identidad está caído convertiría un problema de autenticación en ausencia de copias. Es el mismo criterio por el que una credencial débil avisa y arranca.

#### Scenario: Un proveedor caído no impide arrancar
- **GIVEN** un emisor configurado al que nada contesta
- **WHEN** la aplicación arranca
- **THEN** SHALL arrancar y contestar la salud
- **AND** SHALL emitir un aviso que declare que el descubrimiento falló

#### Scenario: El primer login reintenta el descubrimiento y lo cachea
- **GIVEN** un arranque cuyo descubrimiento falló
- **WHEN** se pide `GET /api/v1/auth/login`
- **THEN** SHALL intentar el descubrimiento otra vez
- **AND** el resultado SHALL quedar cacheado, de modo que un segundo login no repita la petición

#### Scenario: Un `kid` desconocido fuerza una recarga del JWKS
- **GIVEN** un JWKS cacheado sin la clave para un `kid` de un ID token válido
- **WHEN** se atiende el callback con ese ID token
- **THEN** SHALL recargar el JWKS una vez
- **AND** si el `kid` aparece tras recargar, SHALL verificar el ID token y emitir sesión
- **AND** si el `kid` sigue ausente tras recargar, SHALL contestar un error con la envoltura y NO SHALL emitir cookie de sesión

#### Scenario: Una recarga fallida con caché fresca no tumba la verificación
- **GIVEN** un JWKS cacheado válido con una clave que firma un ID token en regla, y el `jwks_uri` inalcanzable
- **WHEN** se atiende el callback con ese ID token
- **THEN** SHALL usar el JWKS cacheado
- **AND** SHALL emitir la cookie de sesión

### Requirement: El login redirige al proveedor y no emite sesión

`GET /api/v1/auth/login` SHALL ser público y SHALL contestar una redirección `302` al `authorization_endpoint` del proveedor con los parámetros `response_type=code`, `client_id`, `redirect_uri`, `scope` —que SHALL incluir `openid` y `email`, porque el `email` de la sesión es un claim del ID token y la mayoría de los proveedores solo lo publican con ese scope—, `state` y `nonce`. Los valores añadidos a la URL SHALL ir codificados. La petición NO SHALL emitir cookie de sesión.

`state` y `nonce` SHALL ser de 32 caracteres aleatorios cada uno y distintos en cada petición. El `state` SHALL guardarse en memoria con su marca de tiempo; el `nonce` SHALL viajar en la petición de autorización para compararse al verificar el ID token.

Si el descubrimiento no está cacheado y su reintento también falla, la petición SHALL contestar `502` con la envoltura `{status, message}`, y el mensaje NO SHALL publicar el `client_secret`.

El código SHALL ser `502` y no un `500` genérico: el fallo es de un servicio de terceros —el proveedor de identidad no contesta—, no de Kesto, y el operador que lee el log tiene que ir a mirar al proveedor en vez de a buscar un fallo interno. Es la variante `BadGateway` del modelo de error.

Como `502` es un `5xx`, al cuerpo de esta respuesta le rige entera la regla del requisito «Un error interno no cuenta por qué falló» de `api-shell`: el mensaje SHALL ser **genérico** y NO SHALL incluir el texto de la causa, que SHALL registrarse en el log del servidor. NO SHALL estrenarse un mensaje explicativo que nombre al proveedor de identidad: el cuerpo no es el sitio donde el operador se entera de nada.

#### Scenario: La redirección lleva los seis parámetros
- **GIVEN** una configuración en modo `Oidc`
- **WHEN** se pide `GET /api/v1/auth/login`
- **THEN** SHALL contestar `302`
- **AND** la cabecera `Location` SHALL apuntar al `authorization_endpoint` y traer `response_type=code`, `client_id`, `redirect_uri`, `scope`, `state` y `nonce`

#### Scenario: Cada login usa un `state` y un `nonce` nuevos
- **GIVEN** dos peticiones consecutivas a `GET /api/v1/auth/login`
- **WHEN** se comparan sus `Location`
- **THEN** el `state` de una NO SHALL coincidir con el de la otra
- **AND** el `state` y el `nonce` SHALL tener 32 caracteres cada uno

#### Scenario: El login no establece sesión
- **GIVEN** una petición a `GET /api/v1/auth/login`
- **WHEN** se atiende
- **THEN** la respuesta NO SHALL traer una `Set-Cookie` de sesión

#### Scenario: El proveedor caído deja el login en un error con envoltura
- **GIVEN** un proveedor al que nada contesta y cuyo descubrimiento ya falló
- **WHEN** se pide `GET /api/v1/auth/login`
- **THEN** SHALL contestar `502` con la envoltura `{status, message}`
- **AND** el mensaje NO SHALL contener el `client_secret`
- **AND** el mensaje SHALL ser genérico y el cuerpo NO SHALL llevar el texto de la causa

### Requirement: El callback consume el `state` una sola vez y comprueba su caducidad

`GET /api/v1/auth/callback` SHALL ser público. SHALL consumir el `state` recibido —extraerlo del mapa y descartarlo—, y SHALL rechazar la petición si el `state` es desconocido, si ya se consumió o si han transcurrido más de 5 minutos desde que se emitió.

La caducidad SHALL comprobarse **de verdad** al consumir, no solo registrarse: es el defecto de shuul que este change corrige —guarda la marca de tiempo y nunca la mira, así que el TTL de cinco minutos que anuncia su comentario no se aplica—.

Si el proveedor devuelve un `error` en la consulta, el callback SHALL rechazar la petición sin canjear el código.

Todo rechazo SHALL contestar la envoltura y NO SHALL emitir cookie de sesión.

#### Scenario: Un `state` desconocido se rechaza
- **GIVEN** un callback con un `state` que no se emitió
- **WHEN** se atiende
- **THEN** SHALL contestar un error con la envoltura
- **AND** NO SHALL emitir cookie de sesión

#### Scenario: Un `state` caducado se rechaza
- **GIVEN** un `state` emitido hace más de 5 minutos
- **WHEN** llega el callback con ese `state`
- **THEN** SHALL contestar un error con la envoltura
- **AND** NO SHALL emitir cookie de sesión

#### Scenario: Un `state` reutilizado se rechaza
- **GIVEN** un `state` ya consumido por un callback anterior
- **WHEN** llega un segundo callback con el mismo `state`
- **THEN** SHALL contestar un error con la envoltura
- **AND** NO SHALL emitir cookie de sesión

#### Scenario: Un error del proveedor no llega a canjear nada
- **GIVEN** un callback con `error=access_denied` y sin `code`
- **WHEN** se atiende
- **THEN** SHALL contestar un error con la envoltura
- **AND** NO SHALL llamarse al `token_endpoint`

### Requirement: El ID token se verifica antes de emitir sesión

Tras canjear el código en el `token_endpoint`, la aplicación SHALL verificar el ID token: firma `RS256` contra las claves publicadas en el `jwks_uri` (servidas desde la **caché JWKS con TTL**), `iss` igual al emisor configurado, `aud` igual al `client_id` configurado, `exp` e `iat` en vigor y `nonce` igual al emitido en el login.

Un token que falle cualquiera de las cinco comprobaciones NO SHALL aceptarse y NO SHALL producir cookie de sesión. Es el segundo defecto de shuul que aquí se corrige: shuul canjea el código y va directo a `userinfo`, sin verificar el ID token que acaba de recibir.

La comprobación de `iat` SHALL tolerar un desfase de reloj de hasta **60 segundos**, el valor de una constante del módulo: el `iat` del ID token lo firma el reloj del proveedor y lo compara el de Kesto —dos relojes—, y va redondeado al segundo, así que sin margen un desfase de un segundo tumba un login por lo demás en regla con un `401` genérico. El `exp` NO SHALL llevar margen: el de un ID token va una hora visto, un desfase de segundos no lo alcanza, y tolerarlo alargaría la vida de un token recién caducado. Más allá del margen, un `iat` adelantado al reloj SHALL rechazarse.

El `nonce` del ID token SHALL compararse con el emitido en el login usando **comparación en tiempo constante** (`subtle::ct_eq`), sin cambiar la semántica: mismo rechazo si no coincide.

Del token **verificado** SHALL tomarse `sub` y `email` como identidad de la sesión. Como consecuencia, NO SHALL llamarse al endpoint `userinfo`: con el ID token verificado sobra, y un documento de descubrimiento sin `userinfo_endpoint` SHALL bastar para completar el flujo.

#### Scenario: Un `aud` ajeno no se acepta
- **GIVEN** un ID token firmado correctamente pero con `aud` distinto del `client_id`
- **WHEN** se atiende el callback
- **THEN** SHALL contestar un error con la envoltura
- **AND** NO SHALL emitir cookie de sesión

#### Scenario: Un token caducado no se acepta
- **GIVEN** un ID token con `exp` en el pasado
- **WHEN** se atiende el callback
- **THEN** SHALL contestar un error con la envoltura
- **AND** NO SHALL emitir cookie de sesión

#### Scenario: Un desfase de reloj dentro del margen no tumba el login
- **GIVEN** un ID token en regla salvo su `iat`, adelantado al reloj de Kesto dentro de los 60 segundos de margen
- **WHEN** se atiende el callback
- **THEN** SHALL completar el flujo, con `302` y cookie de sesión

#### Scenario: Un `nonce` distinto no se acepta
- **GIVEN** un ID token cuyo `nonce` no es el emitido en el login
- **WHEN** se atiende el callback
- **THEN** SHALL contestar un error con la envoltura
- **AND** NO SHALL emitir cookie de sesión

#### Scenario: Una firma fuera del JWKS no se acepta
- **GIVEN** un ID token firmado con una clave que el `jwks_uri` no publica
- **WHEN** se atiende el callback
- **THEN** SHALL contestar un error con la envoltura
- **AND** NO SHALL emitir cookie de sesión

#### Scenario: Un `iss` distinto del emisor configurado no se acepta
- **GIVEN** un ID token con `iss` distinto del `OIDC_ISSUER_URL` configurado
- **WHEN** se atiende el callback
- **THEN** SHALL contestar un error con la envoltura
- **AND** NO SHALL emitir cookie de sesión

#### Scenario: Un token verificado se convierte en sesión
- **GIVEN** un ID token válido con `sub` y `email` conocidos
- **WHEN** se atiende el callback
- **THEN** SHALL emitir la cookie de sesión
- **AND** el `sub` y el `email` de la sesión SHALL ser los del token

#### Scenario: El flujo no depende de `userinfo`
- **GIVEN** un documento de descubrimiento que no publica `userinfo_endpoint`
- **WHEN** se completa el flujo de entrada
- **THEN** SHALL completarse igualmente
- **AND** NO SHALL pedirse ningún `userinfo`

### Requirement: La sesión se verifica contra `KESTO_SECRET` en cada petición protegida

El *gate* de la superficie protegida SHALL extraer la cookie de sesión y verificar su firma con `KESTO_SECRET`, con el algoritmo del emisor, y SHALL comprobar que `exp` está en vigor **y que el `jti` de la sesión no está revocado**. Una cookie **ausente**, **mal firmada**, **firmada con otra clave**, **caducada** o **revocada** NO SHALL aceptarse.

El rechazo SHALL ser un `401` con la envoltura `{status, message}` del change anterior, y NO SHALL ser un cuerpo vacío ni una redirección a una página de entrada: la API contesta JSON y es el panel quien decide qué hacer.

Cuando la sesión es válida, su identidad SHALL quedar disponible para el manejador que la pida.

El porqué de la revocación: la sesión es un JWT sin estado, así que su firma sola no permite cerrarla antes de que `exp` llegue. La revocación se guarda en la base por el `jti` de la sesión, así que una comprobación más —la misma clase de consulta que ya hace el camino del token— es lo que hace efectivo el cierre de sesión.

#### Scenario: Sin cookie no se pasa
- **GIVEN** una petición a una ruta protegida sin cookie
- **WHEN** se atiende
- **THEN** SHALL contestar `401`
- **AND** el cuerpo SHALL ser JSON con `status` y `message`

#### Scenario: Una cookie firmada con otra clave no se acepta
- **GIVEN** una cookie con un JWT bien formado pero firmado con una clave distinta de `KESTO_SECRET`
- **WHEN** se pide una ruta protegida
- **THEN** SHALL contestar `401` con la envoltura

#### Scenario: Una cookie caducada no se acepta
- **GIVEN** una cookie de sesión con `exp` en el pasado
- **WHEN** se pide una ruta protegida
- **THEN** SHALL contestar `401` con la envoltura

#### Scenario: Una cookie manipulada no se acepta
- **GIVEN** una cookie de sesión cuyo contenido se ha alterado sin volver a firmarla
- **WHEN** se pide una ruta protegida
- **THEN** SHALL contestar `401` con la envoltura

#### Scenario: Una cookie revocada no se acepta
- **GIVEN** una cookie de sesión bien firmada y en vigor cuyo `jti` está en la tabla de sesiones revocadas
- **WHEN** se pide una ruta protegida
- **THEN** SHALL contestar `401` con la envoltura

#### Scenario: Una cookie válida deja pasar
- **GIVEN** una cookie de sesión emitida por el propio Kesto y en vigor
- **WHEN** se pide una ruta protegida
- **THEN** SHALL contestar `200`
- **AND** el manejador SHALL ver la identidad de la sesión

#### Scenario: El rechazo no es una redirección
- **GIVEN** una petición sin cookie a una ruta protegida
- **WHEN** se atiende
- **THEN** la respuesta NO SHALL ser `3xx`
- **AND** NO SHALL traer cabecera `Location`

### Requirement: El cierre de sesión borra la cookie

`POST /api/v1/auth/logout` SHALL exigir sesión, SHALL borrar la cookie de sesión —misma cookie, mismo `Path` y `Max-Age=0`— y SHALL contestar la envoltura. SHALL declararse como `POST` y NO SHALL declararse como `GET`: cambia estado, y un `GET` lo dispararía un enlace, un prefetch o una imagen.

Que `GET /api/v1/auth/logout` conteste `405` **solo** con una sesión válida es consecuencia del *gate*, que decide antes que el enrutado por método (`api-shell`): sin sesión la respuesta es `401`, y quien no ha entrado no aprende nada. El `405` se mantiene porque un cliente que **sí** ha entrado merece saber que el método es otro.

#### Scenario: El cierre borra la cookie
- **GIVEN** una petición con una cookie de sesión válida
- **WHEN** se pide `POST /api/v1/auth/logout`
- **THEN** SHALL contestar `200` con la envoltura
- **AND** la respuesta SHALL traer una `Set-Cookie` que deja la cookie sin valor y con `Max-Age=0`

#### Scenario: Sin sesión el cierre no se atiende
- **WHEN** se pide `POST /api/v1/auth/logout` sin cookie
- **THEN** SHALL contestar `401` con la envoltura

#### Scenario: El cierre no se declara como `GET`
- **GIVEN** una petición con una cookie de sesión válida
- **WHEN** se pide `GET /api/v1/auth/logout`
- **THEN** SHALL contestar `405` con la envoltura

#### Scenario: Sin sesión no se aprende qué métodos hay
- **GIVEN** una petición sin cookie
- **WHEN** se pide `GET /api/v1/auth/logout`
- **THEN** SHALL contestar `401` con la envoltura
- **AND** NO SHALL contestar `405`

### Requirement: `GET /api/v1/auth/session` contesta quién es el usuario

`GET /api/v1/auth/session` SHALL exigir sesión y SHALL contestar la envoltura con la identidad de la sesión —su `sub` y su `email`—, sin devolver el JWT.

Sin cookie válida, el `401` del *gate* **es** la respuesta a «¿hay sesión?»: NO SHALL añadirse un `200` con un booleano, ni un endpoint aparte que diga si la autenticación está encendida —el modo NO se publica como metadato; se comprueba por comportamiento: `401` en OIDC frente a `404` en `Disabled`—.

#### Scenario: Con sesión se contesta la identidad
- **GIVEN** una cookie de sesión válida
- **WHEN** se pide `GET /api/v1/auth/session`
- **THEN** SHALL contestar `200` con la envoltura
- **AND** el cuerpo SHALL traer el `sub` y el `email` de la sesión

#### Scenario: Sin sesión, el 401 es la respuesta
- **WHEN** se pide `GET /api/v1/auth/session` sin cookie
- **THEN** SHALL contestar `401` con la envoltura

### Requirement: Ninguna credencial del proveedor acaba en el log ni en la cookie

Ni el `client_secret`, ni el código de autorización, ni el ID token, ni el token de acceso del proveedor SHALL aparecer en el log a ningún nivel de `RUST_LOG`, ni en el cuerpo de una respuesta, ni en una URL de redirección —más allá de lo que el propio proveedor exige para volver—.

El caso concreto es el callback: llega con `?code=` y `?state=` en la cadena de consulta, y una traza de peticiones que registre la URI completa los publicaría. Cuando llegue esa traza, su span NO SHALL incluir la cadena de consulta; queda fijado aquí para que no se descubra en producción.

La cookie de sesión SHALL llevar únicamente el JWT que emite Kesto: NO SHALL llevar el ID token ni el token de acceso del proveedor. El `client_secret` SHALL seguir siendo un `SecretString`, de modo que el cumplimiento no dependa de un cuidado manual en cada llamada de log.

#### Scenario: El flujo completo no deja credenciales en el log
- **GIVEN** `RUST_LOG=trace` y un proveedor falso que devuelve un ID token y un token de acceso distinguibles
- **WHEN** se completa el flujo de entrada —login, callback y una petición con la sesión—
- **THEN** ninguna línea del log SHALL contener el `client_secret`, ni el código de autorización, ni el ID token, ni el token de acceso

#### Scenario: La cookie no lleva el token del proveedor
- **GIVEN** la cookie emitida tras un callback correcto
- **WHEN** se decodifica su JWT
- **THEN** NO SHALL contener el ID token
- **AND** NO SHALL contener el token de acceso del proveedor

#### Scenario: La vuelta al panel no arrastra el código
- **GIVEN** un callback atendido con éxito
- **WHEN** se inspecciona la `Location` de la respuesta
- **THEN** NO SHALL contener el código de autorización

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

### Requirement: El cierre de sesión revoca la sesión además de borrar la cookie

`POST /api/v1/auth/logout` SHALL, además de borrar la cookie, revocar la sesión ante el servidor: SHALL registrar el `jti` de la cookie que se cierra entre las sesiones revocadas, de modo que esa cookie deje de servir aunque se conserve, hasta su caducidad.

El cierre SHALL apoyarse en la sesión que el *gate* ya verificó —la ruta está tras el *gate*, así que sin sesión no se llega al manejador—: SHALL revocar el `jti` de esa sesión verificada antes de responder.

#### Scenario: La cookie cerrada deja de servir
- **GIVEN** una sesión válida y su cookie
- **WHEN** se cierra sesión y después se reutiliza esa misma cookie en una ruta protegida
- **THEN** la reutilización SHALL contestar `401` con la envoltura

#### Scenario: El cierre sigue devolviendo la cookie de borrado
- **GIVEN** una petición con una cookie de sesión válida
- **WHEN** se pide `POST /api/v1/auth/logout`
- **THEN** SHALL contestar `200` con la envoltura
- **AND** la respuesta SHALL traer una `Set-Cookie` que deja la cookie sin valor y con `Max-Age=0`

### Requirement: El modelo de identidad es de un solo operador y no tiene roles

El sistema SHALL asumir un único operador de confianza: NO SHALL existir autorización por rol ni por scope para la sesión, y toda ruta protegida SHALL quedar disponible para cualquier sesión válida. La identidad de la sesión SHALL ser solo la del proveedor (`sub`, `email`); el JWT NO SHALL admitir claims de autorización.

El día que aparezcan varios operadores, esta frontera SHALL ampliarse con un modelo de autorización; hasta entonces, la ausencia de roles es deliberada y no un hueco que llenar por defecto.

#### Scenario: Una sesión válida alcanza cualquier ruta protegida
- **GIVEN** una sesión válida de cualquier identidad
- **WHEN** se pide cualquier ruta protegida
- **THEN** SHALL contestar `200`
- **AND** NO SHALL exigirse ningún rol ni scope de sesión

#### Scenario: Un claim de autorización no cabe en la sesión
- **GIVEN** una cookie con un JWT que añade un claim de rol
- **WHEN** se pide una ruta protegida
- **THEN** SHALL contestar `401` con la envoltura

### Requirement: PKCE S256 en el flujo de login y canje

El flujo de entrada SHALL usar **PKCE con `S256`**: `GET /api/v1/auth/login` SHALL añadir `code_challenge` y `code_challenge_method=S256` a la redirección al `authorization_endpoint`. El `code_challenge` SHALL ser `base64url(sin relleno)` del SHA-256 de un `code_verifier` generado por login (43-128 caracteres de alta entropía).

El `code_verifier` SHALL guardarse junto al `state` y al `nonce` del login pendiente en el mapa de `logins` de `AuthState`, sin cambiar el modelo del mapa (solo se añade un campo más al registro de login pendiente).

Al canjear el código en el `token_endpoint`, la petición POST SHALL incluir `code_verifier` con el valor guardado. Un login que no tenga un `code_verifier` generado NO SHALL emitirse: cada login nuevo SHALL llevar su propio verifier.

#### Scenario: La redirección de login lleva `code_challenge` y `code_challenge_method=S256`
- **GIVEN** una configuración en modo `Oidc`
- **WHEN** se pide `GET /api/v1/auth/login`
- **THEN** SHALL contestar `302`
- **AND** la cabecera `Location` SHALL apuntar al `authorization_endpoint` y traer `code_challenge` y `code_challenge_method=S256`
- **AND** el `code_challenge` SHALL ser `base64url(sin relleno)` del SHA-256 de un `code_verifier` de 43-128 caracteres

#### Scenario: El canje del código envía `code_verifier`
- **GIVEN** un login pendiente con su `code_verifier` guardado y un código de autorización válido
- **WHEN** se atiende `GET /api/v1/auth/callback` con ese código
- **THEN** la petición POST al `token_endpoint` SHALL incluir `code_verifier` con el valor guardado
- **AND** si el canje es válido, SHALL verificar el ID token y emitir sesión

#### Scenario: Un login sin `code_verifier` no se emite
- **GIVEN** un registro de login pendiente sin `code_verifier` (estado anterior al change)
- **WHEN** se intenta canjear su código
- **THEN** NO SHALL emitirse sesión
- **AND** SHALL contestar un error con la envoltura
