# Especificación Docker para migración de chrono

## ADDED Requirements

### Requirement: Imagen multiplataforma sin chrono directo

La matriz Docker debe construir con `buildx` para `linux/amd64,linux/arm64` sin incluir `chrono` ni `chrono-tz` como dependencias directas, y la imagen resultante se publica en el workflow de release con `platforms: linux/amd64,linux/arm64`. `deny.toml` no contiene la excepción `RUSTSEC-2026-0555`; la de `RUSTSEC-2026-0543` solo se conserva documentada si `chrono` persiste vía la cadena FTP inherente a `rustic_backend 0.7.0`.

#### Scenario: Construcción multiplataforma

- **GIVEN** un commit a la rama `feature/chrono-migration`.
- **WHEN** se ejecuta `docker buildx build --platform linux/amd64,linux/arm64 -t kesto:test .`.
- **THEN** la imagen se construye para ambas arquitecturas sin errores y `deny.toml` no contiene la excepción de `RUSTSEC-2026-0555`; `RUSTSEC-2026-0543` solo aparece con la razón documentada de la cadena FTP.

#### Scenario: Arranque de la imagen en arm64

- **GIVEN** la imagen construida.
- **WHEN** se ejecuta `docker run` en un host arm64.
- **THEN** el contenedor arranca y `/api/v1/health` responde, y `deny.toml` sigue sin la excepción de `RUSTSEC-2026-0555` y con `RUSTSEC-2026-0543` documentada por la cadena FTP si `chrono` persiste.
