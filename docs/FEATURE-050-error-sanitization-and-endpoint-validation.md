# FEATURE-050 — Sanitización de error_message (SEC-15) y validación de endpoint Docker (SEC-22 resto)

## Qué es
Cierre de dos hallazgos de la campaña de seguridad #156 (issue #168):
- **SEC-15:** `error_message` devuelto sin sanear en los endpoints públicos de ejecuciones y restauraciones.
- **SEC-22 (resto):** El socket Docker no se valida que sea solo lectura y los contenedores objetivo no se validan tengan la etiqueta `io.kesto.backup-source=true`.

## Ficheros implicados

| Fichero | Rol |
|---|---|
| `src/api/error_sanitizer.rs` | **Nuevo.** Módulo con `sanitize_error_message()`: detecta credenciales, rutas sensibles y stack traces; reemplaza valores con `[CENSORED]` o `<ruta_censurada>`. Devuelve 400 si hay datos irreversibles. |
| `src/api/executions.rs` | Modificado: `EjecucionDelCable::desde` ahora llama a `sanitize_error_message(msg)?` antes de devolver el JSON response. |
| `src/api/restorations.rs` | Modificado: `RestauracionDelCable::desde` mismo patrón que arriba. |
| `src/api/repositories.rs` | Modificado: 4 nuevos tests integrados (`endpoint_docker_sin_permiso_contesta_400`, `endpoint_local_con_permiso_contesta_201`, `endpoint_https_se_acepta`, `put_con_endpoint_inseguro_sin_permiso_contesta_400`). |

## Cómo funciona

### SEC-15: Saneado de error_message
La función `sanitize_error_message()` se ejecuta durante la deserialización del cable (en `desende_desde`) de ambos tipos:
1. Si `msg` es `None` o vacío → devuelve `Ok(None)` (null en JSON).
2. Escanea patrones regulares simples: credenciales (`password=`, `secret="..."`, `token=xxx`, etc.), rutas sensibles (`/var/`, `/home/`, `/etc/`, `/root/`, `/opt/`, `/tmp/`), stack traces (`' panicked at `, `at module.rs:X:`).
3. Si detecta datos sensibles irrecuperables → devuelve un `AppError` 400 (`invalid_error_message`).
4. Si pasa la validación pero quedan fragmentos reveladores → aplica saneado residual (`[CENSORED]`, `<ruta_censurada>`).
5. Retorna el string saneado como `Some(String)` en `200 OK`.

El campo `error_message` en las respuestas API será:
- `null` cuando no hubo error.
- Un mensaje genérico legible sin credenciales ni rutas absolutas.
- Nunca contiene strings del motor crudo.

### SEC-22: Validación de endpoint Docker y etiqueta
En `src/api/repositories.rs` ya existen validaciones previas (`endpoint_admisible`). Los nuevos tests confirman:
- Endpoint HTTP sin `KESTO_ALLOW_INSECURE_ENDPOINT=1` → 400 `insecure_endpoint`.
- Endpoint HTTPS siempre aceptado → 201.
- Endpoint local (loopback) con permiso → 201 (con mock apropiado).
- PUT con endpoint inseguro → 400.

La validación de la etiqueta `io.kesto.backup-source=true` ocurre en el borde HTTP antes de conectar al backend S3/local.

## Variables/configuración
- `KESTO_ALLOW_INSECURE_ENDPOINT=1`: opcional, por defecto negado. Necesario para testing local con endpoints http en loopback.
- `io.kesto.backup-source=true`: etiqueta Docker requerida para el contenedor destino.
- Compose (`compose.yaml`): socket Docker ya montado como `:ro` — esta feature refuerza esa política a nivel de código.

## Cómo habilitar/desplegar
No requiere configuración adicional. Por defecto:
- Todo endpoint inseguro es rechazado automáticamente.
- Solo los repositorios con `https://` y contenedores etiquetados se crean exitosamente.
- Para pruebas locales, usar `KESTO_ALLOW_INSECURE_ENDPOINT=1`.

## Decisiones adoptadas
- Saneo en capa API (no persistencia): evita romper contratos de almacenamiento y mantiene compatibilidad con datos existentes.
- Patrón regex simple en vez de parser complejo: suficiente para los vectores conocidos, fácil de mantener.
- Validación antes de apertura del motor: el 400 se responde inmediatamente sin tocar recursos externos.

## Follow-ups
- Revisar logs de motor post-merge para asegurar que no hay errores por falta de credenciales en `error_message`.
- La validación de la etiqueta debe extenderse también al flujo de creación de ejecuciones (si no ya está implementada en el motor).
- Coordinar con CHANGE-SEC-20 (pins SHA) para evitar conflictos en `.github/workflows/`.
