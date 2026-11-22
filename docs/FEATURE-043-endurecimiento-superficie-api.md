# FEATURE-043 — Endurecimiento de la superficie de red y del borde HTTP (#163, #165, #166)

- **Issues:** #163 (SEC-07), #165 (SEC-09) y #166 (SEC-10), hijos de la auditoría de seguridad **#156** (bloque D). **Change OpenSpec:** `endurecimiento-superficie-api`. **Plan:** `plans/PLAN-043.md`.
- **Tipo:** producto (Rust + configuración + plantilla de despliegue + CI + documentación). **No** toca el motor ni el esquema de la base.
- **Incompatible hacia atrás en dos puntos**, documentados abajo y en `docs/ENTORNO.md`.

## Qué es

Tres hallazgos de la misma superficie —la cookie de sesión, la ruta de entrada y el `endpoint` de un repositorio—, cada uno cerrado donde el valor se decide:

1. **La cookie de sesión es `Secure` siempre.** `session_cookie` (`src/auth/session.rs`) pierde el parámetro `redirect_url` y emite `.secure(true)` incondicional; desaparece `requires_secure`. El atributo dejó de depender del esquema de retorno, así que en modo OIDC `OIDC_REDIRECT_URL` pasa a ser **obligatoria y `https`** (`src/config.rs`): ausente → error de arranque nombrando la variable; presente sin `https` → `ConfigError::InsecureRedirect`. Se elimina la constante `DEFAULT_OIDC_REDIRECT_URL` (era `http`).
2. **Dos protecciones en memoria en la ruta de entrada.** El mapa de logins tiene un tope duro (`MAX_LOGINS = 1024`, `src/state.rs`): al insertar se purga lo caducado y, si sigue lleno, se desaloja la entrada **más antigua** por `issued_at`, **nunca** se rechaza el login nuevo. Y `GET /api/v1/auth/login` pasa por un límite de tasa por IP (módulo puro nuevo `src/auth/limite.rs`: `MAX_PETICIONES_DE_LOGIN = 20`, `VENTANA_DE_LOGIN = 60 s`, `MAX_VENTANAS = 1024`, `IP_CENTINELA = 0.0.0.0`), colgado con `from_fn_with_state` en `src/api/mod.rs`. La IP sale del **socket** (`ConnectInfo<SocketAddr>`, `into_make_service_with_connect_info` en `src/main.rs`), nunca de una cabecera; `AppError::TooManyRequests` → `429` con la envoltura.
3. **El borde valida el `endpoint` de un repositorio.** Módulo puro nuevo `src/rustic/endpoint.rs` (`validar(valor, permitir_inseguro)`, enum `Motivo` de ocho variantes y `ErrorDeEndpoint`, cuyo `Display` nombra el campo y **no** repite la URL). Único consumidor: `endpoint_admisible` en `src/api/repositories.rs`, llamado por el alta y por la edición. `https` siempre; `http` solo con el permiso nuevo `KESTO_ALLOW_INSECURE_ENDPOINT=1`.

## Causa / motivo

Los tres hallazgos comparten raíz: una decisión de seguridad delegada a un valor que el despliegue podía dejar en claro o crecer sin límite.

- **SEC-09:** el atributo `Secure` dependía de que `OIDC_REDIRECT_URL` fuera `https`, y su defecto era `http://localhost:8080/api/v1/auth/callback` (`DEFAULT_OIDC_REDIRECT_URL`). Un despliegue OIDC por HTTP emitía la cookie **sin** `Secure`: interceptable y reutilizable, porque la cookie **es** la sesión y vive ocho horas (`SESSION_LIFETIME`). Fallar el arranque es mejor que un panel que no deja entrar.
- **SEC-10:** `GET /api/v1/auth/login` es pública y cada petición escribe en memoria del proceso. Sin tope, el mapa de logins crecía una entrada por cada intento que nadie completaba; sin límite de tasa, un caudal alto y no autenticado era un sumidero de memoria. La IP de origen se leía —o se leería— de una cabecera falsificable, así que el límite tiene que salir del socket.
- **SEC-07:** el `endpoint` es el único campo de una fila de repositorio que decide **a dónde conecta** el proceso. Sin comprobación, quien ya tiene una sesión de operador puede apuntarlo al loopback del propio Kesto —su API, su base, el socket del daemon— o a la dirección de metadatos de la instancia, donde viven las credenciales de la máquina.

## Ficheros implicados

