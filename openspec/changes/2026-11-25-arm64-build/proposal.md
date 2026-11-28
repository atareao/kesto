# Propuesta: Soporte arm64 (aarch64) en kesto

## Issue
- #129: Reactivar el build arm64 de la imagen (pausado temporalmente)

## Plan
- `plans/PLAN-052.md` (109 líneas)

## Intención
Habilitar la compilación y pruebas del backend Rust para `aarch64-unknown-linux-gnu`, actualizar CI con matriz arm64, Dockerfile multi-arch, y validar que el frontend y Docker funcionen en arm64.

## Alcance
- Backend Rust: target `aarch64-unknown-linux-gnu`, CI job arm64, scripts de verificación.
- Docker: Dockerfile multi-arch con `buildx`, base oficial arm64.
- Frontend: validar dependencias y build en arm64.
- Documentación: actualizar `docs/ENTORNO.md`, `docs/FEATURE-052-arm64.md`.

## Impacto
- CI: añadir matriz arm64, sin degradar x86_64.
- Docker: generar imágenes multi-arch.
- Documentación: nuevas secciones, sin cambios funcionales.

## Criterios de éxito
- `cargo build --locked --workspace --target aarch64-unknown-linux-gnu` sin errores.
- CI incluye job arm64 en verde.
- Docker construye imagen `linux/arm64`.
- `cargo run` en host arm64 y `/api/v1/health` responde.
- Panel compila y pasa pruebas en arm64.
- No hay regresiones en x86_64.

## Riesgos
- Falta de runner arm64 nativo: usar QEMU como fallback, job no bloqueante.
- Dependencias del frontend sin binarios arm64: documentar y evaluar reemplazo.
- Diferencias de versión de clientes de BD en arm64: validar `\restrict` en PostgreSQL.
