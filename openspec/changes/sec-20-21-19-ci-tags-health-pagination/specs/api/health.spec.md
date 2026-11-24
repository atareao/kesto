# Spec Delta — SEC-21: `/health` público sin metadatos sensibles

## Contexto

Parte del change `sec-20-21-19-ci-tags-health-pagination` (refs #168).
El endpoint `GET /api/v1/health` es público (sin autenticación) y hoy devuelve:
```json
{ "status": "ok", "version": "0.1.0", "auth_mode": "oidc" }
```
Los campos `version` y `auth_mode` permiten fingerprinting y selección de vector de ataque.

## Contrato (POST-change)

**Endpoint:** `GET /api/v1/health`
**Autenticación:** pública (no requiere sesión/token)
**Respuesta exitosa (200):**
```json
{ "status": "ok" }
```
- `status`: string — valores permitidos `"ok"` | `"degraded"` | `"down"` (según lógica de healthchecks internos).
- **No** se incluyen `version` ni `auth_mode`.

**Errores:**
- 503 si el servicio se autodeclara `down` (cuerpo: `{ "status": "down" }`).
- 500 para fallos inesperados (cuerpo: envelope de error estándar).

## Escenarios BDD

### Escenario 1 — Happy path: servicio sano
**Dado** que la aplicación está levantada y todos los healthchecks internos pasan  
**Cuando** el cliente hace `GET /api/v1/health` sin credenciales  
**Entonces** la respuesta es `200 OK` con cuerpo `{ "status": "ok" }`  
**Y** el cuerpo **no** contiene la clave `version`  
**Y** el cuerpo **no** contiene la clave `auth_mode`

### Escenario 2 — Servicio degradado
**Dado** que al menos un healthcheck interno reporta degradado (p.ej. latencia de BD alta)  
**Cuando** el cliente hace `GET /api/v1/health`  
**Entonces** la respuesta es `200 OK` con cuerpo `{ "status": "degraded" }`  
**Y** el cuerpo **no** contiene `version` ni `auth_mode`

### Escenario 3 — Servicio caído
**Dado** que un healthcheck crítico falla (p.ej. BD inalcanzable)  
**Cuando** el cliente hace `GET /api/v1/health`  
**Entonces** la respuesta es `503 Service Unavailable` con cuerpo `{ "status": "down" }`  
**Y** el cuerpo **no** contiene `version` ni `auth_mode`

### Escenario 4 — Fallo interno inesperado
**Dado** que el handler de health lanza una excepción no controlada  
**Cuando** el cliente hace `GET /api/v1/health`  
**Entonces** la respuesta es `500 Internal Server Error` con envelope de error estándar  
**Y** el cuerpo **no** filtra `version` ni `auth_mode` (aunque el handler original los tuviera en scope)

### Escenario 5 — Edge case: petición con cabecera `Accept` distinta
**Dado** cualquier cabecera `Accept` (JSON, `*/*`, ausente)  
**Cuando** el cliente hace `GET /api/v1/health`  
**Entonces** la respuesta sigue el contrato anterior (solo `status`, sin metadatos)  
**Y** `Content-Type: application/json`

### Escenario 6 — Compatibilidad: probes que solo miran HTTP status
**Dado** un probe de infraestructura que solo verifica código `2xx` / `5xx`  
**Cuando** el probe consulta `/health`  
**Entonces** el comportamiento del probe **no cambia** (sigue funcionando igual)

## Notas de implementación

- El handler debe construir el JSON a mano o con un struct que **solo** tenga `status`.
- No leer `KESTO_VERSION` ni el modo de autenticación en este endpoint.
- Mantener la ruta y el método idénticos; el cambio es solo en el payload.
- Actualizar `docs/DEFINITION.md` §5 y el doc de feature correspondiente.