- `src/auth/session.rs`: `session_cookie` pierde el parámetro y emite `Secure` incondicional; desaparece `requires_secure`.
- `src/config.rs`: `resolve_auth` exige `OIDC_REDIRECT_URL` presente y `https`; variante `InsecureRedirect`; se elimina `DEFAULT_OIDC_REDIRECT_URL`; constante `ENV_KESTO_ALLOW_INSECURE_ENDPOINT`, campo `Config.allow_insecure_endpoint` con el defecto negado.
- `src/state.rs`: `MAX_LOGINS`, purga por caducidad seguida del desalojo de la más antigua en `registrar_login`; el limitador en `AppState` (`limite: Arc<Mutex<LimitadorDeLogin>>`).
- `src/auth/limite.rs`: módulo nuevo —el limitador de ventana fija por IP— con el reloj y la IP inyectados.
- `src/rustic/endpoint.rs`: módulo nuevo **puro** —`validar`, `Motivo`, `ErrorDeEndpoint`—; la limitación de no resolver el nombre queda declarada.
- `src/api/mod.rs`: la capa `limitar_el_login` sobre `GET /api/v1/auth/login`.
- `src/api/repositories.rs`: `endpoint_admisible`, llamada en `alta` y en `actualizar`.
- `src/main.rs`: `into_make_service_with_connect_info::<SocketAddr>()`.
- `src/error.rs`: variante `TooManyRequests` y su brazo → `429` con la envoltura.
- `Cargo.toml`: `url = "2.5.8"` como dependencia directa (ya estaba en el árbol por `reqwest`).
- `src/test_support.rs`, `tests/arranque.rs`, `tests/e2e/arnes.rs`, `tests/e2e/escenarios.rs`: el permiso `KESTO_ALLOW_INSECURE_ENDPOINT=1` en el arnés y el escenario nuevo del almacén real.
- `compose.yaml`, `.env.example`, `.github/workflows/release.yml`: la cuarta `OIDC_*` con `${VAR:?}` y el smoke test con retorno `https`.
- `docs/DEFINITION.md` (§8), `docs/ENTORNO.md`, `docs/ARQUITECTURA.md`.
- `openspec/changes/endurecimiento-superficie-api/` · `plans/PLAN-043.md`.
- `ui/`: **sin cambios** (el límite y la validación son del borde; el panel no gana ni pierde campos).

## Cómo funciona

- **`Secure` incondicional, retorno obligatorio.** `session_cookie` ni siquiera recibe la URL de retorno: quien decide si el despliegue es admisible es el arranque —con OIDC, `OIDC_REDIRECT_URL` tiene que ser `https`—, no la forma de la respuesta. Configurada **sola**, sin las otras tres `OIDC_*`, la variable se sigue ignorando en silencio: `resolve_auth` decide el modo sobre la presencia de las tres variables de la autenticación (`issuer`, `client_id`, `client_secret`), no del retorno.
- **Tope del mapa de logins: desalojar, no rechazar.** `registrar_login` purga primero lo caducado; el desalojo de la más antigua solo entra si el mapa sigue lleno. Rechazar el login nuevo convertiría el tope en una denegación de servicio de disponibilidad, y el `state` más antiguo es el que menos probabilidad tiene de volver. El desalojo es el último recurso, nunca el primero.
- **Límite de tasa por IP, no global.** Un límite global lo agota un atacante y deja fuera a los legítimos; por IP el daño se acota a quien abusa. La ventana es **fija** —se explica en una frase y se prueba con reloj inyectado, sin dormir—; se busca **acotar** el caudal, no suavizarlo. Se purgan todas las ventanas vencidas antes de contar, y el tope del mapa (`MAX_VENTANAS`) desaloja la ventana más antigua en vez de rechazar: el mismo razonamiento que el tope de logins.
- **La IP sale del socket.** `limitar_el_login` lee `ConnectInfo<SocketAddr>` de las extensiones; donde no hay conexión —las pruebas despachan con `oneshot`, sin socket— cae a la centinela `0.0.0.0`, para que el limitador sea ejercitable sin abrir un puerto. `X-Forwarded-For` la falsifica el cliente y no se honra: Kesto no se despliega detrás de un proxy de confianza.
- **Validación del `endpoint` en el borde, no en el motor.** La edición **no** conecta con el destino, así que una comprobación en el motor no la alcanzaría. `endpoint_admisible` es la **única** puerta de la regla: el alta la llama después de comprobar los dos secretos obligatorios y antes de montar la fila; la edición, después de resolver el identificador y leer la fila, de modo que el `404` de la fila ausente se decide antes. El permiso entra por parámetro desde la configuración, no de la petición: un cliente no puede concedérselo a sí mismo.
- **Qué rebaja el permiso y qué no.** `https` siempre; `http` solo con `KESTO_ALLOW_INSECURE_ENDPOINT=1`. Se rechazan **siempre** —también con el permiso— la dirección de enlace local (`169.254.0.0/16`, `fe80::/10`), la de metadatos (`169.254.169.254`, `fd00:ec2::254`), la no especificada (`0.0.0.0`, `::`) y las credenciales embebidas. El permiso rebaja solo el esquema `http` y el loopback (`127.0.0.0/8`, `::1`) con sus nombres `localhost`/`*.localhost`. No hay laboratorio que necesite alcanzar el servicio de metadatos.
- **IPv4 embebida en IPv6.** `ip_sin_envolver` desmapea `::ffff:a.b.c.d` (RFC 4291) y el prefijo NAT64 `64:ff9b::/96` (RFC 6052) **antes** de juzgar, porque en Linux esas direcciones conectan a la IPv4 real: sin desmapearlas, `is_loopback()` e `is_unspecified()` miran la IPv6 —donde dan `false`— y `https://[::ffff:127.0.0.1]:9000/` se saltaría justo la regla que el módulo existe para hacer cumplir.
- **El alta conserva su oráculo.** La validación es una puerta **previa**: el alta sigue distinguiendo «la petición no vale» (`400`, antes de tocar la base o el destino) de «el destino no responde» (`502`). El `endpoint` admisible contra un destino inalcanzable sigue contestando `502`.

