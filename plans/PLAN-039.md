# PLAN-039 — Endurecimiento del arranque y de la autenticación (SEC-01, SEC-02, SEC-03) (#157, #158, #159)

- **Issues:** #157 (SEC-01), #158 (SEC-02) y #159 (SEC-03), hijos de la auditoría de seguridad **#156**. **Change OpenSpec:** `endurecimiento-arranque-auth`. **Rama:** `feature/endurecimiento-arranque-auth` (desde `development`).
- **Tipo:** producto (Rust + plantilla de despliegue + CI + documentación). **No** toca el panel ni la KDF.
- **Detalle de tareas:** `openspec/changes/endurecimiento-arranque-auth/tasks.md`. Decisiones y alternativas descartadas: `design.md` del mismo change.

## 1. Objetivo

Que el modo sin autenticación deje de ser lo que ocurre por omisión y que una credencial débil deje de arrancar. En concreto: un permiso explícito (`KESTO_ALLOW_INSECURE`) para el modo `Disabled`, una dirección de escucha de loopback en ese modo, un mínimo de 32 caracteres para `KESTO_MASTER_KEY` y `KESTO_SECRET`, y la gestión de tokens fuera del modo sin autenticación.

## 2. Contexto

Los tres hallazgos comparten raíz: `Config::from_lookup` (`src/config.rs:186`) comprueba **presencia** y trata la fortaleza de una credencial como un aviso, y el modo sin autenticación es lo que pasa cuando no hay variables `OIDC_*`.

