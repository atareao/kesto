# Proposal — endurecimiento de la superficie de la API

Issues **#163 (SEC-07)**, **#165 (SEC-09)** y **#166 (SEC-10)**, hijos de la auditoría de seguridad **#156** (bloque D).

## Why

Los tres hallazgos viven en la superficie de red y en el borde HTTP —la cookie de sesión, la ruta pública del login y el destino S3 de un repositorio— y ninguno toca el motor ni la base.

1. **SEC-09 (#165)** — la cookie de sesión solo lleva `Secure` si el esquema de `OIDC_REDIRECT_URL` es `https` (`src/auth/session.rs:126`, vía `requires_secure` en `:136-140`), y el valor por defecto es `http://localhost:8080/api/v1/auth/callback` (`src/config.rs:89`). Un despliegue OIDC por HTTP emite la cookie **sin** `Secure`: interceptable y reutilizable, porque la cookie **es** la sesión y vive ocho horas (`SESSION_LIFETIME`, `src/auth/session.rs:26`). HSTS está apagada por defecto (`KESTO_HSTS_MAX_AGE` es opcional) y `warnings()` no avisa. La spec consagra hoy el comportamiento (`openspec/specs/auth/spec.md:190` y su escenario `:205`).
2. **SEC-10 (#166)** — `GET /api/v1/auth/login` es pública (`src/api/mod.rs:92`) y **no tiene límite de tasa**. Cada petición inserta una entrada en el mapa de logins (`AuthState::registrar_login`, `src/state.rs:129-141`), y ese mapa **no tiene tope**: se poda por caducidad —`STATE_TTL` de cinco minutos— y solo de forma perezosa al insertar o al consumir, cosa que el propio código admite (`src/state.rs:124-128`). Un caudal alto y no autenticado de logins dentro de esa ventana lo hace crecer sin límite: es una denegación de servicio de memoria.
3. **SEC-07 (#163)** — el `endpoint` de un repositorio entra por el alta (`src/api/repositories.rs:88`, `:546-550`) y llega a `opendal` **sin validación de esquema ni de host** (`opciones_de_s3`, `src/rustic/repositorio.rs:1419`): lo único que se comprueba es que no vaya vacío (`Acceso::validar`, `:201-219`). Un `http://` firma SigV4 y manda la `secret_access_key` en claro por la red, y la diferencia entre `201`/`400` y `502` convierte el alta en un oráculo de alcanzabilidad contra hosts arbitrarios —incluida la dirección de metadatos de la nube, si el proceso corre en ella—.

## What Changes

### La cookie de sesión siempre lleva `Secure`, y el retorno de OIDC tiene que ser `https` (SEC-09)

- `session_cookie` deja de decidir el atributo `Secure` a partir del retorno: en modo `Oidc` la cookie **siempre** lo lleva. La función pierde el parámetro `redirect_url` y desaparece `requires_secure` (`src/auth/session.rs:109-140`).
- En modo `Oidc`, `OIDC_REDIRECT_URL` pasa a ser **obligatoria** y su esquema **tiene que ser `https`**: ausente, o presente con otro esquema, es **error de arranque** que nombra la variable. Se elimina `DEFAULT_OIDC_REDIRECT_URL` (`src/config.rs:89`): un retorno por defecto que no puede arrancar nunca en modo OIDC es un valor muerto, y «falta `OIDC_REDIRECT_URL`» es más útil que «no es `https`». Sin las tres `OIDC_*` la variable se sigue ignorando en silencio.
- **BREAKING**: un despliegue OIDC que hoy no declare `OIDC_REDIRECT_URL` —o que lo declare por `http`— **deja de arrancar**. El `compose` de referencia y el smoke test de la imagen pasan a declararlo por `https`.

### El login tiene límite de tasa y el mapa de logins tiene tope (SEC-10)

- El mapa `AuthState.logins` gana un **tope duro** (`MAX_LOGINS`): al insertar, si tras la purga por caducidad sigue lleno, se desaloja la entrada **más antigua** por `issued_at`. La ventana del `state` no cambia.
- `GET /api/v1/auth/login` gana un **límite de tasa por IP** de ventana fija (`VENTANA_DE_LOGIN` / `MAX_PETICIONES_DE_LOGIN`), con el estado en memoria del proceso y acotado también él. Al superarlo contesta **`429`** con la envoltura de error de la casa.
- La IP sale del **socket** (`ConnectInfo<SocketAddr>`), nunca de `X-Forwarded-For` ni de ninguna otra cabecera: no hay proxy de confianza que declarar. El servidor pasa a servirse con `into_make_service_with_connect_info` (`src/main.rs:57`), y donde no hay información de conexión —las pruebas que despachan el router con `oneshot`— se usa una IP centinela, para que el limitador siga siendo ejercitable sin abrir un socket.

### El `endpoint` de un repositorio se valida (SEC-07)

- Módulo nuevo **puro** `src/rustic/endpoint.rs`: valida la URL del `endpoint` —esquema, host, dirección literal, credenciales embebidas— sin tocar la red y con sus propias pruebas.
- Reglas: el esquema tiene que ser **`https`**; la dirección de enlace local (`169.254.0.0/16`, `fe80::/10`), la de metadatos IPv6 (`fd00:ec2::254`), la no especificada (`0.0.0.0`, `::`) y un `endpoint` con credenciales embebidas se rechazan **siempre**; el loopback, un nombre `localhost` y el esquema `http` se rechazan **salvo permiso explícito**; el rango privado (`10/8`, `172.16/12`, `192.168/16`, `fc00::/7`) y las direcciones públicas se aceptan por `https`.
- Variable nueva **`KESTO_ALLOW_INSECURE_ENDPOINT`**: `1` concede el permiso (desarrollo y batería); `0` o ausente lo niegan; cualquier otro valor —vacío incluido— es error de arranque que nombra la variable. Mismo patrón que `KESTO_ALLOW_INSECURE`.
- La validación va en el **borde HTTP**, en el alta **y** en la edición, porque la edición no conecta con el destino (`src/api/repositories.rs:631`) y una comprobación en el motor no la alcanzaría. El alta **conserva** su comprobación de conectividad: no se cierra el oráculo (ver `design.md`).

### Despliegue, batería y documentación

- `compose.yaml` declara `OIDC_REDIRECT_URL` con la forma `${OIDC_REDIRECT_URL:?}` junto a las tres `OIDC_*` y `KESTO_SECRET`; el smoke test de la imagen publicada la declara por `https`.
- El arnés del e2e declara `KESTO_ALLOW_INSECURE_ENDPOINT=1`, porque su almacén real es SeaweedFS en `http://127.0.0.1:<puerto>`.
- `docs/DEFINITION.md` §8, `docs/ENTORNO.md`, `docs/ARQUITECTURA.md` y `.env.example`.

## Non-goals

- NO se cierra el **oráculo de alcanzabilidad** del alta: quien puede dar de alta un repositorio ya tiene sesión de operador y puede sondear la conectividad por la ruta de salud y por el listado de instantáneas, así que uniformar la respuesta del alta quitaría el aviso inmediato sin quitar la capacidad.
- NO se resuelve el **nombre del `endpoint`**: un host que resuelva por DNS a una dirección de enlace local no se detecta. Cerrarlo exige resolver y **fijar** la dirección usada en la conexión, y no cabe en este bloque → issue de seguimiento.
- NO se añade **proxy de confianza** ni lectura de `X-Forwarded-For`.
- NO se pone **TLS en el arnés** ni se sirve SeaweedFS por `https`.
- NO se limita la tasa de `/api/v1/auth/callback`: no es un sumidero de memoria —consume una entrada acotada— y un `state` desconocido se rechaza en el acto.
- NO se toca el `ON_ERROR_STOP` de la restauración (**#175**), ni la KDF (**#169**), ni la separación de privilegio del motor (**#173**).
- NO se añade ninguna dependencia nueva: la crate `url` ya está en el árbol por `reqwest` y pasa a declararse directa.

## Capabilities

### New Capabilities

_(ninguna)_

### Modified Capabilities

- `auth`: la cookie de sesión lleva `Secure` siempre; el login tiene límite de tasa y el mapa de logins tiene tope.
- `config`: en modo OIDC la URL de retorno es obligatoria y `https`; el `endpoint` de los repositorios se valida salvo permiso explícito.
- `api-resources`: el alta y la edición de un repositorio validan el `endpoint`.
- `deploy`: el compose de referencia y el smoke test declaran `OIDC_REDIRECT_URL`.
- `verificacion-e2e`: la batería declara el permiso de endpoints inseguros para su almacén real.

## Impact

- **Código**: `src/config.rs`, `src/auth/session.rs`, `src/auth/limite.rs` (nuevo), `src/state.rs`, `src/error.rs`, `src/api/auth.rs`, `src/api/mod.rs`, `src/api/repositories.rs`, `src/rustic/endpoint.rs` (nuevo), `src/main.rs`, `src/test_support.rs`.
- **API**: `GET /api/v1/auth/login` puede contestar `429`; el alta y la edición de repositorios pueden contestar `400` por el `endpoint`. Ninguna ruta cambia de forma.
- **Despliegue**: `OIDC_REDIRECT_URL` pasa a ser obligatoria en modo OIDC —cambio incompatible para un despliegue que no la declare o la declare por `http`—; nueva variable opcional `KESTO_ALLOW_INSECURE_ENDPOINT`.
- **Dependencias**: ninguna nueva en el `Cargo.lock`; `url` pasa a dependencia directa.
- **Batería**: el arnés del e2e declara el permiso de endpoints inseguros; las filas de prueba de la API usan un `endpoint` con forma de URL.
