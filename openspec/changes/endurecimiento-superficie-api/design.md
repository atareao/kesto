# Design — endurecimiento de la superficie de la API

## D1 · La cookie lleva `Secure` siempre, y el retorno tiene que ser `https`

`Secure` se desacopla del retorno: en modo `Oidc` la cookie se emite **siempre** con `Secure`, y quien decide si el despliegue es admisible es el **arranque**, no el atributo de una respuesta. Se descarta seguir condicionando el atributo y añadir un aviso: la cookie **es** la sesión, vive ocho horas y viaja en cada petición protegida, así que un aviso que arranca igual deja la credencial viajando en claro. Es el mismo argumento que en el bloque A convirtió una credencial débil en puerta de arranque.

No hay válvula de escape, ni siquiera la que existe para el modo sin autenticación. `KESTO_ALLOW_INSECURE` gobierna «sin autenticación», que es un modo deliberado y acotado a loopback; admitir «con autenticación pero por `http`» sería admitir que la sesión viaje en claro, y eso no tiene uso legítimo en un despliegue. Un laboratorio con OIDC por `http` deja de ser posible **a propósito**.

## D2 · `OIDC_REDIRECT_URL` es obligatoria y `https` en modo OIDC, y su defecto desaparece

El defecto `http://localhost:8080/api/v1/auth/callback` (`src/config.rs:89`) no puede sobrevivir al cambio: en modo OIDC no hay retorno admisible que no sea `https`, y el retorno **solo** se usa en modo OIDC. Un valor por defecto que nunca arranca es un valor muerto, y además produce un mensaje peor —«no es `https`» cuando el problema real es «no la has declarado»—.

Así que la regla es: en modo `Oidc`, `OIDC_REDIRECT_URL` **ausente** es error que la nombra, y **presente** con esquema distinto de `https` es error que la nombra. La comprobación vive **dentro** de `resolve_auth` (`src/config.rs:508-538`), que es donde ya se decide el modo, así que no hace falta un campo opcional ni un orden nuevo de lectura: `OidcSettings.redirect_url` sigue siendo `String`, solo que ahora garantiza el esquema. Se descarta mantener el defecto y limitarse a comprobar `https`: dejaría un valor por defecto que jamás puede usarse, y el operador que no declara nada merece que se le diga qué falta, no que se le hable de un esquema.

Sin las tres `OIDC_*` la variable sigue ignorándose en silencio, que es el comportamiento ya documentado (`docs/ENTORNO.md:31`): no hay modo OIDC al que aplicarle la regla.

**Coste aceptado**: un despliegue OIDC que no declare la variable deja de arrancar. Va en la misma dirección que el bloque A —el despliegue de referencia ya exige las tres `OIDC_*` con `${VAR:?}`— y el retorno es la cuarta pieza sin la que el flujo no funciona. `compose.yaml`, el smoke test y `docs/ENTORNO.md` lo declaran, y el aviso de compatibilidad va en la nota de actualización.

## D3 · El límite de tasa: ventana fija por IP, en memoria del proceso, sin dependencias nuevas

Se implementa con un **módulo propio y puro** (`src/auth/limite.rs`) y no con una crate: el repositorio no tiene nada de limitación de tasa, y este caso es una ventana fija por IP, que cabe en unas decenas de líneas con su reloj inyectado. Añadir `governor`/`tower-governor` metería un árbol de dependencias nuevo para una regla que se lee de un tirón.

- **Ventana fija** y no cubo de fichas: la ventana fija se explica en una frase y se prueba con un reloj inyectado, sin dormir. El cubo suaviza mejor el caudal, pero su estado —fichas y marca— es más difícil de justificar y de auditar, y aquí no se busca suavizar sino **acotar**.
- **Por IP** y no global: un límite global convertiría el propio límite en una denegación de servicio —un atacante lo agota y deja fuera a los usuarios legítimos—. Por IP, el daño se acota a quien abusa.
- La IP sale del **socket** (`ConnectInfo<SocketAddr>`) y nunca de una cabecera. `X-Forwarded-For` es falsificable por el cliente y, sin declarar un proxy de confianza, honrarla es regalar el límite: basta variar la cabecera para estrenar IP en cada petición. Kesto no se despliega detrás de un proxy de confianza por diseño, así que no se añade la variable que lo declararía.
- **Centinela sin información de conexión**: las pruebas despachan el router con `oneshot` (`src/test_support.rs:1399-1410`), sin socket, así que no hay `ConnectInfo`. El limitador recibe entonces una IP centinela —todas las peticiones comparten cubo— y sigue siendo ejercitable. El servidor de verdad pasa a servirse con `into_make_service_with_connect_info::<SocketAddr>()` (`src/main.rs:57`).
- **Estado acotado**: el mapa de ventanas también se poda —las vencidas— y se le pone tope, para que el propio limitador no sea un sumidero.