- **SEC-01 (#157):** sin las tres `OIDC_*` el modo es `Disabled` (`src/config.rs:340`), el bind por defecto es `0.0.0.0:8080` (`:58`) y en `Disabled` se monta toda la superficie de negocio sin capa (`src/api/mod.rs:136-156`), incluida `tokens::router()` (`:144`) y `POST /api/v1/snapshots/{id}/restore`, que escribe en el host. El único rastro es un `WARN` (`:396-400`).
- **SEC-02 (#158):** `env_lookup` devuelve `Some("")` para una variable definida pero vacía (`:178`), la comprobación es de presencia (`:189-193`) y la longitud corta solo avisa (`:403-407`). Con la clave vacía, HKDF-SHA256 con sal `None` y la etiqueta constante `kesto/secrets/v1` (`src/db/crypto.rs:273-280`) deriva una clave AES-256 determinista y pública: quien lea el `.db` descifra `repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`.
- **SEC-03 (#159):** en modo OIDC, `KESTO_SECRET` vacía se acepta por el mismo camino. La sesión se firma con `EncodingKey::from_secret(...)` (`src/auth/session.rs:63`) y se verifica en cada petición protegida (`src/auth/middleware.rs:191`): un JWT de sesión forjado es un bypass de autenticación.
- Comportamiento **documentado** que cambia: el modo de desarrollo sin OIDC es una funcionalidad del §8 y `openspec/specs/deploy/spec.md:85` fija que una credencial corta «avisa y arranca igualmente».

## 3. Decisiones de arquitectura

1. **`KESTO_ALLOW_INSECURE`**, prefijo `KESTO_` según la convención del repositorio. `1` permite el modo `Disabled`, `0` lo niega, cualquier otro valor —vacío incluido— es error de formato, y ausente equivale a negado. Se descarta exigirlo solo cuando el bind no sea de loopback: el caso del hallazgo es el `.env` que no se carga, y ahí nadie mira el bind.
2. **Loopback obligatorio en modo `Disabled`** (`127.0.0.0/8` o `::1`), con `DEFAULT_BIND_ADDRESS = "127.0.0.1:8080"`. Desviación deliberada del `0.0.0.0:8080` del §8: escuchar en todas las interfaces es una decisión del operador, no el resultado de no haber dicho nada.
3. **Consecuencia en contenedores:** un proceso atado a loopback dentro de un contenedor no es alcanzable por el puerto publicado, así que la verificación de la imagen publicada pasa a la **forma de producción** (modo OIDC, puerto publicado, `/api/v1/health` desde el runner). El descubrimiento OIDC caído solo avisa (`tests/arranque.rs:114`), así que no hace falta un proveedor real en CI. El modo de desarrollo se verifica en el e2e y el arnés de pruebas.
4. **Credencial débil = puerta de arranque.** Se asume el coste que `openspec/specs/secrets/spec.md:45` defendía (negarse a arrancar es peor que avisar) porque una clave vacía o corta hace públicamente calculable la clave de los secretos guardados. El mensaje nombra la variable y el mínimo, y `docs/ENTORNO.md` lo explica: un despliegue con clave corta dejará de arrancar al actualizarse.
5. **La KDF no se toca:** pasar a Argon2id/scrypt invalidaría los secretos ya cifrados con el sobre `v1:` y exige migración → issue de seguimiento.
6. **`tokens::router()` no se monta en `Disabled`**: sin proveedor no hay sesión que exigir, así que la ruta solo puede quedarse abierta —emitiendo credenciales sin credencial— o contestar `404`, como `/api/v1/auth/*` en ese modo. El panel no usa esas rutas.
7. **Validación inline en `from_lookup`**, con la comprobación cruzada del bind **después** de `resolve_auth` para no reordenar los errores ya probados. Variantes nuevas de `ConfigError`: `InsecureModeNotAllowed { name }`, `InsecureBindAddress { name }` y `WeakCredential { name, min }` (esta última nunca publica el valor).
8. **Commits `feat:`/`fix:` sin `!` ni `BREAKING CHANGE`**, aunque el cambio rompa compatibilidad: el pipeline deduce `major` con esa marca y este proyecto va por `0.x`; el bump que corresponde es `0.18.0`. La incompatibilidad se describe en el cuerpo del PR, el CHANGELOG y la documentación.

## 4. Alcance

- `src/config.rs`: variable, validación, umbral y variantes de error.
- `src/api/mod.rs`: la rama `Disabled` deja de montar `tokens::router()`.
- `src/api/tokens.rs`, `src/api/health.rs`, `src/test_support.rs`: tests y entornos de prueba.
- `tests/arranque.rs`, `tests/e2e/arnes.rs`: el permiso explícito donde se arranca en modo `Disabled`.
- `compose.yaml`, `.env.example`, `.github/workflows/release.yml`: plantilla de producción y smoke test.
- `docs/DEFINITION.md` (§8), `docs/ENTORNO.md`, `docs/ARQUITECTURA.md`.
- `ui/`: **sin cambios** (verificado: ninguna llamada a `/api/v1/tokens` en el panel).

## 5. Tareas (TDD)

> El detalle con las verificaciones de cada tarea está en `openspec/changes/endurecimiento-arranque-auth/tasks.md`.

### 0. Preparación
- [ ] 0.1 `just check` en verde antes de tocar nada.
- [ ] 0.2 Rama `feature/endurecimiento-arranque-auth` desde `development`.
- [ ] 0.3 Change aprobado por el usuario.

### 1. El permiso explícito y el bind de loopback (SEC-01)
- [ ] 1.1 RED: tests de permiso ausente, `=1`, `=0`, valor inválido, con OIDC, y de bind no-loopback con y sin OIDC, más el defecto de loopback.
- [ ] 1.2 GREEN: `ENV_KESTO_ALLOW_INSECURE`, `DEFAULT_BIND_ADDRESS`, variantes de error y comprobación cruzada tras `resolve_auth`; actualizar los tests existentes que cambian de contrato.
- [ ] 1.3 REFACTOR: rustdoc del permiso y de la regla, `cargo fmt`, `cargo clippy -D warnings`.

### 2. Una credencial débil impide el arranque (SEC-02, SEC-03)
- [ ] 2.1 RED: clave maestra vacía y corta, clave de sesión vacía y corta, credenciales largas sin aviso, mensaje sin valor, y ausencia de `KESTO_SECRET` que sigue siendo válida sin OIDC.
- [ ] 2.2 GREEN: `ConfigError::WeakCredential { name, min }`, validación en `from_lookup`, `warnings()` sin credenciales, y actualización de los tests de `config.rs` y `api/health.rs`.
- [ ] 2.3 REFACTOR: rustdoc de `warnings()` y `just check`.

### 3. La gestión de tokens no se monta sin autenticación (SEC-01)
- [ ] 3.1 RED: `GET`, `POST` y `DELETE` de `/api/v1/tokens` en modo `Disabled` contestan `404` y no emiten token.
- [ ] 3.2 GREEN: `tokens::router()` sale de la rama `None` de `src/api/mod.rs`; el test de la gestión abierta pasa a exigir el `404`.
- [ ] 3.3 REFACTOR: comentario en la rama explicando por qué.

### 4. Arnés, CI y plantilla de despliegue (SEC-01)
- [ ] 4.1 `src/test_support.rs`, `src/api/health.rs`, `tests/arranque.rs` y `tests/e2e/arnes.rs` declaran `KESTO_ALLOW_INSECURE=1`.
- [ ] 4.2 `compose.yaml` exige las tres `OIDC_*` y `KESTO_SECRET` con `${VAR:?}`, y no declara `KESTO_ALLOW_INSECURE`.
- [ ] 4.3 `.github/workflows/release.yml`: smoke test en la forma de producción, esperando `auth_mode: "oidc"`.
- [ ] 4.4 `.env.example`: la variable nueva con su aviso y las `OIDC_*` como obligatorias para el compose.
- [ ] 4.5 `just check-all` en verde.

### 5. Documentación
- [ ] 5.1 `docs/DEFINITION.md` §8, `docs/ENTORNO.md` (con nota de actualización) y `docs/ARQUITECTURA.md`.

### 6. Cierre
- [ ] 6.1 `docs/FEATURE-039-endurecimiento-arranque-auth.md`.
- [ ] 6.2 Renombrar este plan a `plans/PLAN-039-COMPLETED.md` con `git mv`.
- [ ] 6.3 PR a `development` con `Closes #157`, `Closes #158`, `Closes #159` y la nota de incompatibilidad.
- [ ] 6.4 `just spec-archive endurecimiento-arranque-auth`.
- [ ] 6.5 Issue de seguimiento para la KDF con sal persistida (Argon2id/scrypt).

## 6. Criterios de aceptación

1. Sin `OIDC_*` y sin `KESTO_ALLOW_INSECURE` el arranque falla nombrando la variable; con el permiso y bind de loopback arranca en `Disabled`.
2. Con el permiso y `BIND_ADDRESS` no loopback el arranque falla nombrando `BIND_ADDRESS`; en modo OIDC, `0.0.0.0:8080` sigue siendo válido.
3. Sin `BIND_ADDRESS` la dirección por defecto es `127.0.0.1:8080`.
4. `KESTO_MASTER_KEY` vacía o de menos de 32 caracteres, y `KESTO_SECRET` igual en modo OIDC, impiden el arranque sin publicar el valor.
5. En modo `Disabled`, `/api/v1/tokens` contesta `404` y no emite ningún token; el resto de la superficie sigue abierta.
6. `podman compose config` se niega a validar la plantilla sin las tres `OIDC_*` y `KESTO_SECRET`.
7. El smoke test del workflow de release verifica la forma de producción y observa `auth_mode: "oidc"`.
8. `just check-all` en verde (backend, `scripts/` y panel) y sin dependencias nuevas.
9. `docs/DEFINITION.md` §8, `docs/ENTORNO.md` y `docs/ARQUITECTURA.md` describen el modo nuevo y la nota de actualización para claves cortas.
