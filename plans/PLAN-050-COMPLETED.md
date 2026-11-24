# PLAN-050: SEC-15 / SEC-22 (resto) — error_message redactado y socket :ro

## Objetivo
Cerrar hallazgos SEC‑15 (error_message sin sanear) y resto de SEC‑22 (socket Docker :ro, separación de privilegios, validación de etiqueta `io.kesto.backup-source`). Estos cambios forman parte del issue #168 de la campaña de seguridad #156.

## Alcance
- **SEC‑15:** Redactar `error_message` en los endpoints públicos `/api/v1/executions` y `/api/v1/restorations`. Introducir función `sanitize_error_message` en la capa API; tests que verifiquen que no haya credenciales ni rutas absolutas en el mensaje devuelto.
- **SEC‑22 (resto):** Validar en el borde HTTP que el endpoint Docker sea seguro (requiere `KESTO_ALLOW_INSECURE_ENDPOINT=1` para `http`/socket) y que el contenedor tenga la etiqueta `io.kesto.backup-source=true` antes de crear `ejec`. Confirmar `:ro` en compose y código que rechice escrituras.

## Issues referenciados
- #168 (campaña de seguridad #156)
- #173 (SEC‑06 2/2: separar privilegio motor/API)

## Tareas (checklist TDD)
- [ ] Aprobar change proposal y specs en `openspec/changes/grupo-5-sec-15-sec-22-resto/`.
- [ ] Implementar `sanitize_error_message` y aplicarlo en handlers `executions.rs` y `restorations.rs` — fase TDD RED/GREEN/REFACTOR.
- [ ] Añadir validación de endpoint Docker y etiqueta `io.kesto.backup-source` en `src/api/repositories.rs` y punto de entrada `exec` — fase TDD RED/GREEN/REFACTOR.
- [ ] Añadir/actualizar tests unitarios/integración para ambos cambios.
- [ ] Ejecutar `just check` y `just check-all` verdes sobre workspace.
- [ ] Actualizar `docs/DEFINITION.md` §5 y crear `docs/FEATURE-050-<slug>.md`.
- [ ] Archivar change en `openspec/specs/` y marcar plan como completado.

## Decisiones adoptadas
- Saneo de `error_message` en la capa API (no en persistencia) para evitar romper contratos de almacenamiento.
- Validación de etiqueta y socket es incremental: primera fase cubre el borde HTTP; motor (`rustic_core/endpoint.rs`) valida etiqueta al crear `exec`.
- Los cambios son compatibles con modo desarrollo mediante `KESTO_ALLOW_INSECURE_ENDPOINT=1`.

## Follow-ups / notas
- Tras fusionar, archivar change y crear `docs/FEATURE-050-<slug>.md`.
- Revisar logs de motor tras implementar saneo para asegurar que no hay errores por falta de credenciales en `error_message`.
- Coordinar con el change paralelo SEC‑20 (pins SHA) para evitar conflitos en `.github/workflows/`.
