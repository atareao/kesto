# PLAN-055 — Fase 2: detector MySQL/MariaDB para deduplicación por manifest

- Issue de entrada: #206 (https://github.com/atareao/kesto/issues/206).
- Change OpenSpec: `openspec/changes/dedup-manifest/` (activo; escenarios ya aprobados).
- Rama: `feature/mysql-change-detector`, creada desde `development`.
- Alcance: completar T-2.1…T-2.4 y T-4.1. La Fase 1 (PostgreSQL) queda intacta.

## 1. Objetivos

1. Añadir un `ChangeDetector` para MySQL/MariaDB que produzca el mismo tipo de manifest que PostgreSQL: `Some(<sha256 hex>)` cuando hay esquema y datos estables, `None` cuando la detección falla.
2. Usar `mysqldump --no-data` para la estructura y `CHECKSUM TABLE <db>.<tabla> EXTENDED` para los datos. `EXTENDED` es obligatorio: fuerza lectura fila a fila y evita el checksum «live» de MyISAM/Aria con `CHECKSUM=1`.
3. Reutilizar sin cambios el etiquetado `kesto.change_manifest`, la retención por manifest y el pipeline de archivado de la Fase 1.
4. Mantener las restricciones del SRS: streaming en memoria, sin ficheros temporales, salida acotada, sin shell, sin credenciales en logs y degradación sin abortar el backup.
5. Cubrir los escenarios de `openspec/changes/dedup-manifest/specs/engine/spec.md` con tests unitarios mediante el inyector de ejecución ya existente, y verificar el workspace completo con `just check-all`.
6. Cerrar el change: marcar T-4.1, ejecutar `openspec archive dedup-manifest`, renombrar este plan a `plans/PLAN-055-COMPLETED.md`, documentar la feature y cerrar el issue #206.

## 2. Mapa arquitectónico

| Capa | Fichero | Cambio previsto |
|---|---|---|
| Motor / detección | `src/engine/extraccion.rs` | Mantener `ChangeDetector`, `SalidaDeDeteccion`, `ErrorDeDeteccion`, `leer_con_tope`, `ejecutar_en_el_sistema`, `calcular_manifest_con` y `PostgresChangeDetector`. Añadir `MysqlChangeDetector` (o名前 equivalente en español del repo) junto a él, con el mismo contrato y dominios de hash separados por `\0`. |
| Motor / orquestación | `src/engine/ejecucion.rs` | Ampliar `detector_de_produccion` para elegir el detector MySQL/MariaDB según el motor del repositorio, reutilizando la configuración del cliente `mariadb-dump`/`mariadb` ya usada en la extracción. El aviso de degradación debe dejar de ser específico de `pg_dump`. |
| Rustic / etiquetado | `src/rustic/mod.rs`, `src/rustic/repositorio.rs`, `src/rustic/retencion.rs` | **Sin cambios.** Reutilizar `PREFIJO_DE_MANIFEST`, `manifiesto_de`, `archivar_volcado`, `add_tags` (conserva `label`) y la retención por manifest. |
| API / panel | `src/api/snapshots.rs`, `ui/src/**` | **Sin cambios.** `change_manifest` y la columna «Cambios» ya funcionan. |
| OpenSpec | `openspec/changes/dedup-manifest/tasks.md` | Marcar T-2.1…T-2.4 y T-4.1 a medida que se cumplen. No crear deltas nuevos: los escenarios de Fase 2 ya están en `specs/engine/spec.md`. |
| Documentación | `docs/FEATURE-055-mysql-change-detector.md` | Nuevo documento de feature emparejado con este plan. Referenciar `docs/FEATURE-054-dedup-manifest.md` y langs `FEATURE-055`. |
| Proceso | `HANDOFF.md` | Actualizar al cierre (fichero ignorado, no versionado). |

Nota de vocabulario: si el repo usa nombres en español para tipos, mantener la convención (`MysqlChangeDetector` es un nombre tentativo; el implementador debe respetar el estilo de `PostgresChangeDetector`).

## 3. Contrato técnico