## Cómo verificar

```sh
just check-all          # backend (fmt + clippy -D warnings + cargo test --locked) + panel
cargo test --locked --bin kesto                 # 763 passed; 0 failed; 44 ignored
cargo test --locked --test arranque             # 6 passed
just e2e                                        # 39 passed; 0 failed; 1 ignored
podman compose config                           # se niega sin OIDC_REDIRECT_URL; sale bien con ella
```

## Decisiones

- **`Secure` incondicional y fallar en el arranque, sin válvula.** Se descartó una excepción para un retorno por HTTP: la cookie **es** la sesión y vive ocho horas, así que un panel que no deja entrar por `http` es el desenlace correcto. La incompatibilidad se paga una vez, en la actualización, y no en cada respuesta.
- **`OIDC_REDIRECT_URL` sin defecto.** `DEFAULT_OIDC_REDIRECT_URL` era un valor muerto —un retorno por `http` que no puede arrancar en modo OIDC—, y «falta `OIDC_REDIRECT_URL`» es más útil para el operador que «no es `https`».
- **Desalojar en vez de rechazar, en los dos topes.** Tanto el mapa de logins como el de ventanas usan el mismo desenlace: purgar lo caducado y, si sigue lleno, desalojar lo más antiguo. Rechazar la petición nueva convertiría un tope de memoria en una denegación de servicio.
- **Ventana fija, no cubo de fichas.** Se explica en una frase y se prueba con reloj inyectado. Aquí no se busca suavizar el caudal sino acotarlo.
- **La IP del socket y nunca de una cabecera.** `X-Forwarded-For` la falsifica el cliente; honrarla regalaría el límite —bastaría variar la cabecera para estrenar una IP en cada petición—. Kesto no se despliega detrás de un proxy de confianza.
- **La validación del `endpoint` en el borde, en un solo sitio.** El módulo puro lo usan las dos rutas, así que no pueden discrepar y el `PUT` no puede ser la puerta de atrás del `POST`. En el motor no alcanzaría a la edición, que no conecta con el destino.
- **Valores `1`/`0` para el permiso, sin sinónimos.** Mismo patrón que `KESTO_ALLOW_INSECURE`: en este repositorio un valor presente pero inutilizable ya es error, y una tabla de sinónimos es superficie de confusión. El permiso se lee **siempre**, también en modo OIDC, porque el `endpoint` se valida en los dos modos.
- **El nombre no se resuelve: limitación declarada.** Un `endpoint` con nombre de host se acepta por su forma; cerrarlo exigiría resolver en el alta y **fijar** la dirección usada en la conexión —entre la comprobación y la conexión cabe un cambio de DNS—, y el cliente S3 no ofrece ese punto de enganche. Issue de seguimiento abierto.

## Incompatibilidades (nota de actualización)

1. **En modo OIDC, `OIDC_REDIRECT_URL` pasa a ser obligatoria y `https`.** Un despliegue que no la declarara —o la declarara por `http`— deja de arrancar al actualizarse; la reparación es declararla por `https`. Configurada sola, sin las otras tres `OIDC_*`, se sigue ignorando en silencio.
2. **Nueva variable `KESTO_ALLOW_INSECURE_ENDPOINT`.** Por defecto **negada**: un `endpoint` por `http` o a loopback que hoy se acepta pasa a rechazarse con `400` si no se declara `KESTO_ALLOW_INSECURE_ENDPOINT=1`. El enlace local, la dirección de metadatos, la no especificada y las credenciales embebidas se rechazan siempre, también con el permiso. `compose.yaml` la deja opcional (viaja por el `.env`).

## Follow-ups

- **Resolución del nombre del `endpoint` (DNS):** un host que resuelva a una dirección rechazada no se detecta hoy. Issue de seguimiento abierto.
- **Resto de la campaña de seguridad #156:** los ya abiertos #169 (KDF con sal persistida), #173 (privilegio del motor) y #175 (`ON_ERROR_STOP` en la restauración de PostgreSQL).
