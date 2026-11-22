# Proposal — endurecimiento de la sesión y de la frontera de confianza

**SEC-16** y **SEC-17** del bloque **F** de la auditoría de seguridad **#156** (#168). El endurecimiento del flujo OIDC (los NIT de SEC-22) va en su propio change, `endurecimiento-oidc`.

## Why

1. **SEC-16 (frontera sin escribir).** El sistema asume un único operador de confianza: no hay roles ni scopes —`Claims` (`src/auth/session.rs:36`) lleva solo `sub`, `email`, `iat` y `exp`, y `deny_unknown_fields` impide que un rol se cuele—, así que cualquier identidad autenticada puede todo lo que el *gate* permite. Es una decisión de producto, no un descuido, pero hoy no está escrita en ningún sitio: quien lea el código no sabe si la ausencia de autorización es deliberada o un olvido.
2. **SEC-17 (el cierre de sesión no cierra).** `POST /api/v1/auth/logout` (`src/api/auth.rs:235`) solo devuelve la cookie de borrado (`Max-Age=0`). La sesión es un JWT **sin estado** firmado con `KESTO_SECRET`, así que una copia de la cookie —en un equipo compartido, en un registro, en un proxy— sigue siendo válida hasta su `exp`, ocho horas después (`SESSION_LIFETIME`, `src/auth/session.rs:26`). «Cerrar sesión» no cierra: solo borra el rastro local.

## What Changes

### SEC-16 — la frontera, por escrito
- Se documenta en `docs/ARQUITECTURA.md` (sección de identidad) y en la spec `auth` que el modelo es de **un solo operador** y que **no hay autorización por rol ni scope**: cada ruta protegida está disponible para cualquier sesión válida porque no hay más de un operador que distinguir. El día que aparezcan roles, esta frontera es la que hay que ampliar.

### SEC-17 — el cierre de sesión revoca
- La sesión (el JWT) SHALL llevar un `jti`: identificador único de la sesión, aleatorio, distinto en cada emisión.
- Una tabla nueva `sesiones_revocadas (jti TEXT PRIMARY KEY, exp INTEGER NOT NULL)` (migración `0010_sesiones_revocadas.sql`) guarda las sesiones revocadas hasta su caducidad. El `logout` inserta el `jti` de la cookie; el *gate* rechaza con `401` cualquier cookie cuyo `jti` esté en la tabla.
- La revocación se purga por caducidad (`exp <= ahora`) al escribir, así que la tabla se queda con lo que aún protege.
- Una cookie emitida **antes** de este change no lleva `jti` y deja de aceptarse tras el despliegue: el despliegue cierra las sesiones vivas. Es el precio de exigir el `jti`; se documenta.

## Non-goals

- No se sustituye el modelo de sesión sin estado por un store de sesiones.
- No se añade autorización por rol ni scope (eso es «el modelo de roles» que SEC-16 **no** construye).
- No se toca el flujo OIDC (caché JWKS, PKCE, `nonce`): va en `endurecimiento-oidc`.
- No se toca la gestión de tokens de API.

## New Capabilities

_(ninguna)_

## Modified Capabilities

- `auth`: la sesión lleva `jti`; el *gate* rechaza una sesión revocada; el cierre de sesión revoca además de borrar la cookie; se escribe la frontera mono-operador.

## Impact

- **Código**: `migrations/0010_sesiones_revocadas.sql` (nueva), `src/db/entities/sesion_revocada.rs` (nuevo), `src/db/entities/mod.rs`, `src/state.rs`, `src/auth/session.rs`, `src/auth/middleware.rs`, `src/api/auth.rs`.
- **API**: ninguna ruta ni forma nueva; un `logout` pasa a invalidar la sesión, y una cookie revocada contesta `401`.
- **Base**: una tabla nueva.
- **Despliegue**: el despliegue cierra las sesiones vivas (una cookie sin `jti` no se acepta).
- **Dependencias**: ninguna nueva.