Parámetros: `MAX_PETICIONES_DE_LOGIN = 20` por `VENTANA_DE_LOGIN = 60 s` por IP. Un humano hace unos pocos logins por minuto; veinte dejan sitio a un reintento y a una pestaña duplicada, y cortan en seco un caudal automatizado.

Respuesta: `429 Too Many Requests` con la envoltura (`{"status": 429, "message": …}`), añadiendo `AppError::TooManyRequests` —que hoy no existe— y su brazo en `into_response`.

## D4 · El tope del mapa de logins: se desaloja el más antiguo, no se rechaza el nuevo

Con el mapa lleno, la alternativa es **rechazar** el login nuevo o **desalojar** el más antiguo. Se desaloja:

- Rechazar convierte el tope en una denegación de disponibilidad: un atacante llena el mapa y nadie más inicia sesión. Desalojar mantiene el login disponible, y el daño se limita a perder un `state` pendiente, que caduca en cinco minutos de todos modos.
- La entrada desalojada es la de menor `issued_at` y no una al azar: `HashMap` no guarda orden de inserción, y la más antigua es la que está más cerca de caducar —la de menos valor— y la que un atacante habría tenido que crear primero.
- Con el límite de tasa por IP delante, llegar al tope exige ya muchas direcciones de origen: el tope es la segunda capa, no la primera.

`MAX_LOGINS = 1024`. Con una ventana de cinco minutos, el mapa sostiene de sobra el uso legítimo de un despliegue real.

## D5 · La validación del `endpoint`: en el borde, con permiso explícito, y sin resolver nombres

**Dónde.** En el borde HTTP, en el alta **y** en la edición. La alternativa —validar en el motor, junto a `Acceso::validar`— deja fuera la edición, porque la edición **no conecta** con el destino (`src/api/repositories.rs:631`): una fila podría quedar guardada con un `endpoint` inadmisible y fallar solo en la siguiente copia. Validar en el borde cubre las dos escrituras y produce el `400` con el nombre del campo, que es donde el operador lo lee. El motor conserva su comprobación de vacío, que es otra cosa.

**Qué se exige.** El esquema tiene que ser `https`. Se rechazan **siempre**: la dirección de enlace local (`169.254.0.0/16`, `fe80::/10`) —que incluye la de metadatos de la nube, `169.254.169.254`—, la de metadatos IPv6 de AWS (`fd00:ec2::254`), la no especificada (`0.0.0.0`, `::`) y un `endpoint` con credenciales embebidas (`https://usuario:clave@host`), que acabarían en la fila y en los registros. Se rechazan **salvo permiso explícito**: el loopback (`127.0.0.0/8`, `::1`), un nombre `localhost`/`*.localhost` y el esquema `http`. Se aceptan por `https`: el rango privado (`10/8`, `172.16/12`, `192.168/16`, `fc00::/7`) y las direcciones públicas.

**Por qué se permite el rango privado.** El caso de uso principal de Kesto es un S3 autoalojado —MinIO, SeaweedFS— en la red interna. Rechazar RFC1918 dejaría el producto inservible para su uso más común, y el atacante que importa aquí es quien ya tiene una sesión de operador: no gana nada alcanzando un servicio interno que el propio Kesto ya tiene que alcanzar para copiar. Lo que sí gana, y por eso se cierra, es alcanzar el **loopback** del proceso —su propia API, su base, el socket del daemon— y la **dirección de metadatos**, donde viven las credenciales de instancia de la máquina.

**Por qué una sola variable de permiso.** `KESTO_ALLOW_INSECURE_ENDPOINT` rebaja a la vez el esquema (`http` permitido) y el loopback —y el nombre `localhost`—. Son el mismo caso, «el destino está en mi propia máquina y en claro», que es lo que hacen el desarrollo local y la batería, cuyo almacén real es SeaweedFS en `http://127.0.0.1:<puerto>`: sin cubrir el loopback, la batería no podría crear un solo repositorio. Lo que la variable **no** rebaja nunca es la dirección de enlace local, la de metadatos ni la no especificada: no hay laboratorio que necesite alcanzar el servicio de metadatos, y dejarlas fuera mantiene cerrado el agujero más caro incluso con el permiso puesto.

