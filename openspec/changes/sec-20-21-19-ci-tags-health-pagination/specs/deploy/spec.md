# Spec Delta — Release: smoke test de la imagen publicada

## Contexto

El workflow `.github/workflows/release.yml` arranca la imagen publicada por digest en configuración OIDC y comprobaba `auth_mode: "oidc"` en `GET /api/v1/health`. SEC-21 retiró ese campo del endpoint público para evitar fingerprinting. En v0.18.0 el smoke test falló con `{"status":200,"message":"servicio en marcha","data":{}}`, el job `Manifest (multi-arquitectura)` se saltó y la release quedó sin publicar completamente.

## Requisitos (POST-change)

- El smoke test de la imagen SHALL consultar `GET /api/v1/health` y verificar la envoltura actual: `.status == 200` y `.data == {}`. NO SHALL exigir `.data.auth_mode` ni ninguna otra clave de metadatos en esa respuesta.
- El smoke test SHALL verificar el modo OIDC **por comportamiento**: `GET /api/v1/auth/session` sin cookie SHALL contestar `401`. En modo `Disabled` esa misma ruta contesta `404`, de modo que la comprobación distingue ambos modos sin publicar metadatos.
- El smoke test SHALL conservar la comprobación de que el contenedor sigue en ejecución (`docker inspect -f '{{.State.Running}}'`) y el volcado de logs en caso de fallo.

## Escenarios BDD

### Escenario 1 — Imagen publicada en modo OIDC
**Dado** que el workflow arranca la imagen por digest con las variables `OIDC_*` y `KESTO_SECRET`  
**Cuando** el runner consulta `GET /api/v1/health` y luego `GET /api/v1/auth/session` sin cookie  
**Entonces** la salud es `200` con `.data == {}` y sin `auth_mode`  
**Y** la sesión contesta `401`  
**Y** el smoke test termina con éxito y el manifest multi-arquitectura se ensambla.

### Escenario 2 — Imagen arrancada en modo Disabled
**Dado** que el contenedor se levantó sin las variables `OIDC_*` (modo `Disabled`)  
**Cuando** el runner consulta `GET /api/v1/auth/session` sin cookie  
**Entonces** la respuesta es `404`  
**Y** el smoke test falla con un mensaje que nombra la ruta y el código esperado, sin llegar a ensamblar el manifest.
