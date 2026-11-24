# Proposal — SEC-15 / SEC-22 (resto)

Issues **SEC-15** (error_message sin sanear) y **SEC-22** (resto: socket :ro, privilegios, validación contenedores) de la auditoría de seguridad **#156** (#168).

## Why

### SEC‑15: error_message expone datos sensibles

Los endpoints públicos `/api/v1/executions` y `/api/v1/restorations` devuelven `error_message` crudo del motor. Un fallo de backup puede contener credenciales (`password=admin`), rutas absolutas (`/var/lib/mysql/data`) o stack traces del sistema. Quien consume `/api/v1/*` (incluyendo clientes externos) obtiene esa información en claro, violando la política de mínima divulgación. El cierre requiere sanitizar toda respuesta antes de serializarla a JSON.

### SEC‑22: superficie Docker no blindada

El socket Docker se monta como `:ro` en compose, pero el código no verifica esta política en el borde HTTP. Adicionalmente, los contenedores objetivo deben declarar la etiqueta `io.kesto.backup-source=true`; sin ella, el motor ejecuta `exec` sobre cualquier contenedor arbitrario. Issue #173 (SEC‑06 2/2) requiere separar los privilegios del motor (rustic_core) de los de la API: ambos operan en el mismo proceso, así que una falta de aislamiento permite que un cliente malicioso ejecute operaciones de Docker directamente.

Ambos hallazgos son BAJO/NIT pero amplificables: un atacante que consiga acceso a exec/restoration o inyecte un contenedor comprometido podría obtener credenciales persistentes o ejecutar comandos remotos.

## What Changes

### Saneado de error_message (SEC-15)

- Nuevo módulo `src/api/error_sanitizer.rs` con `sanitize_error_message(msg: Option<&str>) -> Result<Option<String>, AppError>`:
  - Detecta patrones de credenciales (`password=`, `secret=`, `token=xxx`, etc.) → devuelve `400 invalid_error_message`.
  - Detecta rutas sensibles (`/var/`, `/home/`, `/etc/`, `/root/`) → devuelve `400` si son irrecuperables; sino aplica `<ruta_censurada>`.
  - Detecta stack traces (`' panicked at `, `at module.rs:X:`) → devuelve `400`.
  - Limpia fragmentos residuales con `[CENSORED]` cuando sea posible.
- `src/api/executions.rs`: `EjecucionDelCable::desde` aplica `sanitize_error_message()` antes de devolver la respuesta.
- `src/api/restorations.rs`: mismo patrón en `RestauracionDelCable::desde`.

### Validación de endpoint Docker y etiqueta (SEC-22)

- Refuerzo de validación existente en `src/api/repositories.rs`:
  - Endpoint inseguro sin `KESTO_ALLOW_INSECURE_ENDPOINT=1` → `400 insecure_endpoint`.
  - Endpoint HTTPS siempre aceptado → 201/200.
  - Endpoint local (loopback) con permiso → 201.
  - PUT con endpoint inseguro → 400.
- Test adicionales confirmados: `endpoint_docker_sin_permiso_contesta_400`, `endpoint_local_con_permiso_contesta_201`, `endpoint_https_se_acepta`, `put_con_endpoint_inseguro_sin_permiso_contesta_400`.

### Tests añadidos

- 4 tests por cada tipo de endpoint para SEC-15: `null sin error`, `credenciales→400`, `vacio→null`, `stack trace→400` (ejecutions + restorations = 8 tests nuevos).
- 4 tests para SEC-22 en repositories.rs.

## Cierre

Se completa el grupo 5 de la campaña de seguridad #156. Quedan abiertos: SEC-06(1/2) en issue #173 (parte resolvida aquí, parte pendiente RBAC), SEC-13(cotas), SEC-14(rango keep_*).

## Impacto

- **Contrato API:** `error_message` será siempre null o saneado; consumidores旧que lo leían directo deben usar el nuevo formato saneado.
- **Motor:** Se añade validación previa a exec; cambios mínimos en el borde HTTP.
- **CI/workflows:** Sin cambios directos; pins SHA (SEC-20) siguen en paralelo.
- **Documentación:** `docs/DEFINITION.md` §5 y docs de feature actualizados.
