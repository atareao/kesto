# FEATURE-047 — Endurecimiento de la sesión y de la frontera de confianza (SEC-16 + SEC-17)

- **Issue**: #168 (bloque F de la auditoría #156).
- **Change OpenSpec**: `endurecimiento-sesion-y-frontera` (archivado).
- **Plan**: `plans/PLAN-047.md` → `plans/PLAN-047-COMPLETED.md`.
- **Rama**: `feature/endurecimiento-sesion-y-frontera` desde `development`.

## Qué es

Cierra dos hallazgos del bloque F:
- **SEC-16** — se **documenta** la frontera de confianza mono-operador (no se construye modelo de roles).
- **SEC-17** — el cierre de sesión deja de limitarse a borrar la cookie: ahora **revoca la sesión en el servidor**, de modo que una copia de la cookie deja de servir.

## Ficheros implicados

| Fichero | Cambio |
|---|---|
| `src/auth/session.rs` | `Claims` gana `jti` (32 caracteres alfanuméricos aleatorios), generado en la emisión; `verify` lo devuelve. Un JWT sin `jti` no verifica. |
| `src/db/entities/sesion_revocada.rs` | Nuevo. Capa `SesionesRevocadas`: `revocar(jti, exp)` (idempotente; purga antes lo caducado) y `esta_revocada(jti)`. |
| `migrations/0010_sesiones_revocadas.sql` | Nueva — tabla `sesiones_revocadas (jti TEXT PRIMARY KEY NOT NULL, exp INTEGER NOT NULL)`. |
| `src/db/entities/mod.rs` | Repositorio colgado de `EstadoDeBase` (`estado.db.sesiones`). |
| `src/state.rs` | Repositorio colgado de `EstadoDeBase` (`estado.db.sesiones`). |
| `src/auth/middleware.rs` | `con_cookie` pasa a `async`: tras verificar la firma consulta `esta_revocada`; una cookie revocada da `401` con el mismo `SIN_SESION` (sin distinguir el motivo); un error de base da `500`, como el camino del token. `SessionIdentity` lleva además el `jti`. |
| `src/api/auth.rs` | `logout` revoca el `jti` de la identidad verificada antes de responder y sigue devolviendo la `Set-Cookie` de borrado (`Max-Age=0`). |
| `src/db/migrate.rs` | Entrada de checksum de caracterización para la migración 0010. |
| `docs/ARQUITECTURA.md` | Sección «Identidad y frontera de confianza». |

## Cómo funciona

### La sesión como JWT HS256 sin estado
La sesión es un JWT HS256 **sin estado** firmado con `KESTO_SECRET`. Su firma sola no permite cerrarla antes de su `exp` (8 h, `SESSION_LIFETIME`), así que se añade un `jti` por sesión y una tabla de revocadas.

- El `logout` registra el `jti` de la sesión verificada; el *gate* consulta la tabla en cada petición protegida y rechaza lo revocado.
- La purga por caducidad (`exp <= ahora`) se hace **al escribir**, así que la tabla se queda con lo que aún protege.

### Caducidad vs. revocación
La revocación guarda `ahora + SESSION_LIFETIME` como caducidad: es **≥ el `exp` real** del token, así que **peca de retener de más, nunca de menos** (nunca se purga mientras la cookie podría presentarse). La alternativa descartada era exponer el `exp` literal y contradecir el doc de `SessionIdentity`.

### Purga al escribir
La purga por caducidad se hace **al escribir**, así que la tabla se queda con lo que aún protege.

## Variables y configuración

Ninguna nueva.

## Cómo se despliega

Sin pasos de despliegue propios: el cambio es validación en el borde y `maxLength` en el panel. Las filas ya guardadas **no se tocan**; una que hoy tuviera un valor por encima de su cota sigue leyendo, pero ya no se puede volver a escribir tal cual.

## Decisiones

- Revocación **por sesión y persistida en SQLite** (no solo en memoria: un reinicio borraría las revocaciones y el arreglo se desharía solo en silencio).
- `jti` **obligatorio** (sin `#[serde(default)]`): una cookie sin él no decodifica y el *gate* la rechaza.
- **SEC-16 se documenta, no se construye**: no hay roles ni scopes; el sistema asume un único operador. El único candado de código es el test que comprueba que un claim de rol no cabe en la sesión (`deny_unknown_fields` + los cinco claims exactos).
- Sin dependencias nuevas.

## Follow-ups

- Un despliegue cierra las sesiones vivas: una cookie emitida antes no lleva `jti` y deja de aceptarse (`401`). Es el precio de poder revocar; previsto y documentado.
- `POST /api/v1/auth/logout` invalida la sesión: reutilizar su cookie da `401`.
- Sin cambios de forma en rutas ni respuestas.

## Auditoría @rust-reviewer

- Auditoría `@rust-reviewer` → **APRUEBA**.

## Estado del repositorio

- `docs/ESTADO.md` — estado del repositorio (construido, verificado y pendiente).

## Checksum de migración 0010

- Entrada de caracterización en `src/db/migrate.rs` para la tabla `sesiones_revocadas`.

## Sección ARQUITECTURA — Identidad y frontera de confianza

- Describe cómo `SessionIdentity` lleva `jti`, cómo `con_cookie` verifica `esta_revocada`, y cómo el gate rechaza sesiones revocadas.
