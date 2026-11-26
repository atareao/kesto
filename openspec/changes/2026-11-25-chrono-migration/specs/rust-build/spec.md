# Especificación de compilación cruzada Rust para migración de chrono

## Contratos
- El target `aarch64-unknown-linux-gnu` sigue instalado (`rustup target add aarch64-unknown-linux-gnu`), pero ya no se usa `chrono` en la compilación.
- El enlazador cruzado sigue disponible, pero ya no se enlaza contra `chrono` ni `chrono-tz`.
- La receta `build-arm64` en `justfile` sigue encapsulando `cargo build --locked --workspace --target aarch64-unknown-linux-gnu`, pero ya no depende de `chrono`.
- La receta `check-arm64` falla si la compilación no tiene éxito, sin importar la presencia de `chrono`.

## Escenarios
- **Given** el target está instalado y `chrono` ya no es una dependencia, **When** se ejecuta `cargo build --locked --workspace --target aarch64-unknown-linux-gnu`, **Then** la compilación termina sin errores.
- **Given** la compilación exitosa, **When** se ejecuta `cargo test --locked --workspace --target aarch64-unknown-linux-gnu`, **Then** los tests pasan y `cargo deny --workspace check` no muestra las excepciones de RUSTSEC-2026-0543 ni RUSTSEC-2026-0555.
