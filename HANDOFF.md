# Relevo — FEATURE-014 (SQLite como motor de copia) lista en PR; #11 en curso

`main` en **0.5.2**; `development` sin cambios nuevos en esta sesión. Trabajo de esta sesión en la rama **`feature/sqlite-copy-engine`** (issue **#11**), con `plans/PLAN-014.md` y el change `openspec/changes/sqlite-copy-engine/`.

## Lo hecho en esta sesión

### FEATURE-014 — SQLite como motor de copia (#11)

- Ciclo SDD + TDD completo en la rama `feature/sqlite-copy-engine`.
- **Nuevo `target_type = 'sqlite'`** con `container_name` + `db_path` (ruta absoluta del `.db` dentro del contenedor); no usa `db_user`/`db_password`/`db_name`/`host_path`.
- **Copia consistente en caliente**: `sqlite3 -readonly <db_path> .dump` dentro del contenedor, streaming a `rustic`; válida con WAL y con la base en uso; sin ficheros temporales. El `-readonly` cierra el éxito silencioso (sin él, una ruta inexistente creaba una base vacía y salía con 0).
- **Migración `0003_sqlite_target.sql`**: reconstruye `backup_targets` (añade `db_path`, amplía el `CHECK`), preservando filas e índices y con `foreign_keys` bajo control.
- **Panel**: opción `sqlite` en el formulario de fuentes, con sus campos condicionales.
- **Docs**: `docs/DEFINITION.md` §3/§4, ejemplo en `compose.yaml`, `docs/FEATURE-014-sqlite-copy-engine.md`.
- Code review (rust-reviewer + react-reviewer): **sin bloqueantes**; los hallazgos menores se corrigieron.

## Verificado

- `just check-all` verde: **441 + 5** tests backend, **128** tests UI, `tsc`, `eslint` y `vite build` OK.
- `openspec validate sqlite-copy-engine --strict` en verde.

## Pendiente / siguiente

1. **PR de `feature/sqlite-copy-engine` a `development`** y CI en verde; luego cerrar #11 con el merge.
2. **Verificación manual pendiente:** copia real de una base SQLite en uso con WAL, restaurada sin errores. Exige un motor de contenedores accesible (no disponible en este entorno). El e2e equivalente en `src/engine/mod.rs` está bajo `#[ignore]`; conviene añadir una variante SQLite al arnés e2e (hoy solo cubre `mariadb-dump`).
3. Al fusionar: `openspec archive sqlite-copy-engine` está hecho en la rama; renombrar `plans/PLAN-014.md` a `plans/PLAN-014-COMPLETED.md` (también en la rama).
4. Recordatorios heredados: forzar `podman pull` al verificar tags; el CI no levanta daemon de contenedores.
