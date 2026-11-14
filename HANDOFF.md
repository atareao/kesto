# Relevo — FEATURE-014 (SQLite) mergeada y #11 cerrado; verificación manual pendiente

`main` en **0.5.2**; `development` en **1139b1c** (merge del PR **#48**, FEATURE-014). Issue **#11** cerrado. **`main` todavía no incluye FEATURE-014**: el próximo merge `development → main` la publica (bump automático por los conventional commits).

## Lo hecho en esta sesión — FEATURE-014: SQLite como motor de copia (#11)

- Ciclo SDD + TDD completo; change `sqlite-copy-engine` archivado y `plans/PLAN-014-COMPLETED.md`.
- **Nuevo `target_type = 'sqlite'`** con `container_name` + `db_path` (ruta absoluta del `.db` dentro del contenedor); no usa `db_user`/`db_password`/`db_name`/`host_path`.
- **Copia consistente en caliente**: `sqlite3 -readonly <db_path> .dump` dentro del contenedor, streaming a `rustic`; válida con WAL y con la base en uso; sin ficheros temporales. El `-readonly` cierra el éxito silencioso (sin él, una ruta inexistente creaba una base vacía y salía con 0).
- **Migración `0003_sqlite_target.sql`**: reconstruye `backup_targets` (añade `db_path`, amplía el `CHECK`), preservando filas e índices y con `foreign_keys` bajo control.
- **Panel**: opción `sqlite` en el formulario de fuentes, con sus campos condicionales.
- **Docs**: `docs/DEFINITION.md` §3/§4, ejemplo en `compose.yaml`, `docs/FEATURE-014-sqlite-copy-engine.md`.
- Code review (rust-reviewer + react-reviewer): **sin bloqueantes**; los hallazgos menores se corrigieron.

## Verificado

- `just check-all` verde: **441 + 5** tests backend, **128** tests UI, `tsc`, `eslint` y `vite build` OK.
- `openspec validate --specs --strict` → 12 passed.
- **CI del PR #48 en verde** (Backend, Frontend, Preflight).

## Pendiente / siguiente

1. **Verificación manual:** copia real de una base SQLite **en uso** con WAL, restaurada sin errores. Exige un motor de contenedores accesible (no disponible en este entorno). Follow-up recomendado: añadir una variante SQLite al e2e `#[ignore]` de `src/engine/mod.rs` (hoy solo cubre `mariadb-dump`).
2. **Release:** FEATURE-014 vive en `development`; el próximo merge `development → main` la publica.
3. Recordatorios heredados: forzar `podman pull` al verificar tags; el CI no levanta daemon de contenedores.
