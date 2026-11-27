# Spec Delta — SEC-21: el modo no se anuncia en `/health`

## Contexto

La spec canónica de `auth` afirma, al describir `GET /api/v1/auth/session`, que «eso ya lo dice la salud con su `auth_mode`». SEC-21 eliminó `auth_mode` del endpoint público de salud; la frase queda contradicha y hay que sustituirla.

## Requisitos (POST-change)

- La afirmación de que la salud expone `auth_mode` NO SHALL permanecer en la spec canónica de `auth`: el modo de autenticación NO SHALL publicarse como metadato.
- El modo SHALL comprobarse por comportamiento: `GET /api/v1/auth/session` sin cookie contesta `401` en modo OIDC y `404` en modo `Disabled`.
- NO SHALL añadirse un endpoint nuevo que publish el modo de autenticación.

## Escenarios BDD

### Escenario 1 — Cliente sin sesión en modo OIDC
**Dado** que la aplicación corre en modo OIDC  
**Cuando** se pide `GET /api/v1/auth/session` sin cookie  
**Entonces** contesta `401` con la envoltura.

### Escenario 2 — Cliente sin sesión en modo Disabled
**Dado** que la aplicación corre en modo `Disabled`  
**Cuando** se pide `GET /api/v1/auth/session` sin cookie  
**Entonces** contesta `404` con la envoltura, porque la ruta no está montada.
