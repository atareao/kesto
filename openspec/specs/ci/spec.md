# ci Specification

## Purpose
TBD - created by archiving change 2026-11-25-chrono-migration. Update Purpose after archive.

## Requirements

### Requirement: Matriz CI sin excepciones nuevas

La matriz CI debe incluir `time` o `jiff` como crates, sin `chrono` ni `chrono-tz`, y cada job debe ejecutar `cargo fmt`, `cargo clippy`, `cargo test --locked --workspace` y `cargo deny --workspace check`. Se retira `RUSTSEC-2026-0555` y `RUSTSEC-2026-0543` se mantiene documentada por la cadena FTP inherente a `rustic_backend 0.7.0` (`suppaftp → opendal-service-ftp → opendal → rustic_backend`), que explica por qué `chrono` sigue en el grafo sin ser dependencia directa.

#### Scenario: CI verde con solo la excepción 0543 documentada

- **GIVEN** un push a la rama `feature/chrono-migration`.
- **WHEN** el workflow CI se ejecuta.
- **THEN** el job de CI pasa sin errores y `cargo deny --workspace check` no muestra excepciones nuevas: solo `RUSTSEC-2026-0543`, documentada por la cadena FTP inherente a `rustic_backend`; `RUSTSEC-2026-0555` ha desaparecido.

#### Scenario: Excepción FTP justificada en deny.toml

- **GIVEN** que `chrono` sigue presente vía la cadena FTP inherente a `rustic_backend 0.7.0` (`suppaftp → opendal-service-ftp → opendal → rustic_backend`).
- **WHEN** se ejecuta `cargo deny --workspace check`.
- **THEN** la excepción `RUSTSEC-2026-0543` se mantiene en `deny.toml` con justificación clara de que `chrono` ya no es dependencia directa y de que no puede eliminarse por features.
