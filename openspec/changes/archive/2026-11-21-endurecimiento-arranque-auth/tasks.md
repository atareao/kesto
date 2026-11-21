# Tasks — endurecimiento-arranque-auth

> Issues **#157 (SEC-01)**, **#158 (SEC-02)** y **#159 (SEC-03)**, hijos de la auditoría **#156**.
> Change de **producto**: configuración de arranque, router de la API, plantilla de despliegue y CI.
> Decisiones y alternativas descartadas, en `design.md`. TDD: primero los tests que hoy fallan (rojo),
> luego el mínimo (verde), luego el refactor. Sin dependencias nuevas.

## 0. Preparación

- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error antes de tocar nada).
- [x] 0.2 Rama desde `development`: `just gf-feature endurecimiento-arranque-auth` (verificación: `git branch --show-current`).
- [x] 0.3 Change aprobado por el usuario (verificación: aprobación explícita y `just check-spec` encuentra el change activo).

## 1. El permiso explícito y el bind de loopback (SEC-01, #157)

### RED

- [x] 1.1 Rust: test —sin ninguna `OIDC_*` y sin `KESTO_ALLOW_INSECURE`— exige un error que **nombre `KESTO_ALLOW_INSECURE`** (verificación: rojo hoy, que arranca en `Disabled`).
- [x] 1.2 Rust: test con `KESTO_ALLOW_INSECURE=1` y `BIND_ADDRESS=127.0.0.1:8080`, sin `OIDC_*`, exige modo `Disabled` y configuración válida (verificación: verde tras 1.9).
- [x] 1.3 Rust: test con `KESTO_ALLOW_INSECURE=true` exige error de formato nombrando la variable (verificación: rojo hoy).
- [x] 1.4 Rust: test con `KESTO_ALLOW_INSECURE=0` y sin `OIDC_*` exige el mismo error que su ausencia (verificación: rojo hoy).
- [x] 1.5 Rust: test con las tres `OIDC_*` y `KESTO_SECRET`, sin permiso, exige modo `Oidc` (verificación: rojo hoy).
- [x] 1.6 Rust: test con `KESTO_ALLOW_INSECURE=1` y `BIND_ADDRESS=0.0.0.0:8080`, sin `OIDC_*`, exige error que **nombre `BIND_ADDRESS`** (verificación: rojo hoy).
- [x] 1.7 Rust: test con las tres `OIDC_*`, `KESTO_SECRET` y `BIND_ADDRESS=0.0.0.0:8080` exige configuración válida y escucha en `0.0.0.0:8080` (verificación: verde hoy; se conserva como contrapeso).
- [x] 1.8 Rust: test sin `BIND_ADDRESS` exige que el defecto sea `127.0.0.1:8080` (verificación: rojo hoy, que es `0.0.0.0:8080`).
- [x] 1.9 Verificación conjunta: `cargo test --locked --bin kesto 'config::tests::'` (verificación: falla solo por los casos nuevos).

### GREEN

- [x] 1.10 `src/config.rs`: constante `ENV_KESTO_ALLOW_INSECURE`, lectura y validación de formato (`1` permite, `0` niega, otro valor → `InvalidValue`), `DEFAULT_BIND_ADDRESS = "127.0.0.1:8080"` y las variantes `InsecureModeNotAllowed { name }` e `InsecureBindAddress { name }` en `ConfigError`; la comprobación cruzada del bind va **después** de `resolve_auth`, sin reordenar la lectura (verificación: 1.1–1.9 en verde).
- [x] 1.11 Rust: los tests existentes que cambian de contrato pasan a la forma nueva —`el_compose_del_srs_se_lee_sin_traduccion` (el `0.0.0.0:9090` del §8 se lee con proveedor de identidad) y `sin_variables_ganan_los_defectos_del_srs` (defecto de loopback y permiso explícito)— (verificación: `cargo test --locked --bin kesto 'config::tests::'` verde).

### REFACTOR

- [x] 1.12 Rustdoc de la variable, del permiso y de la regla de loopback —con el porqué— y `cargo fmt --all` (verificación: `cargo clippy --all-targets -- -D warnings` sin warnings).

## 2. Una credencial débil impide el arranque (SEC-02, #158 · SEC-03, #159)

### RED

- [x] 2.1 Rust: test con `KESTO_MASTER_KEY=` presente y vacía exige error nombrando la variable (verificación: rojo hoy, que arranca).
- [x] 2.2 Rust: test con `KESTO_MASTER_KEY` de 12 caracteres exige error nombrando la variable y el mínimo de 32 (verificación: rojo hoy, que avisa y arranca).
- [x] 2.3 Rust: test con las tres `OIDC_*` y `KESTO_SECRET=` vacía exige error nombrando `KESTO_SECRET` (verificación: rojo hoy).
- [x] 2.4 Rust: test con las tres `OIDC_*` y `KESTO_SECRET` de 12 caracteres exige error nombrando `KESTO_SECRET` (verificación: rojo hoy).
- [x] 2.5 Rust: test con credenciales de 64 caracteres exige configuración válida y **ningún aviso** sobre credenciales (verificación: rojo hoy, que no avisa pero sí por las cortas).
- [x] 2.6 Rust: test de que el mensaje del error **no incluye el valor** de la credencial (verificación: verde desde 2.8; `el_volcado_no_publica_credenciales` sigue verde).
- [x] 2.7 Rust: test en modo `Disabled` con `KESTO_MASTER_KEY` larga y sin `KESTO_SECRET` exige configuración válida —la ausencia no es debilidad— (verificación: verde hoy; se conserva como contrapeso).

### GREEN

