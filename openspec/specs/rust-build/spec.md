# rust-build Specification

## Purpose
TBD - created by archiving change 2026-11-25-chrono-migration. Update Purpose after archive.

## Requirements

### Requirement: Compilación cruzada arm64 sin chrono

La compilación cruzada debe mantener el target `aarch64-unknown-linux-gnu` instalado (`rustup target add aarch64-unknown-linux-gnu`) y el enlazador cruzado disponible, sin que `chrono` ni `chrono-tz` se usen en la compilación ni se enlacen. La receta `build-arm64` del `justfile` sigue encapsulando `cargo build --locked --workspace --target aarch64-unknown-linux-gnu` pero ya no depende de `chrono`; la receta `check-arm64` falla si la compilación no tiene éxito, sin importar la presencia de `chrono`.

#### Scenario: Build cruzado arm64

- **GIVEN** el target `aarch64-unknown-linux-gnu` instalado y `chrono` ya no es una dependencia directa.
- **WHEN** se ejecuta `cargo build --locked --workspace --target aarch64-unknown-linux-gnu`.
- **THEN** la compilación termina sin errores.

#### Scenario: Tests cruzados y puerta de dependencias

- **GIVEN** una compilación cruzada exitosa.
- **WHEN** se ejecuta `cargo test --locked --workspace --target aarch64-unknown-linux-gnu` y `cargo deny --workspace check`.
- **THEN** los tests pasan y `cargo deny --workspace check` no muestra la excepción de `RUSTSEC-2026-0555`; `RUSTSEC-2026-0543` solo aparece documentada por la cadena FTP inherente a `rustic_backend 0.7.0`.
