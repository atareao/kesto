# Tasks — endurecimiento-sesion-y-frontera

> SEC-16 y SEC-17 del bloque F, hijos de la auditoría #156 (#168).
> Decisiones y alternativas descartadas, en `design.md`.
> TDD: primero los tests que hoy fallan (rojo), luego el mínimo (verde), luego el refactor.
> Sin dependencias nuevas.
> Plan: `plans/PLAN-047.md`. Documento de la feature: `docs/FEATURE-047-endurecimiento-sesion-y-frontera.md`.

## 0. Preparación

- [ ] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [ ] 0.2 Rama desde `development`: `just gf-feature endurecimiento-sesion-y-frontera` (verificación: `git branch --show-current`).
- [ ] 0.3 Change aprobado por el usuario (verificación: aprobación explícita y `just check-spec` encuentra el change activo).

## 1. El `jti` en la sesión

### RED

- [ ] 1.1 Rust (`src/auth/session.rs`): `issue` produce un `jti` no vacío, y dos emisiones producen `jti` distintos; `verify` devuelve el `jti` (verificación: rojo hoy, que `Claims` no tiene `jti`).

### GREEN

- [ ] 1.2 `src/auth/session.rs`: `Claims` gana `jti`, generado al emitir (verificación: 1.1 en verde).

## 2. La tabla y el repositorio de revocadas

### RED

- [ ] 2.1 Rust (`src/db/entities/sesion_revocada.rs`): `revocar(jti, exp)` inserta y `esta_revocada` lo confirma; `revocar` dos veces no falla; `esta_revocada` de un `jti` no revocado es falso (verificación: rojo hoy, que no existe).
- [ ] 2.2 Rust: `revocar` purga las filas con `exp` ya pasado (verificación: rojo hoy).

### GREEN

- [ ] 2.3 `migrations/0010_sesiones_revocadas.sql` con la tabla.
- [ ] 2.4 `src/db/entities/sesion_revocada.rs`, registrado en `src/db/entities/mod.rs` y en `EstadoDeBase` (verificación: 2.1, 2.2 en verde).

## 3. El *gate* rechaza lo revocado

### RED

- [ ] 3.1 Rust (`src/auth/middleware.rs`): una cookie bien firmada y en vigor cuyo `jti` está revocado contesta `401` con la envoltura (verificación: rojo hoy, que pasa).
- [ ] 3.2 Rust: la misma cookie **sin** revocar contesta `200` (contrapeso; verde hoy).

### GREEN

- [ ] 3.3 `con_cookie` pasa a `async` y consulta `esta_revocada` (verificación: 3.1 en verde).

## 4. El cierre de sesión revoca

### RED

- [ ] 4.1 Rust (`src/api/auth.rs`): tras `POST /api/v1/auth/logout` con una cookie válida, reutilizar **esa misma** cookie contesta `401` (verificación: rojo hoy, que seguiría valiendo).
- [ ] 4.2 Rust: el `logout` sigue contestando `200` con la `Set-Cookie` de borrado (contrapeso; verde hoy).
- [ ] 4.3 Rust: `logout` sin cookie contesta `401` (contrapeso; verde hoy).

### GREEN

- [ ] 4.4 `logout` revoca el `jti` de la cookie antes de responder (verificación: 4.1 en verde).

## 5. La frontera mono-operador, por escrito (SEC-16)

- [ ] 5.1 `docs/ARQUITECTURA.md`: sección de identidad con la frontera y su porqué (verificación: lectura; `rg -ni 'role' src/auth src/api` no encuentra un modelo de autorización de sesión).

## 6. Verificación de la feature

- [ ] 6.1 `just check` (verificación: fmt + clippy + test en verde).
- [ ] 6.2 `just check-all` (verificación: el análogo local del CI en verde).
- [ ] 6.3 El flujo completo —login con el proveedor falso, `logout` y reutilización de la cookie → `401`— queda cubierto por un test de integración en verde (verificación: la suite).

## 7. Documentación y cierre

- [ ] 7.1 `docs/FEATURE-047-endurecimiento-sesion-y-frontera.md`.
- [ ] 7.2 `openspec archive endurecimiento-sesion-y-frontera`.
- [ ] 7.3 Renombrar `plans/PLAN-047.md` a `plans/PLAN-047-COMPLETED.md`.
