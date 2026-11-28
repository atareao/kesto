# Especificación de compilación cruzada Rust para arm64

## Contratos
- El target `aarch64-unknown-linux-gnu` está instalado en el entorno de desarrollo (`rustup target add aarch64-unknown-linux-gnu`).
- El enlazador cruzado (`aarch64-linux-gnu-gcc` o `clang`) está disponible.
- La receta `build-arm64` en `justfile` encapsula `cargo build --locked --workspace --target aarch64-unknown-linux-gnu`.
- La receta `check-arm64` falla si la compilación no tiene éxito.

## Escenarios
- **Given** el target está instalado, **When** se ejecuta `cargo build --locked --workspace --target aarch64-unknown-linux-gnu`, **Then** la compilación termina sin errores.
- **Given** la compilación exitosa, **When** se ejecuta `cargo test --locked --workspace --target aarch64-unknown-linux-gnu`, **Then** los tests pasan.
