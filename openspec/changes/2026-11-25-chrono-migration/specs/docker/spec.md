# Especificación Docker para migración de chrono

## Contratos
- Dockerfile no incluye `chrono` ni `chrono-tz`; la construcción usa `buildx` para `linux/amd64,linux/arm64`.
- La imagen resultante se publica en el workflow de release con `platforms: linux/amd64,linux/arm64`.
- `deny.toml` no contiene excepciones de RUSTSEC-2026-0543 ni RUSTSEC-2026-0555; si chrono persiste vía la cadena FTP, se documenta claramente la razón.

## Escenarios
- **Given** un commit a la rama `feature/chrono-migration`, **When** se ejecuta `docker buildx build --platform linux/amd64,linux/arm64 -t kesto:test .`, **Then** la imagen se construye para ambas arquitecturas sin errores y `deny.toml` no contiene las excepciones de RUSTSEC-2026-0543 ni RUSTSEC-2026-0555.
- **Given** la imagen construida, **When** se ejecuta `docker run` en un host arm64, **Then** el contenedor arranca y `/api/v1/health` responde, y `deny.toml` sigue sin las excepciones de RUSTSEC-2026-0543 y RUSTSEC-2026-0555 (o, si chrono persiste, se documenta la razón).
