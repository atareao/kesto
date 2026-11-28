# Especificación CI para arm64

## Contratos
- La matriz de CI incluye `target: aarch64-unknown-linux-gnu` y `runner: ubuntu-24.04-arm`.
- Cada job debe ejecutar `cargo fmt`, `cargo clippy`, `cargo test --locked --workspace`, `cargo deny --workspace check` para ambos targets.
- Si el runner arm64 nativo no está disponible, usar QEMU (`--platform linux/arm64`) y marcar el job como no bloqueante.

## Escenarios
- **Given** un push a una rama `feature/arm64-build`, **When** el workflow CI se ejecuta, **Then** el job de arm64 pasa sin errores y el job x86_64 sigue verde.
- **Given** que el runner arm64 nativo no está disponible, **When** el job se ejecuta con QEMU, **Then** los tests se ejecutan y el job no falla, pero se documenta como no bloqueante hasta que exista runner nativo.
