# Proposal

## Why

El workflow de release `.github/workflows/release.yml` publica el tag `latest` en GHCR **en cada** ejecución, sin comprobar que la versión publicada sea la más alta. Cuando dos releases se construyen a la vez —habitual porque la imagen arm64 se compila bajo QEMU y tarda horas—, gana la que termina última, que no tiene por qué ser la más nueva.

Ha ocurrido ya: los runs de 0.3.2, 0.3.3 y 0.4.0 se solaparon; 0.3.3 terminó a las 08:10, después que 0.4.0 (08:03), y dejó `ghcr.io/atareao/kesto:latest` apuntando al digest de 0.3.3. Un despliegue con `image: ghcr.io/atareao/kesto:latest` —el que fija el §8 del SRS y el `compose.yaml` versionado— recibe así una versión **antigua**.

Issue de entrada: [#32](https://github.com/atareao/kesto/issues/32).
Plan: `plans/PLAN-010.md`.

## What Changes

- En `.github/workflows/release.yml`, la publicación de `latest` pasa a ser **condicional**: solo se publica cuando el tag de la release es la versión semver más alta ya publicada. `type=raw,value=latest` se condiciona con un `enable` calculado por un paso previo, en vez de publicarse siempre.
- Se añade un **grupo de concurrencia** al job de imagen para que dos releases no se construyan a la vez y no compitan por `latest`.
- **Remedio inmediato:** se reetiqueta `ghcr.io/atareao/kesto:latest` al digest de 0.4.0 para dejar el registro en el estado correcto hoy.
- No se toca código Rust ni el panel: es el pipeline de publicación.

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `deploy`: se añade el requisito de que `:latest` resuelva **siempre** al manifiesto de la release más alta, y de que builds solapados no puedan degradarlo.

## Impact

- **Ficheros:** `.github/workflows/release.yml`, `scripts/latest-tag-guard.sh` (+ su test), `docs/FEATURE-010-latest-tag-guard.md`.
- **Registro:** GHCR — la etiqueta `latest` (remediada a mano una vez).
- **Sin cambios** en la imagen, el binario, la API, el panel ni el `compose.yaml`.