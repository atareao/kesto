# Spec Delta — SEC‑15: error_message redactado en endpoints públicos

## Contexto
Parte del change `grupo-5-sec-15-sec-22-resto` (refs #168). El endpoint `GET /api/v1/executions` y `GET /api/v1/restorations` devuelven `error_message` proveniente del motor. Hoy el mensaje se devuelve tal cual, pudiendo contener credenciales, rutas absolutas o datos sensibles del sistema.

## Contrato (POST‑change)
**Endpoints:** `GET /api/v1/executions`, `GET /api/v1/restorations`  
**Autenticación:** pública (exec) y sesión (restorations) según correspondencia  
**Respuesta exitosa (200):**
- `error_message`: string opcional — si está presente, debe ser un mensaje sin credenciales, sin rutas absolutas (sin `/`), y sin patrones de stack trace. Si no hay error, el campo debe ser `null`.

**Errores:**
- 400 si `error_message` contiene patrones prohibidos (credenciales, rutas `/*`, stack trace).
- 500 para fallos internos.

## Escenarios BDD

### Escenario 1 — Happy path: error_message legible, sin datos sensibles
**Dado** una ejecución completada con error (p.ej. fallo de backup)  
**Cuando** el cliente hace `GET /api/v1/executions?job_id=<id>`  
**Entonces** la respuesta es `200` con `error_message` que contiene un mensaje de usuario legible  
**Y** `error_message` **no** contiene la cadena `/` (rutas absolutas)  
**Y** `error_message` **no** contiene patrones de contraseña o tokens (ej. `password=`, `secret=`)  
**Y** el campo `status` refleja el estado de fallo

### Escenario 2 — Sin error: error_message es null
**Dado** una ejecución completada con éxito  
**Cuando** el cliente hace `GET /api/v1/executions?job_id=<id>`  
**Entonces** la respuesta es `200` con `error_message: null`

### Escenario 3 — Error: error_message contiene datos sensibles (rechazado)
**Dado** el motor devuelve `error_message: "Password 'admin' failed for db user"`  
**Cuando** el cliente hace `GET /api/v1/executions?job_id=<id>`  
**Entonces** la API devuelve `400` con `error: "invalid_error_message", message: "error_message contiene datos sensibles"`  
**Y** el campo `error_message` se omite o se establece en `null` después del saneado

### Escenario 4 — Edge case: error_message vacío
**Dado** el motor devuelve `error_message: ""`  
**Cuando** el cliente hace `GET /api/v1/executions?job_id=<id>`  
**Entonces** la respuesta es `200` con `error_message: null` (tratar vacío como sin error)

### Escenario 5 — Compatibilidad: probes que solo miran status
**Dado** un probe de infraestructura que solo verifica código `2xx` / `5xx`  
**Cuando** el probe consulta `/api/v1/executions`  
**Entonces** el comportamiento del probe **no cambia** (sigue funcionando igual, solo se valida el mensaje)

## Notas de implementación
- Crear una función de saneado `sanitize_error_message(msg: &str) -> Option<String>` en la capa API que elimine/reemplace caracteres riesgosos.
- Aplicar el saneado en los handlers `executions.rs` y `restorations.rs` antes de devolver la respuesta JSON.
- No leer `KESTO_VERSION` ni secrets en este endpoint.
- Mantener la ruta y el método idénticos; el cambio es solo en el payload.
- Actualizar `docs/DEFINITION.md` §5 y el doc de feature correspondiente.
