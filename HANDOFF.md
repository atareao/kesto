# Relevo — release v0.5.2: la imagen publica el binario real (fix del mtime con COPY)

`main` y `development` en **0.5.2**. Tag `v0.5.2` (patch). Issues **#38** y **#41** cerrados. Imagen `latest` == `0.5` == `0.5.2` (manifest list `amd64`+`arm64`, arranca y sirve).

## Lo hecho en esta sesión

Sesión en dos tramos: cerrar la feature #38 (imagen de release rápida) y corregir el bug que esa feature destapó.

### Tramo 1 — FEATURE-011: imagen de release rápida (#38)

1. Change `release-image-rapida`; PR #39 (feature→development) y PR #40 (development→main), ambos con CI verde.
2. `release.yml` desdoblado en `preflight, image(matrix), smoke, manifest, publish, release`: runners **nativos** `ubuntu-26.04`/`ubuntu-26.04-arm`, publicación `push-by-digest=true` por arquitectura, ensamblado del manifest list con `docker buildx imagetools create`. `Dockerfile`: `sccache` (`RUSTC_WRAPPER=sccache`, backend GHA solo con secretos) + capa de dependencias.
3. Release **v0.5.1**: imágenes nativas rapidísimas (amd64 9m43s, arm64 9m31s, frente a ~2 h), pero el **smoke test falló** → el job `manifest` quedó `skipped`; `:latest` siguió en **0.3.3**.

### Tramo 2 — Hotfix del mtime (#41)

1. Diagnóstico: `COPY` de Docker **preserva el mtime**; el `src/main.rs` del checkout era más antiguo que el `fn main() {}` dummy del build → Cargo daba el crate por fresco y **no recompilaba** (2ª compilación: **0.42 s**) → se publicaba el binario vacío.
2. Change `dockerfile-copy-mtime`; rama `hotfix/dockerfile-copy-mtime` desde `main` (gitflow); RED→GREEN; PR **#42** mergeado a `main`.
3. Fix: `RUN find src -type f -exec touch {} +` antes del build real, más un **guard estático** (comprobación nº 7 de `scripts/release-image-workflow.test.sh`).
4. Release **v0.5.2**: Image amd64 **10m56s**, arm64 **14m32s**; **smoke ✅**, **manifest ✅**.

## Verificado

- `podman manifest inspect ghcr.io/atareao/kesto:{latest,0.5,0.5.2}` → mismo index `application/vnd.oci.image.index.v1+json` con `amd64` + `arm64` (digest amd64 idéntico).
- `:latest` arranca (2 s) y `GET /api/v1/health` → `200`, `"version":"0.5.2"`.
- crates.io `kesto` **0.5.2**; GitHub Release **v0.5.2**.
- `just check` verde (425 + 5); `just scripts-test` verde (incluye el nuevo guard).

## Pendiente / siguiente

1. Fitur/cierre: `docs/FEATURE-012-dockerfile-copy-mtime.md`, change archivado (`openspec/changes/archive/2026-11-13-dockerfile-copy-mtime/`), plan renombrado a `plans/PLAN-012-COMPLETED.md` (todo en el PR de cierre `docs/feature-012-cierre`).
2. Follow-ups del deploy (FEATURE-008): `HSTS` si el TLS lo termina un proxy; backup del volumen `kesto_data`.
3. La prueba del socket **real** no se pudo ejecutar aquí (sin daemon accesible); se validó por `podman compose config`.
4. Recordatorio: `podman run` **no** re-pule si la imagen existe en local; al verificar tags, forzar `podman pull` antes.
