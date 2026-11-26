# Especificación CI para migración de chrono

## Contratos
- La matriz de CI incluye `time` o `jiff` como crates, sin `chrono` ni `chrono-tz`.
- Cada job debe ejecutar `cargo fmt`, `cargo clippy`, `cargo test --locked --workspace`, `cargo deny --workspace check` sin excepciones nuevas: se retira `RUSTSEC-2026-0555` y `RUSTSEC-2026-0543` se mantiene documentada por la cadena FTP.
- `chrono` persiste vía la cadena FTP inherente a `rustic_backend 0.7.0` (suppaftp → opendal-service-ftp → opendal → rustic_backend), por lo que `deny.toml` documenta claramente por qué la excepción `RUSTSEC-2026-0543` sigue siendo necesaria.

## Escenarios
- **Given** un push a la rama `feature/chrono-migration`, **When** el workflow CI se ejecuta, **Then** el job de CI pasa sin errores y `cargo deny --workspace check` no muestra excepciones nuevas: solo `RUSTSEC-2026-0543`, documentada por la cadena FTP inherente a `rustic_backend`; `RUSTSEC-2026-0555` ha desaparecido.
- **Given** que `chrono` sigue presente vía la cadena FTP inherente a `rustic_backend 0.7.0` (suppaftp → opendal-service-ftp → opendal → rustic_backend), **When** se ejecuta `cargo deny --workspace check`, **Then** la excepción `RUSTSEC-2026-0543` se mantiene en `deny.toml` con justificación clara de que `chrono` ya no es dependencia directa y de que no puede eliminarse por features.
