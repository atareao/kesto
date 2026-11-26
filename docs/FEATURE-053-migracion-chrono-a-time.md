# FEATURE-053 — Migración de chrono/chrono-tz a `time` (issue #185)

## Qué es

Eliminación de las dependencias deprecadas `chrono`, `chrono-tz`, `croner` y
`tokio-cron-scheduler` del scheduler de Kesto, sustituidas por un analizador y
un reloj nativos construidos sobre `time::OffsetDateTime`. El cambio reduce la
superficie de dependencias, cierra el aviso `RUSTSEC-2026-0555` y deja una
única familia de tipos de fecha en el componente de programación.

La funcionalidad observable no cambia: las expresiones de programación siguen
admitiendo listas, rangos, pasos y año; el disparo sigue siendo secuencial por
repositorio y el cliente SSE sigue recibiendo el mismo payload del §5 del SRS.

## Ficheros implicados

- `Cargo.toml` — baja `chrono`/`chrono-tz`, `croner` y `tokio-cron-scheduler`; sube `time`.
- `Cargo.lock` — grafo resultante, sin croneros ni tokio-cron-scheduler.
- `deny.toml` — retira la excepción de `RUSTSEC-2026-0555`; documenta `RUSTSEC-2026-0543`.
- `kesto-mcp/Cargo.toml` — `rmcp` a 3.12 para cortar la ruta que arrastraba `chrono`.
- `src/scheduler/validacion.rs` — validación de la expresión con el parser nativo.
- `src/scheduler/planificacion.rs` — `Agenda::proxima_tras` sobre `time::OffsetDateTime`.
- `src/scheduler/disparo.rs` — `NativeScheduler` con `notify_one` y apagado acotado.
- `src/scheduler/mod.rs` — exports y cableado del nuevo scheduler.
- `src/scheduler/conversiones.rs` — conversiones entre `time` y los tipos de dominio.
- `src/scheduler/hora.rs` — eliminado; su lógica pasa a `conversiones.rs`.
- `tests/chrono_migration.rs` — prueba de regresión de la migración.
- `openspec/changes/2026-11-25-chrono-migration/` — propuesta y deltas aprobados.
- `plans/PLAN-053.md` — plan de ejecución de la feature.

## Cómo funciona

`Agenda::proxima_tras` calcula la siguiente ejecución en UTC de forma
determinista: la expresión se interpreta como una serie de instantes y se
resuelve con `OffsetDateTime`, de modo que un mismo estado produce siempre la
misma respuesta. Los offsets explícitos y el cambio horario (DST) se tratan
como datos de la zona, no como reglas repartidas por el scheduler.

El parser propio acepta listas (`1,15,30`), rangos (`1-5`), pasos (`*/5`) y un
campo de año opcional. La validación ocurre antes de aceptar la configuración
del repositorio y devuelve un error que nombra el campo, sin depender de
`croner` para interpretar la gramática.

`NativeScheduler` mantiene un permiso guardado para despertar al worker:
`notify_one()` señala el recálculo cuando cambia una agenda, y el apagado se
acota con `tokio::time::timeout` para que un ciclo pendiente no retenga el
proceso. No se introduce un task por job: se conserva un worker secuencial
por `repository_id`, con su cola MPSC.

## Variables/configuración

No hay variables nuevas. `RUSTSEC-2026-0543` permanece documentada en
`deny.toml` por la cadena FTP `suppaftp → opendal-service-ftp → opendal →
rustic_backend 0.7.0`: es inherente a la versión del backend y no se puede
desactivar mediante features. `RUSTSEC-2026-0555` desaparece del grafo y, por
tanto, de la lista de excepciones.

## Decisiones

1. `rmcp` se sube a 3.12 en `kesto-mcp` para eliminar `chrono` de esa ruta sin
   cambiar el protocolo MCP expuesto.
2. La ruta FTP se acepta como inherente y se documenta en `deny.toml` en vez
   de introducir un parche o un backend alternativo.
3. Se conserva `time` como única familia de fechas del scheduler: no conviven
   dos representaciones ni conversiones implícitas entre librerías.

## Verificación

- `just check` en verde (fmt, clippy y test del workspace).
- `just check-all` en verde (suma de deny, scripts y panel).
- `cargo deny --workspace --locked check` en verde.
- `cargo tree -i chrono` devuelve `chrono` solo por la rama FTP.

## Follow-ups

- Resolver la cadena FTP cuando `rustic_backend` ofrezca una configuración
  futura sin `services-ftp` y retirar entonces la excepción 0543.
- Cerrar el issue #185, archivar el cambio OpenSpec y renombrar el plan a
  `plans/PLAN-053-COMPLETED.md` tras el merge a `development`.
