# Tasks — SEC-19 / SEC-20 / SEC-21 (TDD)

> Protocolo: RED → GREEN → REFACTOR por cada hallazgo. Nada de código antes de aprobar el spec.
> Checks previos a cada commit: `just check` (fmt + clippy + test sobre el workspace).

## SEC-19 — Paginación de listados (`targets`, `repositories`, `jobs`, `tokens`)

### RED
- [ ] Test de contrato: `GET /api/v1/targets` sin parámetros devuelve wrapper `{items,total,limit,offset}` con `limit` = defecto.
- [ ] Test: `GET /api/v1/repositories`, `/jobs`, `/tokens` devuelven el mismo wrapper con el mismo defecto.
- [ ] Test: `limit`/`offset` explícitos recortan la ventana; `total` refleja el total real, no el tamaño de la página.
- [ ] Test: `limit` por encima del máximo se rechaza con `400` (o se sanea según contrato del spec) — verificar escenario elegido.
- [ ] Test: `limit=0`, `limit` negativo y `offset` negativo → `400` con mensaje que nombra el parámetro.
- [ ] Test: `offset` mayor que `total` → `200` con `items` vacío y `total` correcto.
- [ ] Test: página vacía (catálogo sin filas) → `200`, `items: []`, `total: 0`.
- [ ] Ejecutar `cargo test --locked` → confirmar ROJO en los tests nuevos y VERDE en los legacy.

### GREEN
- [ ] Introducir tipo/wrapper paginado común reutilizable por los cuatro endpoints.
- [ ] Extraer y sanear `limit`/`offset` de la query con defecto y máximo.
- [ ] Aplicar la ventana en la consulta/mapeo de los cuatro listados.
- [ ] `cargo test --locked` → VERDE; `cargo check` sin errores.

### REFACTOR
- [ ] Unificar la construcción del wrapper y la validación de parámetros (sin duplicar lógica).
- [ ] Ajustar el panel UI para consumir el wrapper paginado.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` y `cargo fmt --all -- --check` limpios.
- [ ] Actualizar `docs/DEFINITION.md` §5 y el doc de feature con el nuevo contrato.

## SEC-20 — Acciones de CI fijadas a SHA

### RED
- [ ] Añadir/ajustar una comprobación (test de `scripts/` o job de CI) que falle si algún `uses:` no apunta a un SHA de 40 hex.
- [ ] Ejecutar la comprobación → ROJO contra los workflows actuales.

### GREEN
- [ ] Resolver el SHA de commit de cada acción usada en `.github/workflows/*`.
- [ ] Sustituir cada `@tag` por `@<sha40> # <tag>` en todos los workflows.
- [ ] Comprobación → VERDE.

### REFACTOR
- [ ] Documentar en `GIT_FLOW.md` el procedimiento de actualización de pins.
- [ ] Verificar que el YAML sigue siendo válido y los jobs no cambian de comportamiento.

## SEC-21 — `/health` sin fuga de metadatos

### RED
- [ ] Test: `GET /api/v1/health` público NO incluye `version`.
- [ ] Test: `GET /api/v1/health` público NO incluye `auth_mode`.
- [ ] Test: `GET /api/v1/health` sigue devolviendo `status` (y código `200` cuando está sano).
- [ ] Ejecutar `cargo test --locked` → ROJO en los tests nuevos.

### GREEN
- [ ] Recortar el payload del handler de health a `{status}`.
- [ ] `cargo test --locked` → VERDE; `cargo check` limpio.

### REFACTOR
- [ ] Revisar que ningún probe de compose/docs depende de `version`/`auth_mode`.
- [ ] `cargo clippy` / `cargo fmt` limpios; actualizar docs de feature.

## Cierre
- [ ] `just check` verde sobre el workspace.
- [ ] `just check-all` verde (si aplica) antes de PR.
- [ ] Crear `docs/FEATURE-NNN-<slug>.md` y archivar el change en `openspec/specs/`.
