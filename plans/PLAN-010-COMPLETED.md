# PLAN-010 — Publicación de `latest` a prueba de carreras de release

**Issue:** [#32](https://github.com/atareao/kesto/issues/32)
**Change OpenSpec:** `openspec/changes/latest-tag-guard/` (capability `deploy`)
**Estado:** completado

## Contexto

`compose.yaml` usa `image: ghcr.io/atareao/kesto:latest` (contrato del §8). El workflow `.github/workflows/release.yml` publica `latest` incondicionalmente en cada release, con `type=raw,value=latest` (línea 142). Con builds multi-arquitectura bajo QEMU que tardan horas, varias releases se solapan y **gana la que termina última**. En la tanda 0.3.2/0.3.3/0.4.0, `latest` quedó en 0.3.3 en vez de 0.4.0.

## Objetivo

Que `:latest` resuelva siempre al manifiesto de la release más alta, y que la construcción de una release antigua no pueda degradarlo.

## Alcance

- **Sí:** `.github/workflows/release.yml`, un script de guard `scripts/latest-tag-guard.sh` con su test, el reetiquetado puntual de `latest`, y la documentación de la feature.
- **No:** código Rust, panel, `compose.yaml`, la imagen en sí.

## Tareas

1. **Preparación** — confirmar issue #32; medir la línea base (`latest` = 0.3.3).
2. **Guard (TDD)** — test que falle (RED), script `latest-tag-guard.sh` que decida por semver (GREEN), encaje condicional de `latest` en el workflow y grupo de concurrencia.
3. **Remedio** — reetiquetar `latest` al digest de 0.4.0.
4. **Documentación y cierre** — `docs/FEATURE-010-latest-tag-guard.md`, `just check`, `openspec validate --strict`, archivar el change y cerrar el plan.

## Verificación

- `scripts/latest-tag-guard.test.sh`: tag más alto → `true`; tag antiguo → `false`; única release → `true`.
- `podman manifest inspect ghcr.io/atareao/kesto:latest` == digest de `0.4.0`.
- `just check` verde y `openspec validate latest-tag-guard --strict` → «valid».

## Follow-ups

- Valorar si `compose.yaml` debería fijar una versión concreta en vez de `latest` para despliegues reproducibles (queda fuera de este plan).
- Repetir el patrón de guard si en el futuro se publican artefactos con alias de canal (por ejemplo `stable`).

## Follow-ups abiertos

- **Remedio del registro (tarea 2.1):** `ghcr.io/atareao/kesto:latest` quedó en 0.3.3; reetiquetar a 0.4.0 requiere credenciales con `write:packages` (el token `gho_` de `gh` no las tiene). Comando: `skopeo copy --all docker://ghcr.io/atareao/kesto:0.4.0 docker://ghcr.io/atareao/kesto:latest`. Documentado en `docs/FEATURE-010-latest-tag-guard.md`.