- [x] 2.8 `src/config.rs`: variante `ConfigError::WeakCredential { name, min }` y validación en `from_lookup` para `KESTO_MASTER_KEY` siempre y `KESTO_SECRET` en modo `Oidc`; `warnings()` deja de informar de credenciales y conserva el aviso del modo sin autenticación (verificación: 2.1–2.7 en verde).
- [x] 2.9 `src/api/health.rs`: el test de la clave débil deja de asumir que arranca —pasa a comprobar el rechazo, o a clave larga más el aviso del modo— (verificación: `cargo test --locked --bin kesto 'api::health::'` verde).
- [x] 2.10 `src/config.rs`: `una_credencial_debil_avisa_y_una_larga_no` pasa a `una_credencial_debil_impide_el_arranque` (verificación: nombre nuevo y suite verde).

### REFACTOR

- [x] 2.11 Rustdoc de `warnings()` actualizado a lo que queda y `just check` en verde (fmt + clippy + tests) (verificación: termina sin error).
- [x] 2.12 Revisión de código: la longitud se mide en **caracteres** (`chars().count()`, no bytes), con test multibyte; test de `BIND_ADDRESS` vacía que no cae al defecto; y `dos_credenciales_largas_arrancan_sin_aviso` reforzado con `avisos.is_empty()` (verificación: `config::tests::` 35/35).

## 3. La gestión de tokens no se monta sin autenticación (SEC-01, #157)

### RED

- [x] 3.1 Rust: test de API —en modo `Disabled`, `GET /api/v1/tokens`— exige `404` con la envoltura (verificación: rojo hoy, que contesta `200`).
- [x] 3.2 Rust: test de API —`POST /api/v1/tokens` en modo `Disabled`— exige `404` y que **no se emita** ningún token (verificación: rojo hoy).
- [x] 3.3 Rust: test de API —`DELETE /api/v1/tokens/{id}` en modo `Disabled`— exige `404` (verificación: rojo hoy).

### GREEN

- [x] 3.4 `src/api/mod.rs`: `tokens::router()` sale de la rama `None`; `en_modo_disabled_la_gestion_queda_abierta` (`src/api/tokens.rs:658`) pasa a `en_modo_disabled_la_gestion_de_tokens_no_esta_montada`, que exige el `404` (verificación: 3.1–3.3 en verde).

### REFACTOR

- [x] 3.5 Comentario en la rama `Disabled` explicando por qué la gestión no se monta —un token sin credencial sobrevive al cambio de modo— (verificación: `just check` verde).

## 4. Arnés de pruebas, CI y plantilla de despliegue (SEC-01)

- [x] 4.1 `src/test_support.rs`: `config_con_dsn` (`:925`) y `estado_desarrollo` (`:1132`) declaran `KESTO_ALLOW_INSECURE=1` (verificación: `cargo test --locked --bin kesto` verde).
- [x] 4.2 `src/api/health.rs` (`:101`) y cualquier otro entorno de prueba que arranque sin OIDC (verificación: `cargo test --locked` verde).
- [x] 4.3 `tests/arranque.rs` (`:145`, `:502`) y `tests/e2e/arnes.rs` (`:947`): el proceso declara el permiso explícito (verificación: `cargo test --locked --test arranque` y `just e2e` verdes).
- [x] 4.4 `compose.yaml`: las tres `OIDC_*` y `KESTO_SECRET` con la forma `${VAR:?}`, sin `KESTO_ALLOW_INSECURE`, y un comentario que explique por qué la plantilla exige proveedor (verificación: `podman compose config` falla sin ellas y valida con ellas).
- [x] 4.5 `.github/workflows/release.yml` (`:147-183`): el smoke test pasa a la forma de producción —las tres `OIDC_*` y `KESTO_SECRET` en el `docker run`, `BIND_ADDRESS=0.0.0.0:8080`, puerto publicado— y espera `200` con `auth_mode: "oidc"` (verificación: el job de smoke test del workflow verde).
- [x] 4.6 `.env.example`: `KESTO_ALLOW_INSECURE` documentada con su aviso y las `OIDC_*`/`KESTO_SECRET` como obligatorias para el compose (verificación: el fichero las nombra y no trae ningún valor de secreto).
- [x] 4.7 `just check-all` en verde —backend, `scripts/` y panel— (verificación: termina sin error).

## 5. Documentación

- [x] 5.1 `docs/DEFINITION.md` §8: el modo de desarrollo con permiso explícito y bind de loopback, las variables nuevas y la plantilla que exige proveedor de identidad (verificación: el §8 las nombra).
- [x] 5.2 `docs/ENTORNO.md`: tabla de variables actualizada y **nota de actualización** —una clave maestra corta deja de arrancar— (verificación: la nota existe).
- [x] 5.3 `docs/ARQUITECTURA.md`: modo de desarrollo y por qué la clave del cifrado en reposo exige 32 caracteres (verificación: el apartado existe).

## 6. Cierre

- [x] 6.1 `docs/FEATURE-039-endurecimiento-arranque-auth.md` con `just feature-new` (verificación: el documento existe).
- [x] 6.2 `plans/PLAN-039.md` con las tareas marcadas y renombrado a `plans/PLAN-039-COMPLETED.md` (verificación: `git mv` hecho).
- [x] 6.3 PR a `development` con `Closes #157`, `Closes #158` y `Closes #159`, y la nota de incompatibilidad en el cuerpo (verificación: PR abierto). PR #170.
- [x] 6.4 `just spec-archive endurecimiento-arranque-auth` una vez fusionado (verificación: el delta entra en `openspec/specs/`; change archivado como `2026-11-21-endurecimiento-arranque-auth`).
- [x] 6.5 Issue de seguimiento abierto para la KDF con sal persistida (Argon2id/scrypt), fuera de alcance aquí (verificación: el issue existe y referencia #158). Issue #169.