- Entrada: lista de bases/tablas obtenida del propio cliente MySQL, ordenada de forma determinista.
- Esquema: salida de `mysqldump --no-data`, leída con `leer_con_tope` (8 MiB) y sin escribir a disco.
- Datos: para cada tabla, `CHECKSUM TABLE \`db\`.\`tabla\` EXTENDED`, con identificadores escapados; se rechaza cualquier nombre que no pueda representarse de forma segura.
- Manifest: SHA-256 con dominios separados, p. ej. `esquema\0<dump>\0datos\0<db>|<tabla>|<checksum>\n…`, hexadecimal en minúsculas. Debe ser estable entre ejecuciones idénticas y distinto ante cambio de esquema o de datos.
- Fallo, timeout (30 s), salida excedida,Tabla inexistente o checksum `NULL`: `change_manifest()` devuelve `None` y registra `warn`; la copia continúa y se archiva sin tag.
- Seguridad: invocation por argv, nunca `sh -c`; sin metacomandos del volcado ejecutados; contraseña solo por entorno del proceso, nunca en argumentos ni en logs.

## 4. Seams de test (pre-acordados en este plan)

1. `ChangeDetector::change_manifest()` con el ejecutor falso de `extraccion.rs` (`ejecutor_falso`/clausura que simula stdout, stderr y argumentos). Se observa solo el `Option<String>` devuelto y los argumentos de los comandos.
2. `detector_de_produccion` + `DetectorFalso` en `ejecucion.rs`: el manifest llega al archivado y su fallo no aborta la copia.
3. Contratos ya existentes de `repositorio.rs` y `retencion.rs`: no se añaden tests nuevos salvo que el cambio de factory lo exija.

## 5. Tareas TDD

- **P-055.1 / T-2.1 (RED).** Escribir los tests de contrato del detector MySQL/MariaDB con el inyector existente: (a) mismo esquema + mismos datos → mismo manifest; (b) datos distintos → manifest distinto; (c) esquema distinto → manifest distinto; (d) comando que no arranca / termina con error / excede tope → `None` + `warn`; (e) `CHECKSUM TABLE … EXTENDED` aparece en los argumentos y no `QUICK`; (f) el hash no depende del orden de llegada de stdout/stderr. Ejecutar los tests filtrados por CLI y verificar que fallan por ausencia del detector, mientras los tests PostgreSQL siguen verdes.
- **P-055.2 / T-2.2 (GREEN).** Implementación mínima del detector: `mysqldump --no-data` + `CHECKSUM TABLE … EXTENDED`, plegado en el hash con los mismos dominios que PostgreSQL. Sin refactorización todavía. Ejecutar tests y `cargo check`.
- **P-055.3 / T-2.3 (GREEN).** Integrar el detector en `detector_de_produccion` y en el aviso de degradación de `ejecucion.rs`. Reutilizar etiquetado y retención sin tocarlos. Añadir/ajustar el test de orquestación que demuestre que el manifest llega al archivado y que su fallo no aborta.
- **P-055.4 / T-2.4 (REFACTOR).** Factorizar la lógica común de hash y manejo de errores compartida con PostgreSQL sin cambiar su comportamiento. `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `just check-all`. Escribir `docs/FEATURE-055-mysql-change-detector.md`.
- **P-055.5 (AUDIT).** `@rust-reviewer` revisa el detector, la integración y los tests; si señala fallos, devolver a `@backend-dev` con los logs hasta que el build y los tests estén verdes.
- **P-055.6 / T-4.1 (CIERRE).** Marcar T-2.1…T-2.4 y T-4.1, ejecutar `openspec archive dedup-manifest`, `git mv plans/PLAN-055.md plans/PLAN-055-COMPLETED.md`, PR a `development`, y cerrar el issue #206 referenciando el merge.

## 6. Verification / Definition of Done

- [x] Los tests RED fallaron por la ausencia del detector antes de implementarlo (evidencia en CLI).
- [ ] `cargo test` filtrado sobre `extraccion.rs` y `ejecucion.rs` en verde.
- [ ] `cargo fmt --all -- --check` y `cargo clippy --workspace --all-targets -- -D warnings` sin salida de error.
- [ ] `cargo test --locked --workspace` y `just check-all` en verde.
- [ ] `@rust-reviewer` aprueba el cambio.
- [ ] `openspec/changes/dedup-manifest/tasks.md` completamente marcado y change archivado.
- [ ] `docs/FEATURE-055-mysql-change-detector.md` creado; `HANDOFF.md` actualizado.
- [ ] PR fusionada a `development` e issue #206 cerrado.

## 7. Fuera de alcance

- Cambios en PostgreSQL, API, panel, esquema SQLite o variables de entorno.
- Detección por binlog position.
- Tests de integración contra un servidor MySQL real (se usa el inyector, igual que en Fase 1).
