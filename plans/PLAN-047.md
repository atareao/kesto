# PLAN-047 — Endurecimiento de la sesión y de la frontera de confianza (SEC-16 + SEC-17)

- **Issue**: #168 (bloque F de la auditoría #156), hallazgos SEC-16 y SEC-17.
- **Change OpenSpec**: `openspec/changes/endurecimiento-sesion-y-frontera/`.
- **Documento de la feature**: `docs/FEATURE-047-endurecimiento-sesion-y-frontera.md`.
- **Rama**: `feature/endurecimiento-sesion-y-frontera` desde `development`.

## Objetivo

1. **SEC-16** — escribir la frontera de confianza mono-operador (documentación; no se construye un modelo de roles).
2. **SEC-17** — el cierre de sesión debe invalidar la sesión en el servidor, no solo borrar la cookie, para que una cookie copiada deje de servir.

## Alcance

- `migrations/0010_sesiones_revocadas.sql` (nueva) — la tabla de revocadas.
- `src/db/entities/sesion_revocada.rs` (nuevo) + registro en `src/db/entities/mod.rs`.
- `src/state.rs` — `EstadoDeBase` gana el repositorio.
- `src/auth/session.rs` — el `jti` en `Claims`.
- `src/auth/middleware.rs` — `con_cookie` consulta la revocación.
- `src/api/auth.rs` — `logout` revoca.
- Specs: `auth` (MODIFIED de la verificación + ADDED del cierre y de la frontera).
- `docs/ARQUITECTURA.md` — la frontera mono-operador.

Fuera de alcance: el flujo OIDC (va en `endurecimiento-oidc`), el modelo de roles, la gestión de tokens de API.

## Decisiones clave

- Revocación **por sesión y persistida** en SQLite (no solo en memoria: un reinicio borraría las revocaciones y el arreglo se desharía solo).
- `jti` **obligatorio** en `Claims`: una cookie sin él no decodifica y el *gate* la rechaza. El despliegue cierra las sesiones vivas; es un efecto previsto y documentado.
- Purga de la tabla por caducidad (`exp <= ahora`) al escribir.
- `con_cookie` pasa a `async`: una consulta más, de la misma clase que ya hace el camino del token.
- Sin dependencias nuevas.

## Tareas

### 0. Preparación

- [ ] 0.1 Punto de partida en verde: `just check`.
- [ ] 0.2 Rama desde `development`: `just gf-feature endurecimiento-sesion-y-frontera`.
- [ ] 0.3 Change aprobado por el usuario.

### 1. El `jti` en la sesión

- [ ] 1.1 RED — `issue` produce `jti` no vacío y distinto por emisión; `verify` lo devuelve.
- [ ] 1.2 GREEN — `Claims` gana `jti`.

### 2. La tabla y el repositorio de revocadas

- [ ] 2.1 RED — `revocar`/`esta_revocada`, idempotencia.
- [ ] 2.2 RED — la purga por caducidad.
- [ ] 2.3 GREEN — `migrations/0010_sesiones_revocadas.sql`.
- [ ] 2.4 GREEN — `SesionesRevocadas` en `EstadoDeBase`.

### 3. El *gate* rechaza lo revocado

- [ ] 3.1 RED — cookie bien firmada con `jti` revocado → `401`.
- [ ] 3.2 RED (contrapeso) — cookie sin revocar → `200`.
- [ ] 3.3 GREEN — `con_cookie` `async` consulta la revocación.

### 4. El cierre de sesión revoca

- [ ] 4.1 RED — tras `logout`, reutilizar la cookie → `401`.
- [ ] 4.2 RED (contrapeso) — `logout` sigue devolviendo la `Set-Cookie` de borrado.
- [ ] 4.3 RED (contrapeso) — `logout` sin cookie → `401`.
- [ ] 4.4 GREEN — `logout` revoca el `jti`.

### 5. La frontera mono-operador (SEC-16)

- [ ] 5.1 `docs/ARQUITECTURA.md` con la frontera y su porqué.

### 6. Verificación

- [ ] 6.1 `just check`.
- [ ] 6.2 `just check-all`.
- [ ] 6.3 El flujo completo login→logout→cookie revocada, cubierto por un test de integración.

### 7. Documentación y cierre

- [ ] 7.1 `docs/FEATURE-047-endurecimiento-sesion-y-frontera.md`.
- [ ] 7.2 `openspec archive endurecimiento-sesion-y-frontera`.
- [ ] 7.3 Renombrar este plan a `plans/PLAN-047-COMPLETED.md`.

## Criterios de aceptación

- Una sesión (JWT) lleva un `jti` único por emisión.
- Reutilizar la cookie de una sesión cerrada contesta `401`.
- Una sesión revocada no se acepta en ninguna ruta protegida.
- La tabla de revocadas se purga por caducidad.
- La frontera mono-operador está escrita y sin modelo de roles.
- `just check-all` en verde.
