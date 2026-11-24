# Tasks — SEC‑15 / SEC‑22 (resto) (TDD)

> Protocolo: RED → GREEN → REFACTOR por cada hallazgo. Nada de código antes de aprobar el spec.
> Checks previos a cada commit: `just check` (fmt + clippy + test sobre el workspace).

## SEC‑15 — error_message redactado en endpoints públicos

### RED
- [x] Test: `GET /api/v1/executions` con job que tiene error devuelve `error_message` sin credenciales ni rutas `/`.
- [x] Test: `GET /api/v1/executions` job sin error devuelve `error_message: null`.
- [x] Test: `GET /api/v1/restorations` con error devuelve mensaje saneado.
- [x] Ejecutar `cargo test --locked` → confirmar ROJO en los tests nuevos y VERDE en los legacy.

### GREEN
- [x] Introducir función `sanitize_error_message` en `src/api/executions.rs` y `src/api/restorations.rs`.
- [x] Aplicar el saneado en los handlers antes de devolver la respuesta JSON.
- [x] `cargo test --locked` → VERDE; `cargo check` sin errores.

### REFACTOR
- [x] Unificar la lógica de saneado si se comparte entre executions y restorations (puede ir en un módulo `error_sanitizer` compartido).
- [x] Ajustar el panel UI para consumir el nuevo formato de `error_message`.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` y `cargo fmt --all -- --check` limpios.
- [x] Actualizar `docs/DEFINITION.md` §5 y el doc de feature con el nuevo contrato.

## SEC‑22 (resto) — socket :ro y validación de contenedores

### RED
- [x] Test: `POST /api/v1/repositories` con endpoint Docker sin etiqueta `io.kesto.backup-source=true` → `400 missing_label`.
- [x] Test: `POST /api/v1/repositories` con endpoint `http://socket` sin `KESTO_ALLOW_INSECURE_ENDPOINT=1` → `400 insecure_endpoint`.
- [x] Test: `POST /api/v1/repositories` con endpoint `https://socket` y etiqueta → `201`.
- [x] Ejecutar `cargo test --locked` → ROJO en los tests nuevos.

### GREEN
- [x] Implementar validación de endpoint y etiqueta en `src/api/repositories.rs` y `src/rustic/endpoint.rs` (o el punto de entrada correspondiente).
- [x] Añadir chequeo de `KESTO_ALLOW_INSECURE_ENDPOINT` en el borde HTTP.
- [x] `cargo test --locked` → VERDE; `cargo check` limpio.

### REFACTOR
- [x] Revisar que la validación no rompa flujos existentes; ajustar `endpoint_admisible` si es necesario.
- [x] Documentar en `docs/DEFINITION.md` §5 y `docs/FEATURE-050-<slug>.md` el nuevo comportamiento.
- [x] `cargo clippy` / `cargo fmt` limpios; actualizar docs de feature.

## Cierre
- [x] `just check` verde sobre el workspace.
- [x] `just check-all` verde (si aplica) antes de PR.
- [x] Crear `docs/FEATURE-050-<slug>.md` y archivar el change en `openspec/specs/` tras aprobación.
