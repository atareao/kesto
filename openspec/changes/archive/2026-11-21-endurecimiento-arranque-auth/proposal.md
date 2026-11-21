# Proposal — endurecimiento del arranque y de la autenticación

Issues **#157 (SEC-01)**, **#158 (SEC-02)** y **#159 (SEC-03)**, hijos de la auditoría de seguridad **#156**.

## Why

Los tres hallazgos comparten una misma raíz: `Config::from_lookup` (`src/config.rs:186`) comprueba **presencia** y trata la **fortaleza** de una credencial como un aviso, y el modo sin autenticación es lo que ocurre por omisión.

1. **SEC-01 (#157)** — sin las tres `OIDC_*` el modo es `Disabled` (`src/config.rs:340`), el bind por defecto es `0.0.0.0:8080` (`src/config.rs:58`) y en `Disabled` se monta **toda** la superficie de negocio sin capa (`src/api/mod.rs:136-156`), incluida `tokens::router()` (`:144`) —que emite credenciales persistentes— y `POST /api/v1/snapshots/{id}/restore`, que escribe en el host. El único rastro es un `WARN` (`src/config.rs:396-400`). Un `.env` que no se carga, o un `compose` que interpola a vacío, deja el despliegue abierto en todas las interfaces.
2. **SEC-02 (#158)** — `KESTO_MASTER_KEY` se acepta vacía: `env_lookup` devuelve `Some("")` para una variable definida pero vacía (`src/config.rs:178`), la comprobación es solo de presencia (`:189-193`) y la longitud corta solo avisa (`:403-407`). Una clave vacía hace que HKDF-SHA256 con sal `None` y la etiqueta constante `kesto/secrets/v1` (`src/db/crypto.rs:273-280`) derive una clave AES-256 **determinista y públicamente calculable**: quien lea el `.db` descifra `repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`.
3. **SEC-03 (#159)** — en modo OIDC, `KESTO_SECRET` vacía se acepta por el mismo camino y con la misma lógica: la sesión se firma con `EncodingKey::from_secret(secret.expose().as_bytes())` (`src/auth/session.rs:63`) y se verifica en cada petición protegida (`src/auth/middleware.rs:191`). Con la clave vacía —o con una de pocos caracteres, que hoy solo avisa— un JWT de sesión es forjable: **bypass de autenticación**.

Los tres son cambios sobre comportamiento **documentado**: el modo de desarrollo sin OIDC es una funcionalidad del §8 y `openspec/specs/deploy/spec.md:85` fija por especificación que una credencial corta «avisa y arranca igualmente».

## What Changes

### El modo sin autenticación deja de ser lo que pasa por omisión (SEC-01)

- Nueva variable opcional **`KESTO_ALLOW_INSECURE`**: `1` permite el modo `Disabled`, `0` lo niega, cualquier otro valor —vacío incluido— es error de formato que nombra la variable, y ausente equivale a negado. En `Disabled` sin el permiso el arranque falla nombrando la variable; con el modo `Oidc` la variable no tiene efecto.
- En modo `Disabled` la dirección de escucha efectiva debe ser de **loopback** (`127.0.0.0/8` o `::1`); si no lo es, el arranque falla nombrando `BIND_ADDRESS`. En modo `Oidc` no hay tal restricción: el §8 declara `0.0.0.0:8080` y sigue siendo legítimo.
- `DEFAULT_BIND_ADDRESS` pasa a **`127.0.0.1:8080`** (`src/config.rs:58`). Es una desviación **deliberada** del `BIND_ADDRESS=0.0.0.0:8080` que el §8 declara en su compose: escuchar en todas las interfaces pasa a ser una decisión explícita del operador y nunca el resultado de no haber dicho nada. El compose de referencia sigue declarándola tal cual.
- `tokens::router()` **deja de montarse** en `Disabled` (`src/api/mod.rs:144`): `GET/POST /api/v1/tokens` y `DELETE /api/v1/tokens/{id}` contestan `404` con la misma envoltura que cualquier ruta desconocida, exactamente como `/api/v1/auth/*` en ese modo. Un token emitido sin credencial sobrevive al cambio de modo, así que la ruta no puede existir donde no hay quien verifique nada. El panel no usa esas rutas (verificado: ninguna llamada a `/api/v1/tokens` en `ui/`).

### Una credencial débil deja de arrancar (SEC-02, SEC-03)

- `KESTO_MASTER_KEY` (siempre) y `KESTO_SECRET` (en modo `Oidc`) con menos de 32 caracteres —**vacía incluida**— son **error de arranque** que nombra la variable y el mínimo, no un aviso. `Config::warnings()` deja de informar de credenciales; conserva el aviso del modo sin autenticación.
- La credencial **ausente** sigue siendo otro caso: la decide el modo de autenticación, y sin OIDC `KESTO_SECRET` no hace falta.
- NO se cambia la KDF: HKDF-SHA256 con la etiqueta constante se queda como está (ver `design.md`).

### El despliegue de referencia exige proveedor de identidad (SEC-01)

- `compose.yaml` exige las tres `OIDC_*` y `KESTO_SECRET` con la forma `${VAR:?}`: `podman compose up` y su validación `compose config` se niegan a levantar el servicio sin proveedor, en vez de arrancar en abierto. NO declara `KESTO_ALLOW_INSECURE`.
- El smoke test de la imagen publicada (`.github/workflows/release.yml:147-183`) pasa a verificar la **forma de producción**: el contenedor arranca con las tres `OIDC_*` y `KESTO_SECRET` —el descubrimiento caído solo avisa, comportamiento ya probado en `tests/arranque.rs:114`—, con `BIND_ADDRESS=0.0.0.0:8080` y el puerto publicado, y el runner consulta `/api/v1/health` —público— esperando `200` con `auth_mode: "oidc"`. El motivo está en `design.md`: con el proceso atado a loopback, el puerto publicado ya no alcanza al servicio, así que un contenedor sin autenticación no puede verificar su puerto publicado.
- El arnés de pruebas y el e2e declaran el permiso explícito donde arrancan en modo `Disabled` a propósito: `src/test_support.rs` (`:925`, `:1132`), `src/api/health.rs:101`, `tests/arranque.rs:145,502` y `tests/e2e/arnes.rs:947`.

### Documentación

- `docs/DEFINITION.md` §8 (el modo de desarrollo y las variables nuevas), `docs/ENTORNO.md` (tabla de variables) y `docs/ARQUITECTURA.md` (modo de desarrollo y cifrado en reposo).

## Non-goals

- NO se cambia la KDF ni el sobre `v1:`: pasar a Argon2id/scrypt invalidaría los secretos ya cifrados y exige migración → issue de seguimiento.
- NO se cierra `POST /snapshots/{id}/restore` en modo `Disabled`, ni el `exec` sobre `container_name`, ni `host_path`/`db_path`: son SEC-04/05/06 (bloque B). Con el permiso explícito y el bind de loopback, el modo es opt-in y no expuesto.
- NO se toca la política de `Secure` de la cookie (SEC-09, bloque D) ni el contrato de `/api/v1/health`, que se queda en `auth_mode` + `version`.
- NO se añade ninguna dependencia nueva.

## Capabilities

### New Capabilities

_(ninguna)_

### Modified Capabilities

- `config`: el bind por defecto pasa a loopback; el modo sin autenticación exige permiso explícito y un bind de loopback.
- `secrets`: el requisito de la credencial débil se invierte — deja de ser un aviso y pasa a ser una puerta de arranque.
- `api-tokens`: en modo `disabled` la gestión de tokens no se monta.
- `deploy`: el compose de referencia exige OIDC; la verificación de la imagen se hace en la forma de producción; una credencial corta ya no arranca.
