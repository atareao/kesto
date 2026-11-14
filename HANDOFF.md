# Relevo — v0.6.0 publicada; backlog vacío, verificación e2e de SQLite pendiente

`main` y `development` en **`2b47f8b`** (`chore: release v0.6.0`), versión **0.6.0**, tag **`v0.6.0`**. Sin PRs ni issues abiertos.

## Lo hecho en esta sesión

### FEATURE-014 — SQLite como motor de copia (#11)

- Ciclo SDD + TDD completo; PR **#48** mergeado. Change `sqlite-copy-engine` archivado y `plans/PLAN-014-COMPLETED.md`.
- **Nuevo `target_type = 'sqlite'`** con `container_name` + `db_path` (ruta absoluta del `.db` dentro del contenedor); no usa `db_user`/`db_password`/`db_name`/`host_path`.
- **Copia consistente en caliente**: `sqlite3 -readonly <db_path> .dump` dentro del contenedor, streaming a `rustic`; válida con WAL y con la base en uso; sin ficheros temporales. El `-readonly` cierra el éxito silencioso (sin él, una ruta inexistente creaba una base vacía y salía con 0).
- **Migración `0003_sqlite_target.sql`**: reconstruye `backup_targets` (añade `db_path`, amplía el `CHECK`), preservando filas e índices y con `foreign_keys` bajo control.
- **Panel**: opción `sqlite` en el formulario de fuentes, con sus campos condicionales.
- **Docs**: `docs/DEFINITION.md` §3/§4, ejemplo en `compose.yaml`, `docs/FEATURE-014-sqlite-copy-engine.md`.
- Code review (rust-reviewer + react-reviewer): **sin bloqueantes**; los hallazgos menores se corrigieron.

### Release v0.6.0

- PR de release **#50** (`development → main`) mergeado. `Prepare Release` subió **0.5.2 → 0.6.0**, escribió el CHANGELOG, commiteó y etiquetó `v0.6.0`.
- El tag disparó `release.yml` (**success**): imagen publicada en **ghcr.io** y crate en **crates.io** (`kesto` `max_version: 0.6.0`). GitHub Release **"kesto v0.6.0"** creada.
- Publica de golpe **FEATURE-013** (HSTS opcional + `just backup-datos`) y **FEATURE-014** (SQLite).

## Verificado

- `just check-all` verde: **441 + 5** tests backend, **128** tests UI, `tsc`, `eslint` y `vite build` OK.
- `openspec validate --specs --strict` → 12 passed.
- CI de `main` y `development` en verde; workflow **Release** v0.6.0 en verde.

## Pendiente / siguiente

1. **Verificación manual (lo único abierto):** copia real de una base SQLite **en uso** con WAL, restaurada sin errores. Exige un motor de contenedores accesible (no disponible en este entorno). Follow-up recomendado: añadir una variante SQLite al e2e `#[ignore]` de `src/engine/mod.rs` (hoy solo cubre `mariadb-dump`).
2. **Backlog:** vacío (sin issues abiertos). Ideas anotadas en `docs/FEATURE-014-sqlite-copy-engine.md`: copia binaria con `.backup` (API `sqlite3_backup`) como alternativa al `.dump` lógico, y Litestream como complemento de replicación continua.
3. Al abrir una feature nueva: empezar por su **GitHub Issue**, luego `plans/PLAN-015.md` y el change SDD.
4. Recordatorios heredados: forzar `podman pull` al verificar tags; el CI no levanta daemon de contenedores.
