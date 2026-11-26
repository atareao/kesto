# Checklist TDD: migración de `chrono`/`chrono-tz` a `time` (issue #185)

Solo tareas de esta migración (cambio `openspec/changes/2026-11-25-chrono-migration`). Las tareas de arm64 pertenecen a otro change (PLAN-052) y no están aquí. Las pruebas del estado final ya están en `tests/chrono_migration.rs`.

## RED — escribir pruebas que fallen

- [x] Verificar que `tests/chrono_migration.rs` existe y define el estado final esperado (5 pruebas: dependencias directas de `Cargo.toml`, excepciones de `deny.toml`, `validacion.rs` sin `chrono`, `conversiones.rs` sin `chrono`).
- [x] Ejecutar `cargo test --locked` y confirmar que las pruebas de migración fallan (RED) mientras el resto de la suite permanece verde.

## GREEN — implementación mínima para poner las pruebas en verde

- [x] Reemplazar `croner::Cron` en `Agenda::analizada` por un analizador de cron nativo construido sin `chrono`.
- [x] Reemplazar `chrono::DateTime::from_timestamp` / `chrono::DateTime<Utc>` en `validacion.rs::proxima_tras` por conversión nativa de `time::OffsetDateTime`.
- [x] Eliminar `src/scheduler/hora.rs` (cruce `time`↔`chrono` para `croner`) y no declararlo en `src/scheduler/mod.rs`.
- [x] Dejar `src/scheduler/conversiones.rs` como marcador vacío que no mencione `chrono` ni `chrono-tz`.
- [x] Eliminar `chrono = "0.4"`, `tokio-cron-scheduler = "0.15.1"` y `croner = "3"` de `[dependencies]` en `Cargo.toml`.
- [x] Retirar la excepción `RUSTSEC-2026-0555` de `[advisories].ignore` en `deny.toml` y mantener `RUSTSEC-2026-0543` con su `reason` documentando la cadena FTP (`suppaftp → opendal-service-ftp → opendal → rustic_backend`) y que `chrono` ya no es dependencia directa.
- [x] Ejecutar `cargo test --locked`: `tests/chrono_migration.rs` y el resto de la suite pasan (GREEN).
- [x] Ejecutar `cargo deny --workspace --locked check`: verde con SOLO `RUSTSEC-2026-0543` documentada por la cadena FTP; `RUSTSEC-2026-0555` retirada.
- [x] Ejecutar `cargo tree -i chrono` y `cargo tree -i chrono-tz`: `chrono` solo devuelve la ruta `suppaftp → opendal-service-ftp → opendal → rustic_backend` (inherente a `rustic_backend 0.7.0`, no eliminable por features) y `chrono-tz` devuelve salida vacía.

## REFACTOR — limpieza sin cambiar comportamiento

- [x] `cargo fmt --all` y `cargo clippy --workspace --all-targets -D warnings`: sin advertencias nuevas.
- [x] Revisar que ninguna firma pública del scheduler nombre `chrono`, `chrono-tz`, `croner` ni `tokio_cron_scheduler`; actualizar docstrings de `Agenda`, `proxima_tras`, `validar` y `normalizar`.
- [x] Re-ejecutar `just check` (fmt → clippy → test): 100 % verde.

## ARCHIVE — consolidación

- [ ] Ejecutar `just check-all` (incluye puerta de dependencias y pruebas de `scripts/`).
- [ ] Marcar todas las casillas de este fichero como completadas.
- [ ] Ejecutar `openspec archive 2026-11-25-chrono-migration` para fusionar el delta en `openspec/specs/`.
