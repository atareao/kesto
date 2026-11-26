## MODIFIED Requirements

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

## ADDED Requirements

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
