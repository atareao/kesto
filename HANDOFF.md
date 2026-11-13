
# Relevo — release 0.5.0 publicada (crate + GitHub Release); imagen en construcción

`main` y `development` en **0.5.0**. Tag `v0.5.0`. Issue **#32 cerrado**.

## Lo hecho en esta sesión

1. **Verificación del handoff anterior:** los tres builds de imagen (0.3.2/0.3.3/0.4.0) en success; se **detectó** que `latest` apuntaba a 0.3.3 por una carrera en `release.yml`.
2. **Fix del guard de `latest`** (issue #32): PR #33 (fix, TDD) + PR #34 (cierre: delta fundido, change archivado, `docs/FEATURE-010-latest-tag-guard.md`, `plans/PLAN-010-COMPLETED.md`). Fusionados en `development`.
3. **Incidente de GitHub:** el merge `development→main` (PR #35) devolvió **502** y GitHub **perdió el evento de push**: `main` quedó con el contenido (#164ee75) pero **no** disparó `release-prepare` ni actualizó el estado del PR (quedó OPEN). No existía forma de reintentar (sin `workflow_dispatch`).
4. **Recuperación:** PR #36 añade `workflow_dispatch` a `release-prepare.yml`; se cerró el PR #35 atascado; PR #37 `development→main` reintentó y **sí** disparó `release-prepare` → **0.5.0** (minor por el `feat` de #33).
5. **`release.yml`** con `v0.5.0`: **Crate ✅** (crates.io `0.5.0`), **GitHub Release ✅** `v0.5.0` (Latest). El paso `Compute latest tag guard` del job de imagen **se ejecutó con éxito**; el build multi-arch sigue **en construcción**.

## Verificado

- crates.io: `kesto` max_version **`0.5.0`**.
- GitHub Release **v0.5.0** (Latest).
- `development` sincronizado (`Cargo.toml` y `.vampus.yml` en 0.5.0).
- `just check` verde (425 + 5); `openspec validate --specs --strict` → 12 passed; CI de #33/#34/#36/#37 en verde.

## Pendiente / siguiente

1. **Verificar `latest` al acabar el build de imagen** (es lo que valida el fix): `podman manifest inspect ghcr.io/atareao/kesto:latest` debe coincidir con `:0.5.0` (digest amd64+arm64). Si el build falla, `latest` seguiría en 0.3.3.
2. **Follow-ups del deploy** (`docs/FEATURE-008-…`): `HSTS` si el TLS no lo termina un proxy; backup del volumen `kesto_data`. El del socket ya está hecho.
3. **Nota:** la prueba del socket **real** no se pudo ejecutar aquí (sin daemon accesible); se validó por `podman compose config`.

## Avisos

- **Trabajo ajeno sin commitear** (NO incluir en PRs): `AGENTS.md`, `GIT_FLOW.md`, `docs/{ARQUITECTURA,COMANDOS,DECISIONES-SRS,ENTORNO,ESTADO}.md` y `assets/`. Sin tocar.
- Overrides de pnpm en `ui/` → van en **`pnpm-workspace.yaml`**.
- **Incidente GitHub (502) + pérdida de evento de push:** si un merge a `main` no dispara `release-prepare`, ahora se puede relanzar con `gh workflow run release-prepare.yml --ref main`.