**Lo que no se hace: resolver el nombre.** Un `endpoint` con nombre de host se acepta por su forma, sin resolverlo, así que un nombre que resuelva a una dirección de enlace local no se detecta. Cerrarlo exige resolver en el momento del alta y **fijar** la dirección usada en la conexión —entre la comprobación y la conexión cabe un cambio de DNS (*rebinding*)—, y `opendal` no ofrece ese punto de enganche. Queda como limitación declarada y como issue de seguimiento.

**El arnés.** Las pruebas de la API usan hoy una **ruta de directorio** como `endpoint` (`destino_local_de_prueba`, `src/test_support.rs:613-633`) y el abridor local la abre tal cual (`abridor_local`, `:483`). Con la validación en el borde, esas filas pasan a llevar una URL `https://…` cuyo **camino** es la ruta real, y el abridor local —y el comprobador de salud local— la traducen. El cambio queda contenido en el arnés y hace que la fila de prueba se parezca a la de producción.

**La crate `url`.** Para leer esquema, host y dirección literal hace falta un analizador de URL; a mano es donde se cuelan los errores (`https://evil@127.0.0.1`, esquemas con espacios, IPv6 entre corchetes). `url` ya está en el árbol —la arrastra `reqwest`—, así que declararla directa no añade nada al `Cargo.lock`.

## D6 · El oráculo de alcanzabilidad no se cierra

El hallazgo llama oráculo a que el alta conteste `201`/`400` cuando el destino responde y `502` cuando no. Se deja como está, a conciencia: quien puede usar el oráculo ya tiene sesión de operador y puede dar de alta repositorios, y ese mismo operador puede sondear la alcanzabilidad por la ruta de salud bajo demanda (`GET /api/v1/repositories/{id}/health`) y por el listado de instantáneas, que distinguen igual de bien lo alcanzable de lo que no. Uniformar la respuesta del alta no cierra la capacidad —la traslada— y en cambio quita el aviso inmediato que hoy recibe el operador cuando el destino no contesta. El `502` tampoco publica el motivo —es un `5xx` y el cuerpo lleva el mensaje genérico—, así que el oráculo es un bit, no un canal.

## D7 · Versión y forma de los commits

El change rompe compatibilidad en un punto: un despliegue OIDC sin `OIDC_REDIRECT_URL` —o con uno por `http`— deja de arrancar. Los commits se marcan `feat:`/`fix:` **sin** `!` y sin `BREAKING CHANGE`, porque el pipeline deduce `major` con esa marca y este proyecto va por `0.x`: el bump que corresponde es `0.19.0`. La incompatibilidad se describe en el cuerpo del PR, en el CHANGELOG y en `docs/ENTORNO.md`, que es donde el operador la lee.

## D8 · La forma del delta: dos requisitos se sustituyen en lugar de modificarse

OpenSpec no admite que un bloque `MODIFIED` pierda un escenario —`openspec validate` lo marca como error y el archivado se niega a aplicar el bloque, porque un `MODIFIED` sustituye al requisito entero y un escenario que desaparece sin decirlo se perdería—. Y aquí hay **dos escenarios que dejan de ser válidos** en lugar de cambiar de cuerpo: «`Secure` depende del esquema de retorno» (la regla que afirmaba ya no existe) y «La URL de retorno tiene un valor por defecto de desarrollo» (el defecto se elimina). Renombrar el escenario tampoco sirve: la comprobación los cuenta por nombre, así que el renombrado se lee como una pérdida.

Así que los dos requisitos afectados se declaran **`REMOVED`** con su motivo y su migración, y se **re-añaden** con la versión endurecida y un nombre que declara el cambio. Es la misma forma que el bloque A usó para el requisito de la credencial débil. La alternativa —conservar el nombre del escenario y escribirle un cuerpo que diga lo contrario— dejaría en la spec archivada una etiqueta que afirma justo lo que el requisito niega, y eso no se sostiene ante quien la lea.

Los otros tres requisitos que cambian —el compose y la verificación de la imagen en `deploy`, y el alta en `api-resources`— sí van como `MODIFIED`, porque conservan todos sus escenarios y solo añaden uno.
