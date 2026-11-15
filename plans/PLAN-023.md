# PLAN-023 — Batería e2e de copia y restauración

- **Issue**: #86
- **Rama**: `feature/bateria-e2e-de-copia-y-restauracion`
- **Change SDD**: `openspec/changes/bateria-e2e-de-copia-y-restauracion/`
- **Estado**: redactado, pendiente de aprobación de la especificación

## Objetivo

Verificar de punta a punta que el ciclo **copia → S3 real → restauración** funciona por cada tipo de origen (mariadb, postgres, sqlite, directorio), que los datos restaurados coinciden con los sembrados y que la batería corre en CI, sustituyendo la verificación manual del `lab/`.

## Punto de partida (hechos)

- 27 tests `#[ignore]` cubren extracción, `docker exec`, túnel y ramas del motor, pero **nunca se ejecutan en CI**.
- Todo test usa un almacén local (`abridor_local`); el único doble remoto es un endpoint caído.
- `Motor::restaurar_en_el_contenedor` (`src/engine/mod.rs:1077`) sin e2e.
- Camino S3 → `volcar_a` → `LectorDelVolcado` → `stdin` no ejercido.
- Sobrescritura de destino no cubierta.
- Verificación con S3 real solo manual (RustFS/SeaweedFS + `lab/`).

## Fases

### Fase 1 — Arnés y esqueleto (RED)
`tests/e2e/` con resolución de `DOCKER_HOST`, arranque de SeaweedFS y espera con plazo. Sin motor o sin S3, falla nombrando lo que falta.

### Fase 2 — Escenarios por tipo de origen (RED → GREEN)
Cuatro escenarios (mariadb, postgres, sqlite, directorio) con comparación de datos real y un caso de diferencia deliberada que debe romper la prueba.

### Fase 3 — Huecos del inventario
Restauración hacia contenedor, S3→stdin, contenedor que no drena `stdin`, sobrescritura con y sin confirmación, retención y purga contra S3, credencial incorrecta sin fuga en el motivo.

### Fase 4 — CI y receta local
Job `e2e` en `ci.yml` aislado del job `backend`; receta `just e2e`; los 27 `#[ignore]` se ejecutan en el job.

### Fase 5 — Cierre
`cargo fmt` + `clippy -D warnings` + `cargo test --locked` + `just check-all`; `docs/FEATURE-023-…`; PR a `development`.

## Decisiones

- **S3 de referencia: SeaweedFS.** MinIO quedó descartado por no ser descargable sin credenciales; `moto` se usó en backup-engine pero no sirve como destino S3 real de rustic.
- **Sin dependencias nuevas en `Cargo.toml`.** Los contenedores se orquestan desde el propio test con el cliente que ya existe.
- **Los `#[ignore]` se conservan.** La batería los ejecuta; no se reescribe su marcado.
- **La batera no se salta nunca.** Un entorno incompleto es un fallo, no un skip.

## Riesgos

- **Descarga de imágenes en CI**: se fijan versiones y se acota el plazo de espera; un fallo de red se ve como fallo del job, no como éxito.
- **Tiempo de ejecución**: el job `e2e` es más lento; se mantiene separado de `backend` para no penalizar el ciclo rápido.
- **Los escenarios pueden destapar defectos reales** en `restaurar_en_el_contenedor` o en `volcar_a`: se corrigen en `src/` dentro de esta feature (tarea 2.3), no se maquillan en el test.

## Verificación

- [ ] `just e2e` verde en local contra podman.
- [ ] Job `e2e` verde en CI, ejecutando batería y `#[ignore]`.
- [ ] Una diferencia de datos rompe la prueba de forma visible.
- [ ] Sin motor o sin S3, la batería falla nombrando lo que falta.
- [ ] `just check` y `just check-all` verdes.