# auth — delta

## MODIFIED Requirements

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

## ADDED Requirements

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
