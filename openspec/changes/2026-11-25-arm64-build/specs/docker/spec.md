# Especificación Docker para arm64

## Contratos
- Dockerfile usa base oficial con variante arm64 (p. ej., `debian:<version>-slim` o `arm64v8/alpine`).
- La construcción se realiza con `docker buildx` para `linux/amd64,linux/arm64`.
- La imagen resultante se publica en el workflow de release con `platforms: linux/amd64,linux/arm64`.

## Escenarios
- **Given** un commit a la rama `feature/arm64-build`, **When** se ejecuta `docker buildx build --platform linux/amd64,linux/arm64 -t kesto:test .`, **Then** la imagen se construye para ambas arquitecturas sin errores.
- **Given** la imagen construida, **When** se ejecuta `docker run` en un host arm64, **Then** el contenedor arranca y `/api/v1/health` responde.
