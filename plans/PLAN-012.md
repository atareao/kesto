# PLAN-012 — La imagen publica el binario real (fix del mtime con COPY)

- **Issue:** [#41](https://github.com/atareao/kesto/issues/41)
- **Change OpenSpec:** `openspec/changes/dockerfile-copy-mtime/`
- **Estado:** en curso

## Objetivo

Corregir el `Dockerfile` para que la imagen publicada contenga el binario real del crate, y repoblar el registro con una imagen correcta (`X.Y.Z` + `latest`).

## Diagnóstico

- `COPY` de Docker preserva el mtime del contexto.
- El `src/main.rs` real (checkout) es más antiguo que el dummy creado durante el build → Cargo no recompila.
- Evidencia: el build real del job `Image (amd64)` de v0.5.1 tardó **0.42 s** (nada compilado).

## Alcance

1. `Dockerfile`: `RUN find src -type f -exec touch {} +` antes del build real.
2. `scripts/release-image-workflow.test.sh`: guard estático del `touch`.
3. Republicar la imagen con una release nueva.

## Fuera de alcance

- Cambiar el resto del pipeline (matriz, manifest, sccache).
- Recuperar el tag `v0.5.1` (se quedará sin imagen; la release nueva lo cubre).

## Tareas

- [ ] Aprobar el change proposal.
- [ ] Rama `hotfix/dockerfile-copy-mtime` desde `main`.
- [ ] RED: guard estático en el test estructural.
- [ ] GREEN: `touch` en el `Dockerfile`.
- [ ] Verificar build local + `just check` + `just scripts-test`.
- [ ] Release nueva y verificar el manifest (amd64 + arm64, `latest` == `X.Y.Z`).
- [ ] `docs/FEATURE-012-*.md`, renombrar el plan y archivar el change.

## Criterios de aceptación

- La imagen publicada arranca y supera el smoke test.
- `latest` y `X.Y.Z` resuelven al mismo manifest list con `amd64` + `arm64`.
- El test estructural falla si se quita el `touch`.

## Riesgos

- El `touch` invalida también la huella de las dependencias si se coloca mal; debe ir **después** del `COPY` del código y **antes** del build real, sin tocar `Cargo.toml`/`Cargo.lock`.
- La caché de `sccache` puede devolver artefactos del dummy: `sccache` cachea por hash de fuente, así que el dummy y el real tienen claves distintas; no hay riesgo de mezcla.
