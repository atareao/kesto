# PLAN-048.md

## Issue
#168 - endurecimiento de la sesión y de la frontera de confianza (SEC-16 + SEC-17)

## Change
`endurecimiento-oidc`

## Documentación
`docs/FEATURE-048-endurecimiento-oidc.md`

## Rama
`feature/endurecimiento-oidc`

## Objetivo
Implementar los NIT de SEC-22 que tocan el flujo OIDC y el hash de los tokens de API:
1. Caché JWKS con TTL (1 hora) y recarga única por `kid` desconocido
2. `nonce` en tiempo constante usando `subtle::ct_eq`
3. PKCE S256 en el flujo de login y canje
4. Hash del token de API con HMAC-SHA256 y secreto derivado de `KESTO_MASTER_KEY`

## Alcance
- Ficheros: `src/auth/oidc.rs`, `src/state.rs`, `src/db/entities/api_token.rs`, `src/db/entities/sesion_revocada.rs`, `src/db/migrates/0010_sesiones_revocadas.sql`, `openspec/changes/endurecimiento-oidc/`, `openspec/changes/endurecimiento-oidc/specs/`, `plans/PLAN-048.md`, `docs/FEATURE-048-endurecimiento-oidc.md`
- Decisiones clave: caché JWKS, `nonce` constante en tiempo, PKCE S256, HMAC para tokens

## Tareas TDD numeradas

0. Preparación
- [x] 0.1 Punto de partida en verde: `just check` (verificación: termina sin error)
- [x] 0.2 Rama desde `development`: `just gf-feature endurecimiento-oidc`
- [x] 0.3 Change aprobado por el usuario (verificación: `just check-spec` encuentra change activo)

1. Caché JWKS con TTL (1 hora)
- [x] 1.1 RED: `src/auth/oidc.rs` y `src/state.rs` sin caché
- [x] 1.2 GREEN: Implementar caché de JWKS con TTL en `AuthState` (junto a `discovery`), recarga única por `kid` desconocido
- [x] 1.3 VERIFICACIÓN: `just check` y `just check-all` en verde

2. `nonce` en tiempo constante
- [x] 2.1 RED: `src/auth/oidc.rs` usa `!=` para comparar `nonce`
- [x] 2.2 GREEN: Cambiar a `subtle::ct_eq` para comparación constante en tiempo
- [x] 2.3 VERIFICACIÓN: tests de integración cubren el flujo completo

3. PKCE S256
- [x] 3.1 RED: `build_authorization_url` no envía `code_challenge` ni `code_challenge_method`
- [x] 3.2 GREEN: Añadir `code_challenge` (SHA-256 sin relleno) y `code_challenge_method=S256` a la redirección; guardar `code_verifier` con el login pendiente
- [x] 3.3 GREEN: `canjear_codigo` envía `code_verifier` en el POST al `token_endpoint`
- [x] 3.3 VERIFICACIÓN: test de flujo completo con PKCE

4. Hash del token de API con HMAC
- [x] 4.1 RED: `hash_de_token` usa SHA-256 pelado
- [x] 4.3 GREEN: Cambiar a HMAC-SHA256 con clave derivada de `KESTO_MASTER_KEY` (HKDF, etiqueta `kesto/api-tokens/v1`) y base64 codificar
- [x] 4.3 VERIFICACIÓN: los tokens existentes dejan de validar (requieren reemitir); la búsqueda por `token_hash` sigue funcionando

5. Verificación final
- [x] 5.1 `just check` (fmt + clippy + test) en verde
- [x] 5.2 `just check-all` (análogo local del CI) en verde
- [x] 5.3 Suite de pruebas de integración cubre: login con PKCE, canje con `code_verifier`, verificación de ID token con caché JWKS, hash de token con HMAC

6. Documentación y cierre
- [x] 6.1 `docs/FEATURE-048-endurecimiento-oidc.md` completa
- [ ] 6.2 `openspec archive endurecimiento-oidc`
- [ ] 6.3 Renombrar `plans/PLAN-048.md` a `plans/PLAN-048-COMPLETED.md`
- [x] 6.4 Documentar en `docs/ARQUITECTURA.md` la frontera mono-operador (SEC-16)