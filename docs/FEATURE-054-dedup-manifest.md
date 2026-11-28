# FEATURE-054 — Manifest de cambios para deduplicación segura de volcados

## Qué es

Implementa el issue #206 (plan `plans/PLAN-054.md`, change `openspec/changes/dedup-manifest/`). Antes de archivar un volcado, el motor calcula un *manifest de cambios* estable y lo compara con el del último volcado. Si coincide, no se suben bytes de datos: se crea una snapshot nueva que apunta al mismo árbol. Si difiere, se sube el volcado completo. Si el cálculo falla, se degrada a subida forzada sin etiqueta y el backup no aborta.

La detección PostgreSQL usa `pg_dump --schema-only` más un checksum MD5 de datos por tabla. El manifest se guarda como tag `kesto.change_manifest=<hash>` en `SnapshotFile.tags`, nunca en `label` (que sigue identificando la tarea). La retención trata las snapshots con el mismo manifest como duplicados lógicos.

## Ficheros implicados

- `src/engine/extraccion.rs`: trait `ChangeDetector`, detector PostgreSQL inyectable, salidas acotadas y plazo de detección.
- `src/engine/ejecucion.rs`: cálculo del manifest en `copiar_el_volcado` y cableado con el archivado.
- `src/engine/restauracion.rs`: apoyo a ladetección con la orden adecuada.
- `src/rustic/mod.rs`: prefijo del tag de manifest.
- `src/rustic/repositorio.rs`: dedup por manifest, snapshot «sin cambios», lectura del último tag al abrir, resumen de snapshot.
- `src/rustic/retencion.rs`: compensación de duplicados lógicos por manifest.
- `src/api/snapshots.rs`: campo `change_manifest: Option<String>` en `GET /api/v1/snapshots`.
- `src/test_support.rs`: ayudante para sembrar snapshots con tag.
- `ui/src/pages/snapshots.tsx` y `ui/src/services/api/types.ts`: columna «Cambios» y contrato del cable.
- `docs/ENTORNO.md` y `compose.yaml`: variables `PGHOST`/`PGPORT` del detector.

## Cómo funciona

1. `copiar_el_volcado` construye el detector con `PGHOST`, `PGPORT` y `PGPASSWORD`, y calcula el manifest antes de archivar.
2. `archivar_volcado` compara con el último manifest leído del repositorio. Si coincide, drena el canal del volcado, crea una snapshot nueva sobre el mismo `tree` con su tag y devuelve un id propio; no sube datos.
3. Si difiere o no hay manifest previo, sube el volcado con `force(true)` salvo dedup, y etiqueta la snapshot.
4. La retención agrupa por manifest: retirar una de varias no libera los chunks compartidos; solo los libera retirar la última.
5. La API publica el hash sin prefijo en `change_manifest`; el panel muestra «sin cambios», «nuevos datos» o «—» según corresponda.

## Variables y configuración

`PGHOST` y `PGPORT` se leen tal cual (sin prefijo `KESTO_`) y se pasan al detector PostgreSQL; `PGPASSWORD` se inyecta desde la ejecución. Sin `PGHOST`/`PGPORT` el detector degrada a subida completa. El resto de variables siguen documentadas en `docs/ENTORNO.md`.

## Decisiones

- El manifest va en tag y no en `label`: `label` es el identificador de la tarea y sobrescribirlo rompería el contrato del cable.
- La ruta «sin cambios» crea una snapshot nueva con id propio sobre el mismo árbol, sin subir bytes: cumple el escenario del spec y mantiene la última snapshot real como contenido restaurable.
- La degradación por fallo de detección no aborta el backup; el manifest viejo se limpia cuando no se puede calcular.
- La detección MySQL/MariaDB (`mysqldump --no-data` + `CHECKSUM TABLE`) queda para la Fase 2 (T-2.1…T-2.4).

## Verificación

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`: limpios.
- `cargo test --locked --workspace --jobs 2`: workspace en verde (paquete raíz 868 tests, total workspace 932; 44 ignorados).
- `CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 just check`: exit 0.
- Panel: `npx vitest run src/pages/snapshots.test.tsx` 38/38, `pnpm test` 343 tests, `npx tsc --noEmit` y `pnpm run lint` en verde.
- Auditorías: `@rust-reviewer` y `@react-reviewer` aprobaron el código.

## Follow-ups

- Fase 2: detector MySQL/MariaDB y sus tests (T-2.1…T-2.4).
- Añadir `PGHOST`/`PGPORT` al resumen de variables sin prefijo de `AGENTS.md`.
- Obtener el manifiesto directamente del contenedor/target en lugar de depender del entorno del proceso.
- Resto de tareas de cierre: PR a `development`, archivado del change y renombrado del plan a `PLAN-054-COMPLETED.md`.
